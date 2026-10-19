#!/usr/bin/env python3
"""Same-user Transfer control client. JSON snapshots are for the local UI."""
import argparse
import json
import os
from pathlib import Path
import socket
import sys


def request(message):
    runtime = Path(os.environ.get("XDG_RUNTIME_DIR", "/run/user/" + str(os.getuid())))
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(4)
        s.connect(str(runtime / "zaman-transfer/control.sock"))
        s.sendall((json.dumps(message) + "\n").encode())
        with s.makefile("rb") as f:
            line = f.readline(4 * 1024 * 1024 + 1)
        if len(line) > 4 * 1024 * 1024 or not line.endswith(b"\n"):
            raise ValueError("Invalid Transfer reply")
        return json.loads(line)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("action",choices=["snapshot","enable","disable","choose","cancel","cancel-session","retry","dismiss-history","clear-history","request"])
    p.add_argument("id",nargs="?")
    p.add_argument("system",nargs="?")
    args = p.parse_args()
    message = dict(action=args.action)
    if args.action == "request":
        raw=args.id if args.id else sys.stdin.readline(8193)
        if not raw or len(raw)>8192:raise ValueError("Invalid request")
        message=json.loads(raw)
        if not isinstance(message,dict):raise ValueError("Invalid request")
    if args.id and args.action != "request":
        message["batch" if args.action == "cancel-session" else "id"] = args.id
    if args.system:
        message["system"] = args.system
    try:
        result = request(message)
    except (OSError, ValueError) as e:
        result = dict(type="error",error="Transfer unavailable: " + str(e))
    print(json.dumps(result),flush=True)
    return 1 if result.get("type") == "error" else 0


if __name__ == "__main__":
    sys.exit(main())
