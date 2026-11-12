"""Controller ROM inventory and recoverable deletion for the Transfer receiver."""
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import uuid

from zaman_inbox import atomic_text, descriptor_refs, fsync_dir, relative_name

DESCRIPTORS = {".cue", ".gdi", ".m3u", ".m3u8", ".ccd", ".mds", ".toc"}
AUXILIARY = {".sav", ".srm", ".sram", ".mcr", ".mcd", ".state", ".rtc", ".sbi",
             ".eep", ".fla", ".nvram", ".ps2", ".savestate"}
SKIP_DIRS = {"assets", "bios", "media", "saves", "savestates", "screenshots", "lost+found"}
REMOVED = "Removed through Manage Games"
PAGE_SIZE = 35


def extension(path):
    return path.suffix.lower()


def auxiliary(path):
    suffix = extension(path)
    return suffix in AUXILIARY or bool(re.fullmatch(r"\.state[0-9]+", suffix))


def stamp(path):
    if path.is_symlink() or not path.is_file() or path.resolve() != path:
        raise ValueError("A game file is missing, moved or linked. Refresh the list.")
    info = path.stat()
    return [info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns]


def local_path(base, name):
    return base / relative_name(name)


def descendants(start, refs):
    seen, pending = set(), [start]
    while pending:
        path = pending.pop()
        if path not in seen:
            seen.add(path)
            pending.extend(refs.get(path, ()))
    return seen


def read_refs(path, folder):
    suffix = extension(path)
    if suffix in {".ccd", ".mds"}:
        types = (".img", ".sub") if suffix == ".ccd" else (".mdf",)
        siblings = {p for p in path.parent.iterdir()
                    if p.stem.casefold() == path.stem.casefold() and extension(p) in types}
        if not any(extension(p) == types[0] for p in siblings):
            raise ValueError("Missing disc image")
        return siblings
    name = path.relative_to(folder).as_posix()
    if suffix in {".m3u8", ".toc"}:
        if path.stat().st_size > 1024 * 1024:
            raise ValueError("Playlist is too large")
        lines = path.read_text(encoding="utf-8-sig").splitlines()
        if suffix == ".toc":
            names = [shlex.split(line)[1] for line in lines
                     if re.match(r"^\s*(FILE|AUDIOFILE|DATAFILE)\s+\S", line, re.I)]
        else:
            names = [line.strip() for line in lines if line.strip() and not line.lstrip().startswith("#")]
        if not names:
            raise ValueError("Disc descriptor has no companion files")
        return {local_path(path.parent, name) for name in names}
    return {folder / member for member in descriptor_refs(path, name)}


def metadata_paths(folder):
    return [path for path in folder.iterdir() if path.is_file() and not path.is_symlink()
            and (path.name in {"metadata.txt", "metadata.pegasus.txt"}
                 or path.name.endswith((".metadata.pegasus.txt", ".metadata.txt")))]


def metadata_blocks(path):
    with path.open(encoding="utf-8", newline="") as stream:
        return re.split(r"(?im)(?=^game:[ \t]*)", stream.read())


def game_files(block, base):
    fields = re.findall(r"(?im)^files?:[^\S\r\n]*([^\r\n]*)([\r\n]*(?:^[ \t]+[^\r\n]+[\r\n]*)*)",
                        block)
    return {(base / value.strip()).resolve() for first, rest in fields
            for value in [first, *rest.splitlines()] if value.strip() and "://" not in value}


