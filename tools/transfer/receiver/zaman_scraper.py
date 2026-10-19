"""Supervised Skyscraper 3.21 jobs, GUI questions and owned metadata enrichment."""
from __future__ import annotations

import codecs
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import subprocess
import termios
import threading
import time
import uuid

from zaman_inbox import ID, atomic_text, fsync_dir, relative_name

PLATFORM_MAP = {"colecovision":"coleco", "gameboy":"gb", "gameboy-color":"gbc",
                "gameboy-advance":"gba", "gamecube":"gc", "ps1":"psx", "sg1000":"sg-1000"}
ACTIVE = {"queued", "running", "needs-input", "waiting-library", "publishing"}
FINAL = {"complete", "failed", "cancelled", "interrupted"}
FIELDS = {"game","developer","publisher","genre","release","players","rating","description"}
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
AUTO_BUDGET_SECONDS = 120


def artwork_issue(error):
    if re.search(r"could not resolve|host not found|network (?:error|unreachable)|no (?:internet|network)|connection (?:refused|failed|timed out)|unable to connect",str(error),re.I):
        return "Could not reach the artwork service. Check your connection and try Artwork & Game Info later."
    return "Artwork lookup could not finish. You can add artwork later from Artwork & Game Info."


def enriched(entry):
    return any(value and (key.startswith("assets.") or key in FIELDS-{"game"}) for key,value in entry.items())


def existing_entries(text, folder):
    """Normalize existing user fields before reconciling private export output."""
    result={}
    for entry in metadata(text)[1]:
        value=entry.get("file",entry.get("files",""))
        if not value or "\n" in value:raise ValueError("Existing metadata has an unsupported game path; current file was kept")
        path=Path(value) if Path(value).is_absolute() else folder/value
        path=path.resolve()
        if not path.is_relative_to(folder.resolve()):raise ValueError("Existing game path is outside its collection")
        entry["file"]=str(path);entry.pop("files",None)
        for key,value in list(entry.items()):
            if key.startswith("assets."):
                paths=[(Path(p) if Path(p).is_absolute() else folder/p).resolve() for p in value.splitlines()]
                entry[key]="\n".join(str(p) for p in paths if p.is_file())
        result[str(path)]=entry
    return result


def merge_fields(previous, fresh, replace=False):
    merged=dict(previous)
    for key,value in fresh.items():
        if key=="file" or not merged.get(key) or replace and (key in FIELDS or key.startswith("assets.")):
            merged[key]=value
    return merged


def plain(text):
    return "".join(c for c in ANSI.sub("", text).replace("\r", "\n") if c in "\n\t" or ord(c) >= 32)


def question(text):
    """The reachable gather/export questions audited in tag 3.21.0."""
    clean = plain(text)
    if re.search(r"\(y/N\)\?\s*$", clean):
        line=clean.strip().splitlines()[-1][-2000:]
        return dict(question=line,kind="overwrite" if "overwrite" in line else "skip-existing" if "skip already existing" in line else "choice",choices=[dict(label="No",value="n"),dict(label="Yes",value="y")])
    if "Please choose the preferred entry" in clean and clean.rstrip().endswith(":"):
        block = clean[clean.rfind("Potential entries for "):] if "Potential entries for " in clean else clean
        choices = [dict(label="Use Skyscraper's suggested match",value="")]
        matches=list(re.finditer(r"(?m)^\s*(\d+)\s*: Title:\s*'([^\n]*)'( <-- Skyscraper's choice)?\s*$", block))
        for index,match in enumerate(matches):
            tail=block[match.end():matches[index+1].start() if index+1<len(matches) else len(block)]
            detail=" · ".join(line.strip() for line in tail.splitlines() if line.strip().startswith(("Platform:","Release date:")))
            choices.append(dict(label=match[2] + (match[3] or ""),detail=detail,value=match[1]))
        choices.append(dict(label="None of these matches",value="-1"))
        return dict(question=block.splitlines()[0]+"\nChoose a match, use the suggestion, or select none.",kind="match",choices=choices)
    if re.search(r"\?\s*$", clean):
        return dict(question=clean.strip()[-5000:],choices=[],text=True,
                    masked=bool(re.search(r"password|secret",clean,re.I)))
    return None


