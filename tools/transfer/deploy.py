#!/usr/bin/env python3
"""Guarded source preparation and owner-run activation/rollback. Never uses git writes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
HOME = Path.home()
SERVICE = "zaman-transfer-receiver.service"


def digest(path):
    if path.is_symlink():
        raise RuntimeError("Refusing symlink target: " + str(path))
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args],check=True,**kwargs)


def output(args):
    return subprocess.check_output([str(x) for x in args],text=True,stderr=subprocess.DEVNULL,timeout=8).strip()


def save(record):
    path=ROOT/"activation.json"
    temp=ROOT/".activation.tmp"
    temp.write_text(json.dumps(record,indent=2)+"\n")
    os.replace(temp,path)


def write(path,data,mode=0o644):
    path.parent.mkdir(parents=True,exist_ok=True)
    temporary=path.with_name("."+path.name+".sprint7")
    with temporary.open("xb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.chmod(temporary,mode)
    os.replace(temporary,path)


def idle(record, rollback=False):
    def call(method):
        return json.loads(output(["busctl","--user","--json=short","--timeout=2","call",
            "com.kawnelectro.Zaman.Session1","/com/kawnelectro/Zaman/Session1",
            "com.kawnelectro.Zaman.Session1",method]))["data"]
    status,foreground,store,installed=(call(m) for m in ("Status","ForegroundStatus","StorefrontStatus","InstalledGameStatus"))
    if (status[0]!="Idle" or foreground[0] not in ({"library","menu"} if rollback else {"library"}) or store[0]
            or installed[3] in {"Starting","Running","Stopping","CleanupFailed"}):
        raise RuntimeError("Return to the library and close the system menu/storefront before deployment")
    if rollback:
        # A broken QML renderer may never answer timings. Stopping its existing
        # supervised service lets sessiond release the lease before restoration.
        run(["systemctl","--user","stop","zaman-quickshell.service"])
        for _ in range(40):
            if call("ForegroundStatus")[0]=="library":
                return
            time.sleep(.1)
        raise RuntimeError("Menu input cleanup did not finish; no files were restored")
    timings=output(["sh",Path(record["live"])/"run.sh",record["sessiond"],"timings"])
    state=json.loads(timings.splitlines()[-1])
    if state.get("status")!="hidden" or state.get("inputOwned") or not state.get("helperIdle"):
        raise RuntimeError("The resident menu still owns input; close it and retry")


def describe(args):
    sessiond=args.sessiond.resolve()
    menu=args.menu.resolve()
    pegasus=args.pegasus.resolve()
    live=args.live.resolve()
    config=Path(os.environ.get("XDG_CONFIG_HOME",str(HOME/".config")))
    theme=config/"pegasus-frontend/themes/zaman"
    source=json.loads((ROOT/"baselines.json").read_text())
    baseline={entry["key"]:entry for entry in source}
    peg_old=json.loads((ROOT/"extra-baselines.json").read_text())
    # Confirm the local publication objects instead of fetching or switching.
    for repo,sha in ((sessiond,"8c768c4"),(menu,"7ce4c3a"),(pegasus,"360f465")):
        actual=output(["git","-C",repo,"rev-parse","--short=7","sprint6-publication-20261018"])
        if actual!=sha:
            raise RuntimeError("Publication baseline changed: "+str(repo))
    spec=[]
    def add(target,data,allowed,group):
        current=digest(target)
        after=hashlib.sha256(data).hexdigest()
        if current not in allowed and current!=after:
            raise RuntimeError("File differs from the audited baseline: "+str(target))
        spec.append(dict(target=str(target),data=data,before=current,after=after,group=group))
    for name in ("shell.qml","NotificationsPage.qml"):
        entry=baseline["qml/"+name]
        data=(ROOT/"menu"/name).read_bytes()
        for directory,group in ((menu/"qml","source"),(live/"qml","live")):
            add(directory/name,data,[entry["before"]],group)
    for name in ("TransferPage.qml","ScrapePage.qml"):
        data=(ROOT/"menu"/name).read_bytes()
        add(menu/"qml"/name,data,[None],"source")
        add(live/"qml"/name,data,[None],"live")
    add(pegasus/"theme.qml",(ROOT/"pegasus/theme.qml").read_bytes(),
        [baseline["theme.qml"]["before"],peg_old["old_pegasus"]],"source")
    add(theme/"theme.qml",(ROOT/"pegasus/theme.qml").read_bytes(),[baseline["theme.qml"]["before"]],"live")
    add(pegasus/"tests/test-rom-refresh.cjs",(ROOT/"pegasus/tests/test-rom-refresh.cjs").read_bytes(),[None],"source")
    for path in (ROOT/"menu-tests").glob("*.cjs"):
        allowed=[peg_old.get("tests/"+path.name)]
        add(menu/"tests"/path.name,path.read_bytes(),allowed,"source")
    # Store only this new Transfer subtree in the existing sessiond checkout.
    # No old docs, probes, compiled targets or absent Sprint 6 files are touched.
    for directory in ("receiver","tests","ui-tests","menu","menu-tests","pegasus"):
        for path in (ROOT/directory).rglob("*"):
            if path.is_file() and "__pycache__" not in path.parts:
                add(sessiond/"tools/transfer"/path.relative_to(ROOT),path.read_bytes(),[None],"source")
    for name in ("deploy.py","baselines.json","extra-baselines.json","README.md"):
        path=ROOT/name
        add(sessiond/"tools/transfer"/name,path.read_bytes(),[None],"source")
    version="sprint7-"+str(time.time_ns())
    release=HOME/".local/libexec/zaman"/version
    state=HOME/".local/state/zaman-transfer-sprint7"
    for path in (ROOT/"receiver").glob("*.py"):
        add(release/path.name,path.read_bytes(),[None],"prepare")
    sys.path.insert(0,str(ROOT/"receiver"))
    from zaman_scraper import probe_program
    program=probe_program(os.environ.get("SKYSCRAPER_BIN","Skyscraper"))
    add(release/"scraper-program.json",(json.dumps(program,indent=2)+"\n").encode(),[None],"prepare")
    # Receiver uses the same explicitly configured registry roots as sessiond.
    pid=output(["systemctl","--user","show","zaman-sessiond.service","-p","MainPID","--value"])
    registry_roots="/usr/share/zaman:/etc/zaman"
    for field in Path("/proc/"+pid+"/environ").read_bytes().split(b"\0"):
        if field.startswith(b"ZAMAN_REGISTRY_DIRS="):
            registry_roots=os.fsdecode(field.split(b"=",1)[1])
    legacy=json.loads(Path("/etc/zaman-transfer/config.json").read_text())
    rom_root=Path(legacy["rom_root"])
    if not rom_root.is_dir() or not os.access(rom_root,os.W_OK):
        raise RuntimeError("ROM root is not writable: "+str(rom_root))
    sys.path.insert(0,str(ROOT/"receiver"))
    from zaman_inbox import Registry
    Registry(registry_roots.split(os.pathsep),rom_root)
    for command in ("openssl","busctl","systemctl","qs"):
        if not shutil.which(command):
            raise RuntimeError("Existing dependency is missing: "+command)
    if output(["systemctl","--user","show",SERVICE,"-p","LoadState","--value"]) != "not-found":
        raise RuntimeError("A receiver service already exists; inspect it before preparing another")
    for port_type in ("-ltn","-lun"):
        listeners=output(["ss",port_type])
        if any(line.split()[3].endswith(":53317") for line in listeners.splitlines()[1:] if len(line.split())>3):
            raise RuntimeError("Port 53317 is already in use; retain the current listener and inspect it")
    def quote(s):
        return '"'+str(s).replace('\\','\\\\').replace('"','\\"').replace('%','%%')+'"'
    argv=["/usr/bin/python3","-B",release/"zaman_receiver.py","--state",state,
          "--rom-root",rom_root,"--config-root",config,"--registry-roots",registry_roots]
    unit=("[Unit]\nDescription=Zaman background Transfer inbox\nAfter=zaman-sessiond.service\n\n[Service]\n"
          "Type=simple\nExecStart="+" ".join(map(quote,argv))+"\nRestart=on-failure\nRestartSec=3\n"
          "UMask=0077\nNice=10\nLimitFSIZE=infinity\nTimeoutStopSec=75\n\n[Install]\nWantedBy=default.target\n")
    add(config/"systemd/user"/SERVICE,unit.encode(),[None],"live")
    dropin="[Service]\nEnvironment="+quote("ZAMAN_TRANSFER_CLIENT="+str(release/"zaman_transfer_client.py"))+"\n"
    add(config/"systemd/user/zaman-quickshell.service.d/zzzz-sprint7-transfer.conf",dropin.encode(),[None],"live")
    return spec,dict(phase="preparing",sessiond=str(sessiond),menu=str(menu),pegasus=str(pegasus),
                     live=str(live),theme=str(theme),state=str(state),release=str(release),changes=[])


def prepare(args):
    if (ROOT/"activation.json").exists():
        record=json.loads((ROOT/"activation.json").read_text())
        print("Existing deployment phase:",record["phase"])
        if record["phase"]=="prepared":
            print("Source is prepared. Run the documented UI checks, then activate.")
            return
        raise RuntimeError("Use activate/status/rollback for this deployment; extract a fresh kit for another preparation")
    spec,record=describe(args)
    idle(record)
    for path in (ROOT/"receiver").glob("*.py"):
        compile(path.read_text(),str(path),"exec")
    backup=ROOT/"backups"
    backup.mkdir(exist_ok=True)
    for index,item in enumerate(spec):
        path=Path(item["target"])
        before=backup/str(index)
        after=backup/(str(index)+".after")
        if path.exists():
            shutil.copy2(path,before)
        after.write_bytes(item.pop("data"))
        item.update(backup=str(before),prepared=str(after),mode=(path.stat().st_mode&0o777) if path.exists() else 0o644)
        record["changes"].append(item)
    save(record)
    for item in record["changes"]:
        if item["group"] in {"source","prepare"}:
            write(Path(item["target"]),Path(item["prepared"]).read_bytes(),item["mode"])
    state=Path(record["state"])
    state.mkdir(mode=0o700,parents=True,exist_ok=True)
    cert,key=state/"receiver.crt",state/"receiver.key"
    if cert.exists()!=key.exists():
        raise RuntimeError("Only one receiver identity file exists; retain it and inspect before retrying")
    if not cert.exists():
        run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","3650",
             "-subj","/CN=Zaman LocalSend receiver","-keyout",key,"-out",cert],capture_output=True)
        os.chmod(key,0o600)
    record["phase"]="prepared"
    save(record)
    print("Source prepared; live UI and services are unchanged.")
    print("State retained at:",state)


def load():
    return json.loads((ROOT/"activation.json").read_text())


def activate():
    record=load()
    if record["phase"]!="prepared":
        raise RuntimeError("Activation requires the prepared phase")
    idle(record)
    for item in record["changes"]:
        expected=item["before"] if item["group"]=="live" else item["after"]
        if digest(Path(item["target"]))!=expected:
            raise RuntimeError("File changed since preparation: "+item["target"])
    record["phase"]="activating"
    save(record)
    for item in record["changes"]:
        if item["group"]=="live":
            write(Path(item["target"]),Path(item["prepared"]).read_bytes(),item["mode"])
    run(["systemctl","--user","daemon-reload"])
    run(["systemctl","--user","start",SERVICE])
    for _ in range(40):
        result=subprocess.run(["/usr/bin/python3","-B",Path(record["release"])/"zaman_transfer_client.py","snapshot"],capture_output=True,text=True)
        if result.returncode==0:
            break
        time.sleep(.1)
    else:
        raise RuntimeError("Receiver did not become ready; use rollback and inspect its journal")
    run(["systemctl","--user","restart","zaman-quickshell.service"])
    for _ in range(40):
        try:
            idle(record)
            break
        except (RuntimeError,ValueError,subprocess.SubprocessError):
            time.sleep(.1)
    else:
        raise RuntimeError("Menu did not recover its hidden input-idle state; run rollback")
    run(["systemctl","--user","is-active","zaman-quickshell.service",SERVICE])
    run(["systemctl","--user","enable",SERVICE])
    record["phase"]="active"
    save(record)
    print("Active. Receiving remains OFF until enabled from Transfer Games.")
    print("Pegasus loads the ROM refresh listener when its theme next reloads; see README.")


def rollback():
    record=load()
    for item in record["changes"]:
        actual=digest(Path(item["target"]))
        if actual not in {item["before"],item["after"]}:
            raise RuntimeError("Rollback would overwrite newer work: "+item["target"])
    if record["phase"] in {"active","activating"}:
        idle(record,rollback=True)
    subprocess.run(["systemctl","--user","disable","--now",SERVICE],check=False)
    for item in reversed(record["changes"]):
        path=Path(item["target"])
        if digest(path)==item["before"]:
            continue
        if item["before"] is None:
            path.unlink(missing_ok=True)
        else:
            write(path,Path(item["backup"]).read_bytes(),item["mode"])
    run(["systemctl","--user","daemon-reload"])
    if record["phase"] in {"active","activating"}:
        run(["systemctl","--user","restart","zaman-quickshell.service"])
    record["phase"]="rolled-back"
    save(record)
    print("Rolled back. Pending imports, SQLite, receiver identity and imported ROMs were retained.")


def main():
    p=argparse.ArgumentParser()
    p.add_argument("action",choices=["prepare","activate","rollback","status"])
    p.add_argument("--sessiond",type=Path,default=HOME/"projects/zaman-sprint4/zaman-sessiond")
    p.add_argument("--menu",type=Path,default=HOME/"projects/zaman-sprint5/zaman-menu")
    p.add_argument("--pegasus",type=Path,default=HOME/"zaman-sprint4-release-final/zaman-pegasus")
    p.add_argument("--live",type=Path,default=HOME/"zaman-quickshell-prototype")
    args=p.parse_args()
    if args.action=="prepare":prepare(args)
    elif args.action=="activate":activate()
    elif args.action=="rollback":rollback()
    else:
        record=load()
        print(json.dumps({k:record[k] for k in ("phase","state","release")},indent=2))


if __name__=="__main__":
    try:
        main()
    except Exception as error:
        print("Stopped:",str(error),file=sys.stderr)
        sys.exit(1)
