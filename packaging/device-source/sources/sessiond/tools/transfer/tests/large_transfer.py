#!/usr/bin/env python3
"""Opt-in >4 GiB TLS stream + validation + destination copy test; needs 10 GiB free.

Synthetic protocol evidence only. Stock phone and PC acceptance remains separate.
"""
import hashlib
import http.client
import json
from pathlib import Path
import resource
import ssl
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"receiver"))
from zaman_inbox import Registry, Inbox
from zaman_receiver import HttpServer, Receiver


def main():
    size = 4*1024**3+17
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="zaman-large-test-") as temp:
        root = Path(temp)
        rom = root/"roms"
        rom.mkdir()
        systems = root/"registry/systems.d"
        systems.mkdir(parents=True)
        (systems/"ps2.toml").write_text('schema=1\n[system]\nid="ps2"\nname="PS2"\nextensions=["iso"]\n')
        cert,key=root/"cert",root/"key"
        subprocess.run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1",
                        "-subj","/CN=test","-keyout",str(key),"-out",str(cert)],check=True,capture_output=True)
        inbox=Inbox(root/"state",Registry([root/"registry"],rom),root/"metafiles",0)
        receiver=Receiver(inbox)
        ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(cert,key)
        server=HttpServer(("127.0.0.1",0),receiver,ctx)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            pin=receiver.control(dict(action="enable"))["pin"]
            ready=receiver.prepare(dict(info=dict(alias="large synthetic"),files={"big":dict(fileName="large.iso",size=size)}),"127.0.0.1",pin)
            data=b"\0"*(1024*1024)
            expected=hashlib.sha256()
            def chunks():
                remaining=size
                while remaining:
                    block=data[:min(len(data),remaining)]
                    expected.update(block)
                    remaining-=len(block)
                    yield block
            connection=http.client.HTTPSConnection("127.0.0.1",server.server_port,
                context=ssl._create_unverified_context(),timeout=120)
            route="/api/localsend/v2/upload?sessionId="+ready["sessionId"]+"&fileId=big&token="+ready["files"]["big"]
            connection.request("POST",route,body=chunks(),headers={"Content-Length":str(size)})
            response=connection.getresponse()
            response.read()
            assert response.status==200,response.status
            connection.close()
            row=inbox.rows()[0]
            assert row["received"]==size
            assert row["phase"]=="validating"
            inbox.validate_batch(row["batch"])
            inbox.publish(row["id"])
            row=inbox.get(row["id"])
            destination=Path(row["target"])/"large.iso"
            assert destination.stat().st_size==size
            assert row["digest"]==expected.hexdigest()
            assert row["phase"]=="complete"
            print(json.dumps(dict(result="PASS",bytes=size,seconds=round(time.monotonic()-started,2),
                peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                sha256=row["digest"],transport="synthetic HTTPS Content-Length",published=True)))
        finally:
            server.shutdown()
            server.server_close()
            inbox.db.close()


if __name__=="__main__":
    main()