def metadata(text):
    header, entries = {}, []
    current, previous = header, None
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if line[0].isspace():
            if previous is None:
                raise ValueError("Unexpected metadata continuation")
            current[previous] += "\n" + line.strip()
            continue
        key, sep, value = line.partition(":")
        if not sep or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_.-]*",key):
            raise ValueError("Unsupported metadata line")
        if key == "game":
            current = {}; entries.append(current)
        current[key] = current[key] + "\n" + value.strip() if key in current else value.strip()
        previous = key
    return header, entries


def render(header, entries):
    lines = ["# Generated by Zaman Scrape Games; ROM files are never modified."]
    for values in [header] + entries:
        if values is not header:
            lines.append("")
        for key, value in values.items():
            parts = str(value).splitlines() or [""]
            lines.append(key + ": " + parts[0])
            lines.extend("  " + part for part in parts[1:])
    return "\n".join(lines) + "\n"


def fingerprint(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None


def probe_program(binary="Skyscraper"):
    found = shutil.which(binary)
    if not found:
        raise RuntimeError("Skyscraper was not found; set SKYSCRAPER_BIN to its absolute path")
    result = subprocess.run([found,"--help"],check=True,capture_output=True,text=True,timeout=25)
    text = result.stdout + result.stderr
    if not re.search(r"Skyscraper v3\.21\.0(?:\s|$)",text):
        raise RuntimeError("This prompt adapter requires the audited Skyscraper 3.21.0")
    block = re.search(r"-p <PLATFORM>(.*?)--query",text,re.S)
    platforms = sorted(set(re.findall(r"'([a-z0-9][a-z0-9-]*)'",block[1] if block else "")))
    if not {"nes","ps2"} <= set(platforms):
        raise RuntimeError("Cannot read the installed platform list")
    return dict(binary=os.path.abspath(found),version="3.21.0",platforms=platforms)


class Scraper:
    def __init__(self, inbox, safe, notify=lambda row: True, program=None):
        self.inbox, self.safe, self.notify = inbox, safe, notify
        self.root = inbox.state / "scraping"
        self.root.mkdir(mode=0o700,exist_ok=True)
        self.lock = threading.RLock()
        self.jobs, self.replies = {}, {}
        self.stopping = threading.Event()
        self.thread = None
        if program is None:
            config = Path(__file__).with_name("scraper-program.json")
            program = json.loads(config.read_text()) if config.is_file() else {}
        self.binary = str(program.get("binary", ""))
        self.platforms = set(program.get("platforms", []))
        self.version = program.get("version", "")
        self.available = bool(self.binary and Path(self.binary).is_file() and self.platforms and self.version == "3.21.0")
        self.error = "" if self.available else "Artwork lookup is unavailable. Your games can still be added. Try again later."
        for path in sorted(self.root.glob("*/job.json")):
            try:
                job = json.loads(path.read_text())
                if not re.fullmatch(r"[a-f0-9]{32}",job["id"]) or path.parent.name != job["id"]:
                    continue
                if job["phase"] in ACTIVE:
                    job.update(phase="interrupted",stage="Artwork update interrupted; try again",prompt=None)
                self.jobs[job["id"]] = job
                for view in (path.parent/"work").glob("*/input"):
                    shutil.rmtree(view,ignore_errors=True)
            except (OSError, ValueError, KeyError):
                continue
        for row in self.inbox.rows():
            if row["phase"] == "scraping":
                job=self.jobs.get(row["scrape_job"],{})
                if job.get("automatic"):
                    self.inbox.update(row["id"],phase="ready",scrape_choice="skip",error="",artwork_warning="Artwork lookup was interrupted. You can add artwork later from Artwork & Game Info.")
                else:self.inbox.update(row["id"],phase="needs-scrape",error="The artwork update was interrupted")
        self.folders = []
        self.scan()

    def scan(self):
        found = []
        for path in sorted(self.inbox.registry.rom_root.iterdir()):
            if not path.is_dir() or path.name.startswith("."):
                continue
            platform = PLATFORM_MAP.get(path.name,path.name)
            reason = ("Folder links are not scraped" if path.is_symlink() else
                      "Mixed-system folder; choose a system-specific folder" if path.name == "others" else
                      "This collection is not supported by the artwork service" if not ID.fullmatch(path.name) or platform not in self.platforms else "")
            found.append(dict(id=path.name,name=self.inbox.registry.systems.get(path.name,{}).get("name",path.name),
                              platform=platform,supported=not reason,reason=reason))
        with self.lock:
            self.folders = found

    def save(self, job, **fields):
        with self.lock:
            if job.get("phase") == "cancelled" and fields.get("phase") in ACTIVE | {"complete","failed"}:
                raise InterruptedError("Scraping cancelled")
            job.update(fields)
            path = self.root / job["id"] / "job.json"
            atomic_text(path,json.dumps(job,ensure_ascii=False))

    def snapshot(self):
        with self.lock:
            rows = [j for j in sorted(self.jobs.values(),key=lambda j:j["created"],reverse=True) if not j.get("hidden")]
            history_count = sum(j["phase"] in FINAL for j in rows)
            rows = [dict(j,log=j.get("log","")[-4000:]) for j in rows[:100]]
            return json.loads(json.dumps(dict(available=self.available,error=self.error,version=self.version,
                folders=self.folders,jobs=rows,historyCount=history_count)))

    def start(self, folder, mode="cached", review=False, ident=None, retry_folders=None, automatic=False):
        if not self.available:
            raise ValueError(self.error)
        if mode not in {"cached","refresh","offline"} or type(review) is not bool:
            raise ValueError("Invalid scraping mode")
        with self.inbox.lock, self.lock:
            if any(j["phase"] in ACTIVE and (not automatic or not j.get("automatic")) for j in self.jobs.values()):
                raise ValueError("Another artwork update is active")
            if ident:
                row = self.inbox.get(ident)
                if row["phase"] != "needs-scrape":
                    raise ValueError("Import is not waiting for a scraping decision")
                folders = [row["system"]]
                if PLATFORM_MAP.get(row["system"],row["system"]) not in self.platforms:
                    raise ValueError("This collection is unsupported by Skyscraper")
            else:
                supported = [f["id"] for f in self.folders if f["supported"]]
                if retry_folders is not None:
                    if not retry_folders or not set(retry_folders)<=set(supported):raise ValueError("A retry folder is no longer available")
                    folders=list(retry_folders)
                else:folders = supported if folder == "all" else [folder] if folder in supported else []
                if not folders:
                    raise ValueError("Select a supported ROM folder")
            job = dict(id=uuid.uuid4().hex,created=time.time(),folders=folders,mode=mode,review=review,
                       imports=[ident] if ident else [],phase="queued",stage="Waiting for the library",folder="",
                       progress=0,prompt=None,log="",results=[],hidden=False,refresh=False,notified="",automatic=automatic,elapsed=0)
            self.jobs[job["id"]] = job
            self.save(job)
            if ident:
                self.inbox.update(ident,phase="scraping",scrape_choice="scrape",scrape_job=job["id"],error="",artwork_warning="")
            return job["id"]

    def auto_import(self, ident):
        try:return self.start(None,ident=ident,automatic=True)
        except (ValueError,OSError) as error:
            with self.inbox.lock:
                if self.inbox.get(ident)["phase"]=="needs-scrape":
                    self.inbox.update(ident,phase="ready",scrape_choice="skip",error="",artwork_warning=artwork_issue(error))

    def answer(self, ident, token, answer):
        with self.lock:
            job = self.jobs.get(ident)
            prompt = job.get("prompt") if job else None
            if not prompt or prompt["id"] != token or job["phase"] not in {"needs-input","waiting-library"}:
                raise ValueError("This scraping question is no longer current")
            if not isinstance(answer,str) or len(answer) > 256 or any(ord(c)<32 or ord(c)==127 for c in answer):
                raise ValueError("Invalid scraping answer")
            if prompt.get("choices") and answer not in {choice["value"] for choice in prompt["choices"]}:
                raise ValueError("Choose one of the offered answers")
            self.replies[ident] = (answer + "\n").encode()
            self.save(job,prompt=None,phase="running",stage="Continuing Skyscraper")

    def cancel(self, ident):
        with self.lock:
            job = self.jobs.get(ident)
            if not job or job["phase"] not in ACTIVE - {"publishing"}:
                raise ValueError("This scraping job is not active")
            self.save(job,phase="cancelled",prompt=None,stage="Cancelled; previously published folders remain intact")
        self.release_import(job,"Scraping cancelled; choose another action")

    def release_import(self, job, error):
        for ident in job["imports"]:
            with self.inbox.lock:
                row = self.inbox.get(ident)
                if row["phase"] == "scraping" and row["scrape_job"] == job["id"]:
                    if job.get("automatic"):
                        self.inbox.update(ident,phase="ready",scrape_choice="skip",error="",artwork_warning=artwork_issue(error))
                    else:self.inbox.update(ident,phase="needs-scrape",error=error)

    def dismiss(self, ident=None):
        with self.lock:
            jobs = [self.jobs[ident]] if ident in self.jobs else list(self.jobs.values()) if ident is None else []
            if not jobs or (ident and jobs[0]["phase"] not in FINAL):
                raise ValueError("Choose a finished scraping record")
            for job in jobs:
                if job["phase"] in FINAL:
                    self.save(job,hidden=True)

    def retry(self, ident):
        with self.lock:
            old = self.jobs.get(ident)
            if not old or old["phase"] not in {"failed","interrupted","cancelled"}:
                raise ValueError("Only an interrupted or failed scraping job can be retried")
            completed={r["folder"] for r in old["results"] if r["ok"]}
            remaining=[sid for sid in old["folders"] if sid not in completed]
            automatic_imports=list(old["imports"]) if old.get("automatic") else []
            if old.get("automatic"):
                args=(old["folders"][0],old["mode"],False,None,None)
            else:args=(old["folders"][0],old["mode"],old["review"],old["imports"][0] if old["imports"] else None,remaining)
        if any(self.inbox.get(item)["phase"]!="complete" for item in automatic_imports):
            raise ValueError("Your game is still being added; try the artwork update after import finishes")
        return self.start(*args)

    def begin(self):
        self.thread = threading.Thread(target=self.worker,daemon=True)
        self.thread.start()

    def close(self):
        self.stopping.set()
        if self.thread:
            self.thread.join(6)

    def wait_safe(self, job):
        while not self.safe():
            if self.stopping.wait(.25) or job["phase"] == "cancelled":
                raise InterruptedError("Scraping paused or cancelled")
            if job["phase"] != "waiting-library":
                self.save(job,phase="waiting-library",stage="Waiting for the library; gameplay continues")
        if self.stopping.is_set() or job["phase"] == "cancelled":
            raise InterruptedError("Scraping paused or cancelled")

    def run_command(self, job, argv, stage):
        self.wait_safe(job)
        master, slave = pty.openpty()
        attrs = termios.tcgetattr(slave); attrs[3] &= ~termios.ECHO; termios.tcsetattr(slave,termios.TCSANOW,attrs)
        process = None
        try:
            env = dict(os.environ,NO_COLOR="1",TERM="dumb")
            process = subprocess.Popen(argv,stdin=slave,stdout=slave,stderr=slave,env=env,
                cwd=argv[argv.index("-i")+1],start_new_session=True,close_fds=True)
            os.close(slave); slave = -1
            self.save(job,phase="running",stage=stage,progress=0,current="")
            decoder = codecs.getincrementaldecoder("utf-8")("replace")
            pending, output = "", ""
            last_io, next_safe, paused = time.monotonic(),0,False
            elapsed=job.get("elapsed",0);last_tick=time.monotonic()
            while True:
                now=time.monotonic()
                if not paused:elapsed+=now-last_tick
                last_tick=now
                if job.get("automatic") and elapsed>AUTO_BUDGET_SECONDS:
                    raise TimeoutError("Automatic artwork lookup timed out")
                if self.stopping.is_set() or job["phase"] == "cancelled":
                    raise InterruptedError("Scraping interrupted")
                if time.monotonic() >= next_safe:
                    safe = self.safe(); next_safe = time.monotonic()+2
                    if not safe and not paused:
                        os.killpg(process.pid,signal.SIGSTOP); paused=True
                        self.save(job,phase="waiting-library",stage="Paused during gameplay; return to the library to continue")
                    elif safe and paused:
                        os.killpg(process.pid,signal.SIGCONT); paused=False; last_io=time.monotonic()
                        self.save(job,phase="needs-input" if job["prompt"] else "running",stage=stage)
                with self.lock:
                    reply = self.replies.pop(job["id"],None)
                if reply is not None:
                    os.write(master,reply); last_io=time.monotonic()
                readable,_,_ = select.select([master],[],[],.1)
                data = b""
                if readable:
                    try:
                        data = os.read(master,65536)
                    except OSError:
                        pass
                if data:
                    last_io=time.monotonic()
                    text=plain(decoder.decode(data)); pending=(pending+text)[-32000:];output=(output+text)[-64000:]
                    self.save(job,log=(job["log"]+text)[-32000:])
                    progress=re.findall(r"#(\d+)\s*/\s*(\d+)",text) or re.findall(r"[\(\[](\d+)\s*/\s*(\d+)",text)
                    if progress:
                        a,b=progress[-1]; done,total=int(a),int(b)
                        self.save(job,progress=min(1,done/total) if total else 0)
                    current=re.findall(r"(?m)^#\d+/\d+[^\n]*",text)
                    if current:self.save(job,current=current[-1][:300])
                    prompt = question(pending) if not job["prompt"] else None
                    if prompt:
                        if prompt.get("kind") in {"overwrite","skip-existing"}:
                            # Output lives in a private work directory. Existing
                            # user fields are reconciled before publication.
                            os.write(master,b"y\n" if prompt["kind"]=="overwrite" else b"n\n")
                            pending="";continue
                        if job.get("automatic"):
                            raise RuntimeError("Artwork service requested an interactive answer")
                        prompt["id"]=uuid.uuid4().hex
                        self.save(job,phase="needs-input" if not paused else "waiting-library",stage="Answer Skyscraper's question",prompt=prompt)
                        pending=""
                        self.notice(job,"scrape-input")
                elif process.poll() is not None:
                    break
                if not paused and not job["prompt"] and time.monotonic()-last_io > 300:
                    raise TimeoutError("Skyscraper stopped producing output; inspect the job details and retry")
            if "User chose not to overwrite" in output:
                raise InterruptedError("Overwrite declined; current metadata was kept")
            if process.returncode:
                raise RuntimeError("Skyscraper exited with code " + str(process.returncode) + "; see job details")
            return output
        finally:
            if slave >= 0:
                os.close(slave)
            if process is not None and process.poll() is None:
                try:
                    os.killpg(process.pid,signal.SIGCONT);os.killpg(process.pid,signal.SIGTERM)
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid,signal.SIGKILL);process.wait()
                except ProcessLookupError:
                    pass
            os.close(master)
            if "elapsed" in locals():self.save(job,elapsed=elapsed)

    def notice(self, job, phase):
        if job.get("automatic"):return
        names=", ".join(self.inbox.registry.systems.get(sid,{}).get("name",sid) for sid in job["folders"])
        if self.notify(dict(id=job["id"],name=names,phase=phase)):
            self.save(job,notified=phase)

    def folder_work(self, job, sid):
        folder = self.inbox.registry.rom_root / sid
        if folder.is_symlink() or folder.resolve().parent != self.inbox.registry.rom_root:
            raise ValueError("Unsafe ROM folder")
        work = self.root / job["id"] / "work" / sid
        work.mkdir(parents=True,exist_ok=True)
        input_dir = folder
        if job["imports"]:
            row = self.inbox.get(job["imports"][0])
            if row["phase"] != "scraping" or row["scrape_job"] != job["id"]:
                raise InterruptedError("Import changed while scraping")
            input_dir = work / "input";input_dir.mkdir()
            for name, ident in json.loads(row["manifest"]).items():
                destination = input_dir / relative_name(name);destination.parent.mkdir(parents=True,exist_ok=True)
                source = self.inbox.path(ident)
                if source.is_symlink() or not source.is_file():
                    raise ValueError("Staged ROM is unavailable")
                os.link(source,destination)
        elif not folder.is_dir():
            raise ValueError("ROM folder was removed")
        meta = next((folder/name for name in ("metadata.pegasus.txt","metadata.txt") if (folder/name).is_file()),folder/"metadata.pegasus.txt")
        if meta.is_symlink():
            raise ValueError("Metadata is a symlink")
        before = fingerprint(meta)
        export_dir = work/"export";export_dir.mkdir()
        exported = export_dir/"metadata.pegasus.txt"
        if meta.is_file() and not job["imports"]:
            shutil.copy2(meta,exported)
        media = work/"media"
        platform = PLATFORM_MAP.get(sid,sid)
        common = [self.binary,"-p",platform,"-f","pegasus","-i",str(input_dir),"-g",str(export_dir),"-t","1"]
        gather_error=""
        if job["mode"] != "offline":
            argv = common + ["-s","screenscraper"]
            if job["mode"] == "refresh": argv += ["--refresh"]
            if job["review"]: argv += ["--flags","interactive"]
            try:self.run_command(job,argv,"Finding artwork and game info · " + sid)
            except (InterruptedError,TimeoutError):raise
            except Exception as error:
                if not job.get("automatic"):raise
                gather_error=str(error)+"\n"+job.get("log","")[-2000:]
        self.run_command(job,common+["-o",str(media),"--gamelistfilename",exported.name,
            "-e",'/usr/local/bin/zamanctl launch '+sid+' "{file.path}"',"--flags","skipped"],"Preparing game information · " + sid)
        if not exported.is_file():
            raise ValueError("Skyscraper did not produce metadata; no library changes were made")
        header, entries = metadata(exported.read_text())
        if not entries:
            raise ValueError("No game entries were produced; current metadata was kept")
        final_media = self.inbox.metadata.parent/"scrape-media"/job["id"]/sid
        for entry in entries:
            value=entry.get("file",entry.get("files",""))
            if not value or "\n" in value:
                raise ValueError("Unsupported exported game path")
            candidates=[Path(value)] if Path(value).is_absolute() else [export_dir/value,input_dir/value]
            valid={p.resolve() for p in candidates if not p.is_symlink() and p.resolve().is_relative_to(input_dir.resolve()) and p.is_file()}
            if len(valid) != 1:
                raise ValueError("Exported ROM path escapes its collection")
            resolved=valid.pop()
            entry["file"]=str(resolved);entry.pop("files",None)
        if not job["imports"] and meta.is_file():
            previous=existing_entries(meta.read_text(),folder)
            entries=[merge_fields(previous.pop(e["file"],{}),e,job["mode"]=="refresh") for e in entries]
            entries.extend(previous.values())
        for entry in entries:
            for key,value in list(entry.items()):
                if not key.startswith("assets."):continue
                mapped=[]
                for asset in value.splitlines():
                    choices=[Path(asset)] if Path(asset).is_absolute() else [export_dir/asset,input_dir/asset]
                    durable=(self.inbox.metadata.parent/"scrape-media").resolve()
                    valid={p.resolve() for p in choices if p.is_file() and (p.resolve().is_relative_to(media.resolve()) or p.resolve().is_relative_to(folder.resolve()) or p.resolve().is_relative_to(durable))}
                    if len(valid)!=1:raise ValueError("Exported artwork is missing or outside its collection")
                    path=valid.pop()
                    if path.is_relative_to(media.resolve()):
                        mapped.append(str(final_media/path.relative_to(media.resolve())))
                    elif path.is_relative_to(folder.resolve()):
                        mapped.append(str(path))
                    elif path.is_relative_to(durable):mapped.append(str(path))
                    else:
                        raise ValueError("Exported artwork path is outside its collection")
                entry[key]="\n".join(mapped)
        if job.get("automatic") and not any(enriched(entry) for entry in entries):
            raise ValueError(gather_error or "No artwork or game information was found")
        self.wait_safe(job)
        self.save(job,phase="publishing",stage="Publishing metadata · " + sid)
        if media.is_dir():
            final_media.parent.mkdir(parents=True,exist_ok=True)
            if final_media.exists():raise ValueError("Artwork destination already exists")
            shutil.copytree(media,final_media)
            for path in final_media.rglob("*"):
                if path.is_file():
                    with path.open("rb") as stream:os.fsync(stream.fileno())
            for path in sorted([p for p in final_media.rglob("*") if p.is_dir()],key=lambda p:len(p.parts),reverse=True):fsync_dir(path)
            fsync_dir(final_media);fsync_dir(final_media.parent)
        self.wait_safe(job)
        with self.inbox.lock:
            if job["phase"] == "cancelled" or self.stopping.is_set():
                raise InterruptedError("Scraping cancelled before publication")
            if job["imports"]:
                ident=job["imports"][0];row=self.inbox.get(ident)
                if row["phase"] != "scraping" or row["scrape_job"] != job["id"]:
                    raise InterruptedError("Import was cancelled")
                entry=next((e for e in entries if Path(e["file"]) == (input_dir/row["name"]).resolve()),None)
                if not entry:raise ValueError("The imported game was not present in the scraper export")
                details={k:v for k,v in entry.items() if k in FIELDS or k.startswith("assets.")}
                if job.get("automatic") and not enriched(details):raise ValueError(gather_error or "No artwork or game information was found for this game")
                self.inbox.update(ident,scraped=json.dumps(details),scrape_choice="done",phase="ready",error="",artwork_warning="")
            else:
                if fingerprint(meta) != before:
                    raise ValueError("Collection metadata changed during scraping; current file was kept")
                # Transfer retains ownership of its entries and companions. A
                # manual export enriches those rows instead of duplicating them.
                owned, members = {}, set()
                for row in self.inbox.rows():
                    if row["phase"] == "complete" and row["system"] == sid and not row["external"]:
                        owned[str((Path(row["target"])/row["name"]).resolve())]=row
                        members.update(str((Path(row["target"])/n).resolve()) for n in json.loads(row["manifest"]))
                normal=[]
                for entry in entries:
                    if entry["file"] in owned:
                        details={k:v for k,v in entry.items() if k in FIELDS or k.startswith("assets.")}
                        old=json.loads(owned[entry["file"]]["scraped"])
                        for key,value in list(old.items()):
                            if key.startswith("assets."):old[key]="\n".join(p for p in value.splitlines() if Path(p).is_file())
                        details=merge_fields(old,details,job["mode"]=="refresh")
                        fields=dict(scraped=json.dumps(details))
                        if enriched(details):fields["artwork_warning"]=""
                        self.inbox.update(owned[entry["file"]]["id"],**fields)
                    if entry["file"] not in members:normal.append(entry)
                if meta.is_file():shutil.copy2(meta,work/"metadata.before")
                # Preserve established collection identity and custom directives.
                if meta.is_file():header=metadata(meta.read_text())[0]
                header.setdefault("collection",self.inbox.registry.collections.get(sid,sid))
                header.setdefault("shortname",self.inbox.registry.shortnames.get(sid,sid))
                if "command" not in header:header.setdefault("launch",'/usr/local/bin/zamanctl launch '+sid+' "{file.path}"')
                atomic_text(meta,render(header,normal))
                self.inbox.write_metadata()
                self.save(job,refresh=True)
        missing=sum(not enriched(e) for e in entries)
        self.save(job,results=job["results"]+[dict(folder=sid,ok=True,games=len(entries),missing=missing)],progress=1)

    def worker(self):
        while not self.stopping.wait(.2):
            with self.lock:
                job=next((j for j in self.jobs.values() if j["phase"] == "queued"),None)
            if not job:continue
            try:
                for sid in job["folders"]:
                    self.wait_safe(job);self.save(job,folder=sid,progress=0)
                    try:self.folder_work(job,sid)
                    except InterruptedError:raise
                    except Exception as error:
                        self.save(job,results=job["results"]+[dict(folder=sid,ok=False,error=str(error)[:1000])])
                failed=any(not result["ok"] for result in job["results"])
                missing=sum(result.get("missing",0) for result in job["results"])
                self.save(job,phase="failed" if failed else "complete",prompt=None,
                          stage="Some collections need attention; open details" if failed else "Some games could not be matched; open details" if missing else "Artwork & info updated")
                if failed:self.release_import(job,"\n".join(r.get("error","") for r in job["results"]))
                self.notice(job,"scrape-failed" if failed or missing else "scrape-complete")
            except InterruptedError as error:
                self.release_import(job,str(error))
                self.save(job,phase="interrupted" if self.stopping.is_set() else "cancelled",stage=str(error),prompt=None)
            finally:
                # Keep exported metadata/logs and published media; private input
                # hardlinks are no longer needed once the subprocess has ended.
                for path in (self.root/job["id"]/"work").glob("*/input"):
                    shutil.rmtree(path,ignore_errors=True)

    def needs_refresh(self):
        with self.lock:return any(job.get("refresh") for job in self.jobs.values())

    def refreshed(self):
        with self.lock:
            for job in self.jobs.values():
                if job.get("refresh"):self.save(job,refresh=False)