class RomLibrary:
    def __init__(self, inbox):
        self.inbox = inbox
        self.journal = inbox.state / "rom-deletion.json"

    def folders(self):
        registry = self.inbox.registry
        return getattr(registry, "folder_targets", registry.paths)

    def collections(self):
        registry = self.inbox.registry
        return [{"id": name, "label": registry.systems.get(name, {}).get("name", name)}
                for name, folder in sorted(self.folders().items())
                if not folder.is_symlink() and folder.is_dir()
                and folder.name.casefold() not in SKIP_DIRS]

    def folder(self, name):
        folder = self.folders().get(name)
        if folder is None or folder.resolve() != folder or not folder.is_dir():
            raise ValueError("This collection is no longer available.")
        return folder

    def catalog(self, collection):
        folder = self.folder(collection)
        registry = self.inbox.registry
        system = registry.systems.get(collection)
        extensions = {"." + ext.lower() for item in ([system] if system else registry.systems.values())
                      for ext in item["extensions"]} - AUXILIARY
        refs, protected, bundles, candidates, titles = {}, set(), {}, set(), {}
        metadata_files = metadata_paths(folder)
        imported = self.inbox.metadata / ("zaman-transfer-" + collection + ".metadata.pegasus.txt")
        if imported.is_file() and not imported.is_symlink():
            metadata_files.append(imported)
        for metadata in metadata_files:
            for block in metadata_blocks(metadata)[1:]:
                title = block.splitlines()[0].partition(":")[2].strip()
                for path in game_files(block, metadata.parent):
                    if path.is_relative_to(folder):
                        titles[path] = title
        for root, directories, files in os.walk(folder, followlinks=False):
            directory = Path(root)
            directories[:] = [name for name in directories
                              if not name.startswith(".") and name.casefold() not in SKIP_DIRS
                              and not (directory / name).is_symlink()]
            if directory.parent == folder and re.fullmatch(r"import-[a-f0-9]{64}", directory.name):
                directories.clear()
                manifest = directory / ".zaman-manifest.json"
                if manifest.is_file() and not manifest.is_symlink():
                    try:
                        data = json.loads(manifest.read_text())
                        entry = local_path(directory, data["entry"])
                        members = {local_path(directory, name) for name in data["files"]}
                    except (KeyError, TypeError, ValueError, OSError):
                        continue
                    if (data.get("version") == 1 and entry in members and entry.is_file()
                            and not entry.is_symlink() and not auxiliary(entry)
                            and (extension(entry) in extensions | DESCRIPTORS or entry in titles)):
                        bundles[entry] = (members, manifest)
                        candidates.add(entry)
                continue
            for name in files:
                path = directory / name
                if name.startswith(".") or path.is_symlink() or not path.is_file():
                    continue
                suffix = extension(path)
                if suffix in extensions or suffix in {".m3u", ".m3u8"} or (path in titles and suffix in DESCRIPTORS):
                    candidates.add(path)
                if suffix in DESCRIPTORS:
                    try:
                        refs[path] = read_refs(path, folder)
                    except (ValueError, OSError, IndexError):
                        protected.add(directory)
        referenced = set().union(*refs.values())
        candidates.difference_update(referenced)
        return folder, sorted(candidates, key=lambda p: (titles.get(p, p.stem).casefold(), str(p))), titles, refs, protected, bundles

    def list_games(self, collection, page):
        if type(page) is not int or page < 0:
            raise ValueError("Invalid page")
        folder, paths, titles, _, _, _ = self.catalog(collection)
        page = min(page, max(0, (len(paths) - 1) // PAGE_SIZE))
        return {"type": "rom-games", "page": page, "count": len(paths),
                "more": (page + 1) * PAGE_SIZE < len(paths),
                "games": [{"id": str(path.relative_to(folder)), "label": titles.get(path, path.stem),
                           "detail": str(path.relative_to(folder)) if path.parent == folder
                           else path.name}
                          for path in paths[page * PAGE_SIZE:(page + 1) * PAGE_SIZE]]}

    def plan(self, collection, ident):
        folder, candidates, titles, refs, protected, bundles = self.catalog(collection)
        if not isinstance(ident, str):
            raise ValueError("Choose a game.")
        entry = folder / ident
        if entry not in candidates or str(entry.relative_to(folder)) != ident:
            raise ValueError("This game changed or is referenced by another game. Refresh the list.")
        manifest = None
        if entry in bundles:
            members, manifest = bundles[entry]
        else:
            members = descendants(entry, refs)
        shared = set()
        for parent in refs.keys() - members:
            for member in refs[parent] & members:
                shared.update(descendants(member, refs))
        members = {path for path in members - shared if not auxiliary(path)}
        if any(p.is_relative_to(directory) for p in members for directory in protected):
            raise ValueError("A disc descriptor in this folder could not be read. Its files were kept.")
        if entry not in members:
            raise ValueError("This game is referenced by another game.")
        # The manifest is bookkeeping. Only stage it when every member is removed.
        paths = sorted(path for path in members if path.is_file() and not path.is_symlink())
        if entry not in paths:
            raise ValueError("The game file is no longer available.")
        count = len(paths)
        identity = [stamp(path) for path in paths]
        if manifest and not shared:
            paths.append(manifest)
            identity.append(stamp(manifest))
        token = hashlib.sha256(json.dumps([list(map(str, paths)), identity]).encode()).hexdigest()
        return folder, entry, paths, identity, {"type": "rom-confirm", "token": token,
                "title": titles.get(entry, entry.stem), "files": count,
                "bytes": sum(item[2] for item in identity[:count]),
                "shared": len(shared)}

    def remove(self, collection, ident, token, allowed, signal):
        if self.journal.exists():
            self.recover(signal)
        folder, _, paths, identity, confirmation = self.plan(collection, ident)
        if token != confirmation["token"]:
            raise ValueError("The game changed. Select it again to review the updated files.")
        if not allowed():
            raise ValueError("Open Manage Games from the library with games and storefronts closed.")
        staging = folder / (".zaman-deleting-" + uuid.uuid4().hex)
        filesystem = folder.stat()
        record = dict(folder=str(folder), device=filesystem.st_dev, inode=filesystem.st_ino,
                      staging=str(staging), paths=list(map(str, paths)), committed=False)
        atomic_text(self.journal, json.dumps(record))
        try:
            staging.mkdir(mode=0o700)
            fsync_dir(folder)
            for index, path in enumerate(paths):
                if stamp(path) != identity[index]:
                    raise ValueError("A game file changed during removal.")
                if path.stat().st_dev != staging.stat().st_dev:
                    raise ValueError("The game spans different filesystems.")
                path.rename(staging / str(index))
                fsync_dir(path.parent)
                fsync_dir(staging)
            record["committed"] = True
            atomic_text(self.journal, json.dumps(record))
        except (OSError, ValueError):
            self.recover(signal)
            raise
        self.recover(signal)
        return {"type": "rom-deleted", "title": confirmation["title"]}

    def recover(self, signal):
        if not self.journal.is_file():
            return
        record = json.loads(self.journal.read_text())
        folder = Path(record["folder"])
        stage = Path(record["staging"])
        paths = [Path(path) for path in record["paths"]]
        root = self.inbox.registry.rom_root
        if (folder.parent != root or folder.resolve() != folder or stage.parent != folder
                or not re.fullmatch(r"\.zaman-deleting-[a-f0-9]{32}", stage.name)
                or stage.is_symlink() or any(not path.is_relative_to(folder)
                                              or path.resolve() != path for path in paths)):
            raise ValueError("Unsafe ROM deletion journal.")
        filesystem = folder.stat()
        if (filesystem.st_dev, filesystem.st_ino) != (record["device"], record["inode"]):
            raise ValueError("The original game filesystem is unavailable for deletion recovery.")
        if record["committed"]:
            removed = set(paths)
            # Keep authored metadata and any alternative launch file in a multi-file entry.
            for metadata in metadata_paths(folder):
                blocks = metadata_blocks(metadata)
                kept = [blocks[0]]
                for block in blocks[1:]:
                    referenced = game_files(block, metadata.parent)
                    if not referenced or not referenced <= removed:
                        kept.append(block)
                if len(kept) != len(blocks):
                    atomic_text(metadata, "".join(kept))
            with self.inbox.db:
                for row in self.inbox.rows():
                    if (row["phase"] == "complete" and row["target"]
                            and Path(row["target"]) / row["name"] in removed):
                        self.inbox.db.execute(
                            "UPDATE items SET phase='cancelled', hidden=1, notified='cancelled', "
                            "refresh=1, evidence=? WHERE id=?", (REMOVED, row["id"]))
            self.inbox.write_metadata()
            signal()
            for index in range(len(paths)):
                (stage / str(index)).unlink(missing_ok=True)
            for path in sorted({p.parent for p in paths}, key=lambda p: -len(p.parts)):
                while path != folder and re.fullmatch(r"import-[a-f0-9]{64}", path.relative_to(folder).parts[0]):
                    if not path.is_dir() or any(path.iterdir()):
                        break
                    path.rmdir()
                    fsync_dir(path.parent)
                    path = path.parent
        else:
            for index, path in reversed(list(enumerate(paths))):
                moved = stage / str(index)
                if moved.is_file():
                    if path.exists():
                        raise ValueError("A replacement file blocks recovery of a removed ROM.")
                    moved.rename(path)
                    fsync_dir(path.parent)
                    fsync_dir(stage)
        if stage.is_dir():
            stage.rmdir()
            fsync_dir(folder)
        self.journal.unlink()
        fsync_dir(self.journal.parent)
