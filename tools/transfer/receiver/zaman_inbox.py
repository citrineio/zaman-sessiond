"""Durable ROM inbox. Transport credentials never enter SQLite or metadata."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shlex
import shutil
import sqlite3
import stat
import threading
import time
import tomllib
import uuid
import zipfile

ID = re.compile(r"^[a-z0-9][a-z0-9_-]{0,31}$")
TERMINAL = {"complete", "failed", "cancelled", "attached"}


def relative_name(value):
    if not isinstance(value, str) or not value or len(value.encode()) > 1024:
        raise ValueError("Invalid filename")
    if any(c in value for c in ('\\', '"', '$', '`')) or any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("Unsafe filename")
    p = PurePosixPath(value)
    if p.is_absolute() or any(x in ("", ".", "..") or x.startswith(".")
                              or len(x.encode()) > 220 for x in value.split("/")):
        raise ValueError("Unsafe filename")
    return value


def fsync_dir(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_text(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name("." + path.name + "." + uuid.uuid4().hex)
    try:
        with temp.open("x", encoding="utf-8") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temp, path)
        fsync_dir(path.parent)
    finally:
        temp.unlink(missing_ok=True)


def sha256(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        while b := f.read(1024 * 1024):
            h.update(b)
    return h.hexdigest()


class Registry:
    """Same ordered system override semantics as sessiond; never union layers."""
    def __init__(self, roots, rom_root):
        self.systems = {}
        self.paths = {}
        self.collections = {}
        self.shortnames = {}
        self.rom_root = Path(rom_root).resolve(strict=True)
        for root in roots:
            for path in sorted((Path(root) / "systems.d").glob("*.toml")):
                doc = tomllib.loads(path.read_text())
                s = doc["system"]
                if doc.get("schema") != 1 or not ID.fullmatch(s["id"]):
                    raise ValueError("Invalid system registry: " + str(path))
                if not s.get("extensions") or not s.get("name"):
                    raise ValueError("Incomplete system registry: " + str(path))
                self.systems[s["id"]] = s
        if not self.systems:
            raise ValueError("System registry is empty")
        for sid, system in self.systems.items():
            directory = self.rom_root / sid
            if directory.is_symlink():
                raise ValueError("Collection directory is a symlink: " + str(directory))
            self.paths[sid] = directory
            # A matching sessiond launch command establishes existing collection
            # ownership, independent of shortname (GameCube uses 'gc').
            collection = None
            shortname = sid
            meta = directory / "metadata.pegasus.txt"
            if meta.is_file():
                header = meta.read_text(errors="replace").splitlines()
                launch = ""
                for line in header:
                    key, sep, value = line.partition(":")
                    if key.lower() == "game":
                        break
                    if key.lower() == "collection":
                        collection = value.strip()
                    if key.lower() == "shortname":
                        shortname = value.strip()
                    if key.lower() == "launch":
                        launch = value.strip()
                try:
                    args = shlex.split(launch)
                except ValueError:
                    args = []
                if collection and args[:3] != ["/usr/local/bin/zamanctl", "launch", sid]:
                    raise ValueError("Collection launch ownership is unclear: " + str(meta))
            self.collections[sid] = collection or system["name"]
            self.shortnames[sid] = shortname

    def candidates(self, name):
        ext = PurePosixPath(name).suffix.lower().lstrip(".")
        return sorted(sid for sid, s in self.systems.items() if ext in s["extensions"])

    def inspect(self, path, name):
        candidates = self.candidates(name)
        evidence = "Registry extension match" if candidates else "Extension is not supported by the active registry"
        suffix = PurePosixPath(name).suffix.lower()
        if suffix == ".zip":
            with zipfile.ZipFile(path) as z:
                entries = z.infolist()
                if len(entries) > 4096 or sum(i.file_size for i in entries) > 32 * 1024**3:
                    raise ValueError("ZIP exceeds validation limits")
                for i in entries:
                    relative_name(i.filename.rstrip("/"))
                    if i.flag_bits & 1 or stat.S_ISLNK(i.external_attr >> 16):
                        raise ValueError("Encrypted or symlink ZIP entries are unsupported")
                    if i.file_size > max(64 * 1024**2, i.compress_size * 500):
                        raise ValueError("ZIP exceeds decompression ratio limit")
                bad = z.testzip()
                if bad:
                    raise ValueError("ZIP CRC failed")
                files = [i for i in entries if not i.is_dir()]
                # A single, uniquely identified ROM is evidence. Multiple binary
                # tracks and arcade ROM sets must not be guessed from extensions.
                if len(files) == 1:
                    inner = self.candidates(files[0].filename)
                    if len(inner) == 1 and inner[0] in candidates:
                        candidates = inner
                        evidence = "Single ZIP member identifies " + inner[0]
        elif suffix == ".7z":
            # No silent extraction or new dependency. Retain until a supported
            # validator is configured; sending ZIP/raw is the current fallback.
            return [], "7z validation is unavailable; resend as ZIP or uncompressed"
        return candidates, evidence


def descriptor_refs(path, name):
    suffix = PurePosixPath(name).suffix.lower()
    if suffix not in {".cue", ".m3u", ".gdi"}:
        return []
    if path.stat().st_size > 1024 * 1024:
        raise ValueError("Disc descriptor is too large")
    text = path.read_text(encoding="utf-8-sig", errors="strict")
    names = []
    if suffix == ".cue":
        for line in text.splitlines():
            if re.match(r"^\s*FILE\s", line, re.I):
                parts = shlex.split(line)
                if len(parts) < 3:
                    raise ValueError("Malformed CUE FILE entry")
                names.append(parts[1])
    elif suffix == ".m3u":
        names = [l.strip() for l in text.splitlines() if l.strip() and not l.lstrip().startswith("#")]
    else:
        lines = [l for l in text.splitlines() if l.strip()]
        count = int(lines[0])
        if count != len(lines) - 1:
            raise ValueError("Incomplete GDI descriptor")
        for line in lines[1:]:
            parts = shlex.split(line)
            if len(parts) != 6:
                raise ValueError("Malformed GDI track")
            names.append(parts[4])
    if not names:
        raise ValueError("Disc descriptor has no companion files")
    parent = PurePosixPath(name).parent
    return [relative_name(str(parent / relative_name(n))) for n in names]


class Inbox:
    def __init__(self, state, registry, metadata, reserve=1024**3, require_scrape=False):
        self.state = Path(state)
        self.state.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.stage = self.state / "staging"
        self.stage.mkdir(mode=0o700, exist_ok=True)
        self.registry, self.metadata, self.reserve = registry, Path(metadata), reserve
        self.require_scrape = require_scrape
        self.lock = threading.RLock()
        self.db = sqlite3.connect(self.state / "inbox.sqlite3", check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.executescript('''
          CREATE TABLE IF NOT EXISTS items (
            id TEXT PRIMARY KEY, batch TEXT NOT NULL, name TEXT NOT NULL,
            source TEXT NOT NULL, size INTEGER NOT NULL, received INTEGER NOT NULL DEFAULT 0,
            expected TEXT, digest TEXT, phase TEXT NOT NULL, candidates TEXT NOT NULL DEFAULT '[]',
            system TEXT, evidence TEXT NOT NULL DEFAULT '', error TEXT NOT NULL DEFAULT '',
            parent TEXT, target TEXT, manifest TEXT, created REAL NOT NULL,
            notified TEXT NOT NULL DEFAULT '', refresh INTEGER NOT NULL DEFAULT 0,
            external INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0);
          CREATE UNIQUE INDEX IF NOT EXISTS batch_name ON items(batch,name);
        ''')
        if "hidden" not in {column[1] for column in self.db.execute("PRAGMA table_info(items)")}:
            self.db.execute("ALTER TABLE items ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0")
        columns = {column[1] for column in self.db.execute("PRAGMA table_info(items)")}
        for name, default in (("scrape_choice", "''"), ("scrape_job", "''"), ("scraped", "'{}'"), ("artwork_warning", "''")):
            if name not in columns:
                self.db.execute("ALTER TABLE items ADD COLUMN " + name + " TEXT NOT NULL DEFAULT " + default)
        if require_scrape:
            self.db.execute("UPDATE items SET phase='needs-scrape' WHERE phase='ready' AND scrape_choice=''")
        self.db.execute("UPDATE items SET phase='failed', error='Network reception was interrupted; resend the file' WHERE phase IN ('offered','receiving')")
        self.db.commit()
        # fsynced .ready files bridge the receive/SQLite crash boundary.
        for row in self.rows():
            if row["phase"] == "failed" and self.path(row["id"]).is_file():
                self.update(row["id"], phase="validating", error="")
            elif row["phase"] == "failed":
                self.path(row["id"],True).unlink(missing_ok=True)

    def rows(self):
        with self.lock:
            return [dict(r) for r in self.db.execute("SELECT * FROM items ORDER BY created,id")]

    def get(self, ident):
        with self.lock:
            row = self.db.execute("SELECT * FROM items WHERE id=?", (ident,)).fetchone()
            if row is None:
                raise ValueError("Import item no longer exists")
            return dict(row)

    def update(self, ident, **fields):
        allowed = {"received", "digest", "phase", "candidates", "system", "evidence", "error", "parent", "target", "manifest", "notified", "refresh", "external", "scrape_choice", "scrape_job", "scraped", "artwork_warning"}
        if not fields or not set(fields) <= allowed:
            raise ValueError("Invalid inbox update")
        with self.lock:
            self.db.execute("UPDATE items SET " + ",".join(k + "=?" for k in fields) + " WHERE id=?", (*fields.values(), ident))
            self.db.commit()

    def path(self, ident, partial=False):
        if not re.fullmatch(r"[a-f0-9]{32}", ident):
            raise ValueError("Invalid import ID")
        return self.stage / (ident + (".part" if partial else ".ready"))

    def transition(self, ident, phases, **fields):
        with self.lock:
            if self.get(ident)["phase"] not in phases:
                return False
            self.update(ident, **fields)
            return True

    def offer(self, files, source):
        """One transaction, bounded manifest and aggregate capacity reservation."""
        if not isinstance(files, dict) or not 1 <= len(files) <= 256:
            raise ValueError("Send between 1 and 256 files per batch")
        batch = uuid.uuid4().hex
        prepared = []
        seen = set()
        for fid, f in files.items():
            if not isinstance(fid, str) or not 1 <= len(fid) <= 256 or not isinstance(f, dict):
                raise ValueError("Invalid file metadata")
            name = relative_name(f.get("fileName"))
            size = f.get("size")
            if type(size) is not int or not 0 < size <= 2**63 - 1:
                raise ValueError("Invalid file size")
            expected = f.get("sha256") or ""
            if not isinstance(expected, str) or (expected and not re.fullmatch(r"[a-fA-F0-9]{64}", expected)):
                raise ValueError("Invalid SHA-256")
            if name.casefold() in seen:
                raise ValueError("Duplicate filename in batch")
            seen.add(name.casefold())
            prepared.append((fid, uuid.uuid4().hex, name, size, expected.lower()))
        with self.lock:
            pending = self.db.execute("SELECT count(*) FROM items WHERE phase NOT IN ('complete','failed','cancelled','attached')").fetchone()[0]
            if pending + len(prepared) > 1024:
                raise ValueError("Inbox is full; resolve or cancel pending imports")
            held = self.db.execute("SELECT coalesce(sum(size-received),0) FROM items WHERE phase IN ('offered','receiving')").fetchone()[0]
            if shutil.disk_usage(self.stage).free < held + sum(p[3] for p in prepared) + self.reserve:
                raise ValueError("Not enough staging storage")
            with self.db:
                for fid, ident, name, size, expected in prepared:
                    self.db.execute("INSERT INTO items(id,batch,name,source,size,expected,phase,created) VALUES(?,?,?,?,?,?,'offered',?)",
                                    (ident,batch,name,str(source)[:120],size,expected,time.time()))
        return batch, {fid: ident for fid, ident, *_ in prepared}

    def receive(self, ident, chunks, cancelled=lambda: False):
        row = self.get(ident)
        if row["phase"] != "offered":
            raise ValueError("File was already received or cancelled")
        self.update(ident, phase="receiving")
        digest, count, last = hashlib.sha256(), 0, 0.0
        part, ready = self.path(ident, True), self.path(ident)
        try:
            with part.open("xb") as f:
                for block in chunks:
                    if cancelled():
                        raise ValueError("Transfer cancelled")
                    count += len(block)
                    if count > row["size"]:
                        raise ValueError("Upload exceeds advertised size")
                    f.write(block)
                    digest.update(block)
                    if time.monotonic() - last > .25:
                        self.update(ident, received=count)
                        last = time.monotonic()
                if cancelled():
                    raise ValueError("Transfer cancelled")
                if count != row["size"]:
                    raise ValueError("Upload ended before advertised size")
                actual = digest.hexdigest()
                if row["expected"] and actual != row["expected"]:
                    raise ValueError("SHA-256 checksum mismatch")
                f.flush()
                os.fsync(f.fileno())
            os.replace(part, ready)
            fsync_dir(self.stage)
            self.update(ident, received=count, digest=actual, phase="validating", error="")
        except Exception as e:
            part.unlink(missing_ok=True)
            self.update(ident, received=count, phase="cancelled" if cancelled() else "failed", error=str(e))
            raise

    def validate_batch(self, batch):
        rows = [r for r in self.rows() if r["batch"] == batch]
        if any(r["phase"] in {"offered", "receiving"} for r in rows):
            return
        available = {r["name"]: r for r in rows if self.path(r["id"]).is_file()}
        refs, invalid = {}, {}
        for r in rows:
            if r["phase"] not in {"validating", "attached"}:
                continue
            try:
                path = self.path(r["id"])
                if path.stat().st_size != r["size"]:
                    raise ValueError("Staged length mismatch")
                if not r["digest"]:
                    actual = sha256(path)
                    if r["expected"] and actual != r["expected"]:
                        raise ValueError("Recovered checksum mismatch")
                    self.update(r["id"], digest=actual, received=r["size"])
                refs[r["name"]] = descriptor_refs(path, r["name"])
            except Exception as e:
                invalid[r["name"]] = str(e)
                self.transition(r["id"], {"validating","attached"}, phase="needs-files", error=str(e))
        # Process outer playlists first, then descriptors, then raw tracks.
        referenced = {n for names in refs.values() for n in names}
        for r in rows:
            if self.get(r["id"])["phase"] != "validating" or r["name"] in referenced:
                continue
            try:
                members, visiting = {}, set()
                def walk(name):
                    if name in visiting:
                        raise ValueError("Cyclic disc references")
                    if name not in available:
                        raise ValueError("Missing companion: " + name)
                    if name in invalid:
                        raise ValueError("Invalid companion " + name + ": " + invalid[name])
                    if name in members:
                        return
                    visiting.add(name)
                    member = available[name]
                    members[name] = member["id"]
                    for n in refs.get(name, []):
                        walk(n)
                    visiting.remove(name)
                walk(r["name"])
                candidates, evidence = self.registry.inspect(self.path(r["id"]), r["name"])
                if PurePosixPath(r["name"]).suffix.lower() == ".m3u":
                    # Playlist entries are launchable discs/descriptors, unlike
                    # the raw track payloads referenced by CUE/GDI descriptors.
                    entries = {n for name in members if PurePosixPath(name).suffix.lower() == ".m3u" for n in refs.get(name, [])}
                    for name in entries:
                        candidates = [sid for sid in candidates if sid in self.registry.candidates(name)]
                    if not candidates:
                        raise ValueError("Playlist entries have no common registry-supported system")
                chosen = r["system"] if r["system"] in candidates else candidates[0] if len(candidates) == 1 else None
                next_phase = ("needs-scrape" if self.require_scrape and not r["scrape_choice"] else "ready") if chosen else "needs-destination"
                changed = self.transition(r["id"], {"validating"}, phase=next_phase,
                            candidates=json.dumps(candidates), system=chosen,
                            evidence=evidence, manifest=json.dumps(members))
                if not changed:
                    continue
                for name, ident in members.items():
                    if ident != r["id"]:
                        self.update(ident, phase="attached", parent=r["id"])
            except Exception as e:
                self.transition(r["id"], {"validating"}, phase="needs-files", error=str(e))
        # Unclaimed referenced files are retained with an actionable explanation.
        for r in rows:
            if self.get(r["id"])["phase"] == "validating":
                self.transition(r["id"], {"validating"}, phase="needs-files", error="Resolve the disc descriptor and its companions together")

    def choose(self, ident, system):
        with self.lock:
            row = self.get(ident)
            if row["phase"] != "needs-destination" or system not in json.loads(row["candidates"]):
                raise ValueError("Destination is no longer available for this item")
            self.update(ident, system=system, phase="needs-scrape" if self.require_scrape else "ready", error="")

    def skip_scrape(self, ident):
        with self.lock:
            if self.get(ident)["phase"] != "needs-scrape":
                raise ValueError("This import is not waiting for a scraping decision")
            self.update(ident, scrape_choice="skip", phase="ready", error="")

    def retry(self, ident):
        with self.lock:
            row = self.get(ident)
            if row["phase"] != "needs-files" or not self.path(ident).is_file():
                raise ValueError("Only a staged import can be retried")
            self.update(ident, phase="validating", error="")

    def cancel(self, ident):
        with self.lock:
            row = self.get(ident)
            if row["phase"] in {"offered", "receiving", "importing", "complete", "attached"}:
                raise ValueError("Cancel the active sender session, or wait for publication")
            self.update(ident, phase="cancelled", error="Cancelled from Transfer")
            # This is an explicit confirmed import cancellation. Notification
            # dismissal and disabling reception never reach this path.
            members = json.loads(row["manifest"]) if row["manifest"] else {row["name"]:ident}
            needed = set()
            for other in self.rows():
                if other["id"] != ident and other["phase"] not in {"complete","cancelled","attached"} and other["manifest"]:
                    needed.update(json.loads(other["manifest"]).values())
            for item in members.values():
                if item not in needed:
                    self.path(item).unlink(missing_ok=True)
                    self.path(item,True).unlink(missing_ok=True)

    def dismiss_history(self, ident=None):
        # Complete rows own library metadata and descriptor relationships.
        # Hide terminal history only; never delete rows, staged files or ROMs.
        with self.lock:
            if ident is not None:
                row = self.get(ident)
                if row["phase"] not in {"complete", "failed", "cancelled"}:
                    raise ValueError("Only finished transfer records can be removed")
                with self.db:
                    self.db.execute("UPDATE items SET hidden=1 WHERE id=?", (ident,))
            else:
                with self.db:
                    self.db.execute("UPDATE items SET hidden=1 WHERE phase IN ('complete','failed','cancelled')")

    def history_count(self):
        with self.lock:
            return self.db.execute("SELECT count(*) FROM items WHERE hidden=0 AND phase IN ('complete','failed','cancelled')").fetchone()[0]

    def publish(self, ident):
        with self.lock:
            row = self.get(ident)
            if row["phase"] not in {"ready", "importing"}:
                return
            if self.require_scrape and row["scrape_choice"] not in {"skip", "done"}:
                raise ValueError("Choose scraping or import without scraping first")
            self.update(ident, phase="importing")
        sid = row["system"]
        if sid not in self.registry.systems or sid not in self.registry.candidates(row["name"]):
            raise ValueError("Registry changed; destination must be reviewed")
        members = json.loads(row["manifest"])
        records = {name: self.get(i) for name, i in members.items()}
        signature = json.dumps([(name, r["digest"]) for name, r in sorted(records.items())])
        content = hashlib.sha256(signature.encode()).hexdigest()
        directory = self.registry.paths[sid]
        directory.mkdir(mode=0o775, exist_ok=True)
        if directory.is_symlink() or directory.resolve().parent != self.registry.rom_root:
            raise ValueError("Unsafe collection path")
        # Deduplicate a title already covered by the existing collection's
        # metadata. Preserve its title/artwork and never write a second record.
        old_meta = directory / "metadata.pegasus.txt"
        existing = directory / row["name"]
        if len(records) == 1 and old_meta.is_file() and existing.is_file() and not existing.is_symlink():
            text = old_meta.read_text(errors="replace")
            extension = PurePosixPath(row["name"]).suffix.lstrip(".").lower()
            header = re.split(r"^game:", text, maxsplit=1, flags=re.M|re.I)[0]
            listed = any(line.strip() == "file: " + row["name"] for line in text.splitlines())
            extensions = [v.strip() for v in re.findall(r"^extensions?:\s*(.*)$", header, re.M|re.I) for v in v.split(",")]
            known = "ignore-" not in header and (listed or extension in extensions)
            if known and existing.stat().st_size == row["size"] and sha256(existing) == row["digest"]:
                self.update(ident, phase="complete", target=str(directory), external=1,
                            evidence="Already in the library; existing metadata preserved", refresh=1)
                self.path(ident).unlink(missing_ok=True)
                return
        target = directory / ("import-" + content)
        temporary = directory / (".zaman-import-" + ident)
        manifest_text = json.dumps({"version":1,"system":sid,"entry":row["name"],"files":{
            n:{"size":r["size"],"sha256":r["digest"]} for n,r in records.items()}}, sort_keys=True)
        self.update(ident, phase="importing", target=str(target))
        # Install the collection's exclusion before copying, so an unrelated
        # producer's rescan cannot expose our destination-side temporary tree.
        self.write_metadata()
        if target.exists():
            if target.is_symlink() or not (target / ".zaman-manifest.json").is_file() or (target / ".zaman-manifest.json").read_text() != manifest_text:
                raise ValueError("Import destination collision; nothing was overwritten")
            for name, record in records.items():
                p = target / name
                if not p.is_file() or p.is_symlink() or sha256(p) != record["digest"]:
                    raise ValueError("Previously published content changed; retained for review")
        else:
            if temporary.exists():
                # Only our generated private temporary directory, never a ROM.
                if temporary.is_symlink():
                    raise ValueError("Unsafe temporary import path")
                shutil.rmtree(temporary)
            temporary.mkdir(mode=0o775)
            try:
                total = sum(r["size"] for r in records.values())
                if shutil.disk_usage(directory).free < total + self.reserve:
                    raise ValueError("Not enough destination storage")
                for name, r in records.items():
                    destination = temporary / relative_name(name)
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    digest = hashlib.sha256()
                    with self.path(r["id"]).open("rb") as source, destination.open("xb") as output:
                        while block := source.read(1024 * 1024):
                            digest.update(block)
                            output.write(block)
                        output.flush()
                        os.fsync(output.fileno())
                    if digest.hexdigest() != r["digest"]:
                        raise ValueError("Staged content changed during import")
                atomic_text(temporary / ".zaman-manifest.json", manifest_text)
                for d in sorted([p for p in temporary.rglob("*") if p.is_dir()], key=lambda p:len(p.parts), reverse=True):
                    fsync_dir(d)
                fsync_dir(temporary)
                # The receiver is single-instance, content-addressed and never
                # replaces a populated directory. Copy occurs on destination FS.
                temporary.rename(target)
                fsync_dir(directory)
            except Exception:
                if temporary.exists():
                    shutil.rmtree(temporary)
                raise
        # This commit is recoverable: manifest and target are durable before the
        # owned metadata is rebuilt. A crash retries this exact directory.
        self.update(ident, phase="complete", error="", refresh=1)
        self.write_metadata()
        # Only finalized imports release staging space. Unresolved/cancelled
        # work remains until an explicit user cleanup is implemented.
        still_needed = set()
        for pending in self.rows():
            if pending["phase"] not in {"complete", "cancelled", "attached"} and pending["manifest"]:
                still_needed.update(json.loads(pending["manifest"]).values())
        for r in records.values():
            if r["id"] not in still_needed:
                self.path(r["id"]).unlink(missing_ok=True)

    def write_metadata(self):
        with self.lock:
            self._write_metadata()

    def _write_metadata(self):
        groups = {}
        for row in self.rows():
            if row["system"] and row["phase"] in {"ready","importing"}:
                groups.setdefault(row["system"], {})
            if row["phase"] == "complete" and not row["external"]:
                groups.setdefault(row["system"], {})[(row["target"], row["name"])] = row
        for sid, games in groups.items():
            lines = ["# Owned by Zaman Transfer; other metadata is never edited.",
                     "collection: " + self.registry.collections[sid], "shortname: " + self.registry.shortnames[sid],
                     "ignore-regex: (^|/)[.]zaman-import-[^/]+/",
                     f'launch: /usr/local/bin/zamanctl launch {sid} "{{file.path}}"']
            for row in games.values():
                for name in json.loads(row["manifest"]):
                    if name != row["name"]:
                        lines.append("ignore-file: " + str(Path(row["target"]) / name))
            for row in games.values():
                fields = json.loads(row["scraped"])
                title = fields.pop("game", PurePosixPath(row["name"]).stem)
                lines.extend(["", "game: " + str(title).replace("\n", " "),
                              "file: " + str(Path(row["target"]) / row["name"])])
                for key, value in fields.items():
                    if key not in {"developer","publisher","genre","release","players","rating","description"} and not key.startswith("assets."):
                        continue
                    parts = str(value).splitlines()
                    if parts:
                        lines.append(key + ": " + parts[0])
                        lines.extend("  " + part for part in parts[1:])
            atomic_text(self.metadata / ("zaman-transfer-" + sid + ".metadata.pegasus.txt"), "\n".join(lines) + "\n")

    def snapshot(self):
        jobs = []
        history = 0
        for r in reversed(self.rows()):
            if r["phase"] == "attached":
                continue
            if r["phase"] in TERMINAL:
                if r["hidden"]:
                    continue
                history += 1
                if history > 100:
                    continue
            jobs.append({k:r[k] for k in ("id","batch","name","source","size","received","phase","system","evidence","error","created","scrape_job","artwork_warning")})
            jobs[-1]["candidates"] = json.loads(r["candidates"])
        return jobs
