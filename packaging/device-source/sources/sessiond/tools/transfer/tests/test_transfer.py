"""Behavioral tests: recovery, TLS protocol, disk publication and inbox policy."""
import hashlib
import http.client
import io
import json
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"receiver"))
from zaman_inbox import Inbox, Registry, descriptor_refs
from zaman_receiver import Receiver, HttpServer, ProtocolError, safe_to_publish


class Setup(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.rom = self.root/"roms"
        self.rom.mkdir()
        self.regroot = self.root/"registry"
        (self.regroot/"systems.d").mkdir(parents=True)
        for sid,ext in {"nes":["nes","zip"],"snes":["sfc","zip"],"ps1":["cue","bin","iso","m3u"],"ps2":["iso","bin","m3u"]}.items():
            self.system(sid,ext)
        self.registry = Registry([self.regroot],self.rom)
        self.inbox = Inbox(self.root/"state",self.registry,self.root/"metafiles",reserve=0)
        self.addCleanup(self.inbox.db.close)
        self.receiver = Receiver(self.inbox)

    def system(self,sid,ext,root=None):
        root = root or self.regroot
        (root/"systems.d").mkdir(parents=True,exist_ok=True)
        (root/"systems.d"/(sid+".toml")).write_text(
            f'schema=1\n[system]\nid="{sid}"\nname="{sid}"\nextensions={json.dumps(ext)}\n')

    def offer(self, files):
        _, ids = self.inbox.offer({str(n):dict(fileName=name,size=len(data)) for n,(name,data) in enumerate(files.items())},"Phone")
        return dict(zip(files,ids.values()))

    def deliver(self,files):
        ids = self.offer(files)
        for name, ident in ids.items():
            self.inbox.receive(ident,[files[name]])
        batch = self.inbox.get(next(iter(ids.values())))["batch"]
        self.inbox.validate_batch(batch)
        return ids


class InboxTests(Setup):
    def test_remove_history_keeps_games_metadata_dedup_and_survives_restart(self):
        content=b"NES\x1ahistory"
        ident=self.deliver({"Keep.nes":content})["Keep.nes"]
        self.inbox.publish(ident)
        target=Path(self.inbox.get(ident)["target"])
        metadata=self.root/"metafiles/zaman-transfer-nes.metadata.pegasus.txt"
        original=metadata.read_bytes()
        result=self.receiver.control({"action":"dismiss-history","id":ident})
        self.assertEqual(result["jobs"],[])
        self.assertEqual(result["historyCount"],0)
        self.assertEqual((target/"Keep.nes").read_bytes(),content)
        self.inbox.write_metadata()
        self.assertEqual(metadata.read_bytes(),original)
        with patch("zaman_receiver.notify",return_value=True) as notice:
            self.receiver.work_once(safe=lambda:True)
            notice.assert_not_called()
        reopened=Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(reopened.snapshot(),[])
            self.assertEqual(reopened.get(ident)["hidden"],1)
            reopened.write_metadata()
        finally:
            reopened.db.close()
        again=self.deliver({"Keep.nes":content})["Keep.nes"]
        self.inbox.publish(again)
        self.assertEqual(self.inbox.get(again)["target"],str(target))
        self.assertEqual(len(list((self.rom/"nes").glob("import-*"))),1)
        self.assertEqual(metadata.read_bytes(),original)

    def test_clear_history_keeps_every_pending_phase_and_staging_file(self):
        phases=["offered","receiving","validating","needs-destination","needs-files","ready","importing",
                "complete","failed","cancelled","attached"]
        ids=self.offer({phase+".iso":b"disc" for phase in phases})
        for phase in phases:
            ident=ids[phase+".iso"]
            self.inbox.update(ident,phase=phase)
            self.inbox.path(ident).write_bytes(b"disc")
            if phase not in {"complete","failed","cancelled"}:
                with self.assertRaisesRegex(ValueError,"Only finished"):
                    self.inbox.dismiss_history(ident)
        self.assertEqual(self.inbox.history_count(),3)
        result=self.receiver.control({"action":"clear-history"})
        self.assertEqual(result["historyCount"],0)
        self.assertEqual({r["phase"] for r in result["jobs"]},set(phases[:7]))
        for phase in phases:
            ident=ids[phase+".iso"]
            self.assertTrue(self.inbox.path(ident).exists())
            self.assertEqual(self.inbox.get(ident)["hidden"],int(phase in {"complete","failed","cancelled"}))

    def test_history_migration_keeps_existing_rows(self):
        ident=self.deliver({"Pending.iso":b"disc"})["Pending.iso"]
        self.inbox.db.execute("ALTER TABLE items DROP COLUMN hidden")
        self.inbox.db.commit()
        reopened=Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(reopened.get(ident)["hidden"],0)
            self.assertEqual(reopened.snapshot()[0]["phase"],"needs-destination")
        finally:
            reopened.db.close()

    def test_recovered_failed_import_returns_even_after_history_was_cleared(self):
        ident=self.offer({"Recovered.nes":b"data"})["Recovered.nes"]
        self.inbox.update(ident,phase="failed")
        self.inbox.dismiss_history(ident)
        self.inbox.path(ident).write_bytes(b"data")
        reopened=Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(reopened.get(ident)["phase"],"validating")
            self.assertEqual(reopened.snapshot()[0]["id"],ident)
        finally:
            reopened.db.close()

    def test_dismiss_requires_id_and_clear_includes_older_than_visible_limit(self):
        with self.assertRaisesRegex(ValueError,"Choose a finished"):
            self.receiver.control({"action":"dismiss-history"})
        ids=self.offer({str(i)+".nes":b"data" for i in range(102)})
        for ident in ids.values():
            self.inbox.update(ident,phase="failed")
        self.assertEqual(len(self.inbox.snapshot()),100)
        self.assertEqual(self.inbox.history_count(),102)
        self.receiver.control({"action":"clear-history"})
        self.assertEqual(self.inbox.snapshot(),[])
        self.assertTrue(all(self.inbox.get(ident)["hidden"] for ident in ids.values()))

    def test_override_replaces_extensions_instead_of_union(self):
        other = self.root/"etc"
        self.system("nes",["nes"],other)
        r = Registry([self.regroot,other],self.rom)
        self.assertEqual(r.candidates("a.zip"),["snes"])

    def test_unique_format_waits_for_safe_publication_and_metadata(self):
        ident = self.deliver({"Demo.nes":b"NES\x1a"+bytes(4096)})["Demo.nes"]
        with patch("zaman_receiver.notify",return_value=True):
            self.receiver.work_once(safe=lambda:False)
        self.assertEqual(self.inbox.get(ident)["phase"],"ready")
        self.assertFalse((self.rom/"nes").exists())
        with patch("zaman_receiver.notify",return_value=True):
            self.receiver.work_once(safe=lambda:True)
        row = self.inbox.get(ident)
        self.assertEqual(row["phase"],"complete")
        self.assertEqual((Path(row["target"])/"Demo.nes").read_bytes(),b"NES\x1a"+bytes(4096))
        self.assertIn("game: Demo",next((self.root/"metafiles").glob("*")).read_text())

    def test_iso_needs_explicit_destination(self):
        ident = self.deliver({"game.iso":b"disc"})["game.iso"]
        row = self.inbox.get(ident)
        self.assertEqual(row["phase"],"needs-destination")
        self.assertEqual(json.loads(row["candidates"]),["ps1","ps2"])
        with self.assertRaises(ValueError):
            self.inbox.choose(ident,"nes")
        self.inbox.choose(ident,"ps2")
        self.inbox.publish(ident)
        self.assertEqual(self.inbox.get(ident)["system"],"ps2")

    def test_queue_survives_close_and_reopen(self):
        ident = self.deliver({"a.iso":b"disc"})["a.iso"]
        other = Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(other.get(ident)["phase"],"needs-destination")
        finally:
            other.db.close()

    def test_explicit_cancel_reclaims_staging_without_touching_library(self):
        ident=self.deliver({"a.iso":b"disc"})["a.iso"]
        self.assertTrue(self.inbox.path(ident).exists())
        self.inbox.cancel(ident)
        self.assertFalse(self.inbox.path(ident).exists())
        self.assertEqual(self.inbox.get(ident)["phase"],"cancelled")

    def test_validation_cannot_resurrect_an_import_cancelled_during_inspection(self):
        ident=self.offer({"a.nes":b"bytes"})["a.nes"]
        self.inbox.receive(ident,[b"bytes"])
        entered,release=threading.Event(),threading.Event()
        def inspect(*args):
            entered.set()
            release.wait(2)
            return ["nes"],"Registry"
        with patch.object(self.registry,"inspect",side_effect=inspect):
            thread=threading.Thread(target=self.inbox.validate_batch,args=(self.inbox.get(ident)["batch"],))
            thread.start()
            self.assertTrue(entered.wait(2))
            self.inbox.cancel(ident)
            release.set()
            thread.join(2)
        self.assertEqual(self.inbox.get(ident)["phase"],"cancelled")

    def test_interrupted_stream_recovery_has_no_published_file(self):
        ident = self.offer({"a.nes":b"1234"})["a.nes"]
        self.inbox.update(ident,phase="receiving",received=2)
        self.inbox.path(ident,True).write_bytes(b"12")
        other = Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(other.get(ident)["phase"],"failed")
            self.assertFalse((self.rom/"nes").exists())
        finally:
            other.db.close()

    def test_receive_commit_boundary_recovers_ready_file(self):
        ident = self.offer({"a.nes":b"1234"})["a.nes"]
        self.inbox.path(ident).write_bytes(b"1234")
        other = Inbox(self.root/"state",self.registry,self.root/"metafiles",0)
        try:
            self.assertEqual(other.get(ident)["phase"],"validating")
            other.validate_batch(other.get(ident)["batch"])
            self.assertEqual(other.get(ident)["phase"],"ready")
        finally:
            other.db.close()

    def test_incomplete_upload_and_checksum_mismatch_are_not_success(self):
        batch,ids = self.inbox.offer({"a":dict(fileName="a.nes",size=4,sha256="0"*64)},"PC")
        ident = ids["a"]
        with self.assertRaisesRegex(ValueError,"checksum"):
            self.inbox.receive(ident,[b"1234"])
        self.assertEqual(self.inbox.get(ident)["phase"],"failed")
        self.assertFalse(self.inbox.path(ident).exists())
        ident = self.offer({"b.nes":b"12345"})["b.nes"]
        with self.assertRaises(ValueError):
            self.inbox.receive(ident,[b"123"])
        self.assertFalse(self.inbox.path(ident).exists())

    def test_zip_evidence_and_crc_validation(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream,"w") as z:
            z.writestr("game.nes",b"NES\x1a1234")
        ident = self.deliver({"game.zip":stream.getvalue()})["game.zip"]
        self.assertEqual(self.inbox.get(ident)["system"],"nes")
        ident = self.deliver({"bad.zip":b"invalid archive"})["bad.zip"]
        self.assertEqual(self.inbox.get(ident)["phase"],"needs-files")

    def test_multifile_cue_attaches_tracks_and_publishes_together(self):
        ids = self.deliver({"Disc.cue":b'FILE "Track 01.bin" BINARY\n TRACK 01 MODE2/2352\n',"Track 01.bin":b"data"})
        cue = self.inbox.get(ids["Disc.cue"])
        self.assertEqual(cue["system"],"ps1")
        self.assertEqual(self.inbox.get(ids["Track 01.bin"])["phase"],"attached")
        self.inbox.publish(cue["id"])
        target = Path(self.inbox.get(cue["id"])["target"])
        self.assertTrue((target/"Track 01.bin").exists())
        meta = (self.root/"metafiles/zaman-transfer-ps1.metadata.pegasus.txt").read_text()
        self.assertIn("ignore-file: "+str(target/"Track 01.bin"),meta)
        self.assertEqual(meta.count("game:"),1)

    def test_missing_companion_is_retained(self):
        ident = self.deliver({"Disc.cue":b'FILE "missing.bin" BINARY\n'})["Disc.cue"]
        self.assertEqual(self.inbox.get(ident)["phase"],"needs-files")
        self.assertIn("Missing companion",self.inbox.get(ident)["error"])

    def test_shared_companion_survives_first_disc_publication(self):
        ids = self.deliver({"One.cue":b'FILE "shared.bin" BINARY\n',
                            "Two.cue":b'FILE "shared.bin" BINARY\n',"shared.bin":b"track"})
        self.inbox.publish(ids["One.cue"])
        self.inbox.dismiss_history()
        self.assertTrue(self.inbox.path(ids["shared.bin"]).exists())
        self.inbox.publish(ids["Two.cue"])
        self.assertEqual(self.inbox.get(ids["Two.cue"])["phase"],"complete")
        self.assertFalse(self.inbox.path(ids["shared.bin"]).exists())

    def test_playlist_cannot_bypass_invalid_nested_descriptor(self):
        ids=self.deliver({"Playlist.m3u":b"Disc.cue\n","Disc.cue":b'FILE "../unsafe.bin" BINARY\n'})
        self.assertEqual(self.inbox.get(ids["Playlist.m3u"])["phase"],"needs-files")

    def test_playlist_cycle_and_traversal_are_not_imported(self):
        ids = self.deliver({"a.m3u":b"b.m3u\n","b.m3u":b"a.m3u\n"})
        self.assertTrue(all(self.inbox.get(i)["phase"]=="needs-files" for i in ids.values()))
        with self.assertRaises(ValueError):
            self.offer({"../game.nes":b"a"})
        ident = self.deliver({"bad.cue":b'FILE "../outside.bin" BINARY\n'})["bad.cue"]
        self.assertEqual(self.inbox.get(ident)["phase"],"needs-files")

    def test_repeated_transfer_deduplicates_and_changed_bytes_do_not_overwrite(self):
        first = self.deliver({"a.nes":b"first"})["a.nes"]
        self.inbox.publish(first)
        again = self.deliver({"a.nes":b"first"})["a.nes"]
        self.inbox.publish(again)
        self.assertEqual(self.inbox.get(first)["target"],self.inbox.get(again)["target"])
        self.assertEqual((self.root/"metafiles/zaman-transfer-nes.metadata.pegasus.txt").read_text().count("game:"),1)
        changed = self.deliver({"a.nes":b"changed"})["a.nes"]
        self.inbox.publish(changed)
        self.assertNotEqual(self.inbox.get(first)["target"],self.inbox.get(changed)["target"])
        self.assertEqual((Path(self.inbox.get(first)["target"])/"a.nes").read_bytes(),b"first")

    def test_existing_scraped_game_is_not_added_a_second_time(self):
        directory=self.rom/"nes"
        directory.mkdir()
        (directory/"a.nes").write_bytes(b"first")
        metadata='collection: nes\nextensions: nes\nlaunch: /usr/local/bin/zamanctl launch nes "{file.path}"\n\ngame: Original scraped title\nfile: a.nes\n'
        (directory/"metadata.pegasus.txt").write_text(metadata)
        ident=self.deliver({"a.nes":b"first"})["a.nes"]
        self.inbox.publish(ident)
        self.assertEqual(self.inbox.get(ident)["external"],1)
        self.assertEqual((directory/"metadata.pegasus.txt").read_text(),metadata)
        self.assertFalse((self.root/"metafiles/zaman-transfer-nes.metadata.pegasus.txt").exists())

    def test_publication_recovers_after_directory_commit(self):
        ident = self.deliver({"a.nes":b"first"})["a.nes"]
        self.inbox.publish(ident)
        self.inbox.update(ident,phase="importing")
        self.inbox.publish(ident)
        self.assertEqual(self.inbox.get(ident)["phase"],"complete")
        self.assertEqual(len(list((self.rom/"nes").glob("import-*"))),1)

    def test_low_storage_admission_leaves_no_queue_or_files(self):
        with patch("zaman_inbox.shutil.disk_usage",return_value=type("Disk",(),{"free":1})()):
            with self.assertRaisesRegex(ValueError,"storage"):
                self.offer({"a.nes":b"1234"})
        self.assertEqual(self.inbox.rows(),[])

    def test_large_byte_count_not_narrowed_to_uint32(self):
        size = 5*1024**3+17
        with patch("zaman_inbox.shutil.disk_usage",return_value=type("Disk",(),{"free":size*3})()):
            _,ids = self.inbox.offer({"id":dict(fileName="big.iso",size=size)},"PC")
        ident=ids["id"]
        self.inbox.update(ident,received=size-1)
        self.assertEqual(self.inbox.snapshot()[0]["received"],size-1)
        self.assertEqual(self.inbox.get(ident)["size"],size)

    def test_destination_low_storage_keeps_staging_and_no_partial_game(self):
        ident = self.deliver({"a.nes":bytes(3*1024*1024+7)})["a.nes"]
        with patch("zaman_inbox.shutil.disk_usage",return_value=type("Disk",(),{"free":1})()):
            with self.assertRaisesRegex(ValueError,"destination storage"):
                self.inbox.publish(ident)
        self.assertTrue(self.inbox.path(ident).is_file())
        self.assertFalse(Path(self.inbox.get(ident)["target"]).exists())
        self.assertEqual(list((self.rom/"nes").glob(".zaman-import-*")),[])
        self.assertNotIn("game:",(self.root/"metafiles/zaman-transfer-nes.metadata.pegasus.txt").read_text())


class ProtocolTests(Setup):
    def test_receiver_disabled_on_start_and_pin_required(self):
        msg=dict(info=dict(alias="PC"),files={"f":dict(fileName="a.nes",size=4)})
        with self.assertRaises(ProtocolError) as e:
            self.receiver.prepare(msg,"127.0.0.1","")
        self.assertEqual(e.exception.code,403)
        snap=self.receiver.control(dict(action="enable"))
        with self.assertRaises(ProtocolError) as e:
            self.receiver.prepare(msg,"127.0.0.1","")
        self.assertEqual(e.exception.code,401)
        prepared=self.receiver.prepare(msg,"127.0.0.1",snap["pin"])
        self.assertIn("f",prepared["files"])
        self.assertNotIn(prepared["sessionId"],json.dumps(self.inbox.rows()))

    def test_closing_receive_window_does_not_cancel_accepted_upload(self):
        snap=self.receiver.control(dict(action="enable"))
        ready=self.receiver.prepare(dict(files={"f":dict(fileName="a.nes",size=4)}),"ip",snap["pin"])
        self.receiver.control(dict(action="disable"))
        session,fid,ident=self.receiver.claim_upload({"sessionId":[ready["sessionId"]],"fileId":["f"],"token":[ready["files"]["f"]]},"ip")
        self.inbox.receive(ident,[b"1234"])
        self.assertEqual(self.inbox.get(ident)["phase"],"validating")

    def test_cancel_is_scoped_to_sender(self):
        snap=self.receiver.control(dict(action="enable"))
        ready=self.receiver.prepare(dict(files={"f":dict(fileName="a.nes",size=4)}),"ip",snap["pin"])
        with self.assertRaises(ProtocolError):
            self.receiver.cancel_session(ready["sessionId"],"other-ip")
        self.receiver.cancel_session(ready["sessionId"],"ip")
        self.assertEqual(self.inbox.rows()[0]["phase"],"cancelled")

    def test_sender_disappearance_does_not_block_forever(self):
        snap=self.receiver.control(dict(action="enable"))
        ready=self.receiver.prepare(dict(files={"f":dict(fileName="a.nes",size=4)}),"ip",snap["pin"])
        self.receiver.sessions[ready["sessionId"]]["touched"]-=121
        self.receiver.expire()
        self.assertEqual(self.inbox.rows()[0]["phase"],"failed")

    def test_safe_publication_fails_closed_during_every_game_kind(self):
        values={"Status":["Idle","","","","",""],"ForegroundStatus":["menu","library",""],
                "InstalledGameStatus":["0","","","Exited","",False],
                "StorefrontStatus":["",0,"","",False,""]}
        with patch("zaman_receiver.bus_call",side_effect=lambda method:values[method]):
            self.assertTrue(safe_to_publish())
            values["Status"][0]="Running"
            self.assertFalse(safe_to_publish())
            values["Status"][0]="Idle"
            values["InstalledGameStatus"][3]="Starting"
            self.assertFalse(safe_to_publish())
            values["InstalledGameStatus"][3]="Exited"
            values["StorefrontStatus"][4]=True
            self.assertFalse(safe_to_publish())
        with patch("zaman_receiver.bus_call",side_effect=OSError("bus lost")):
            self.assertFalse(safe_to_publish())

    def test_tls_fixed_length_chunked_and_checksum_error(self):
        cert,key=self.root/"test.crt",self.root/"test.key"
        subprocess.run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1",
                        "-subj","/CN=test","-keyout",str(key),"-out",str(cert)],check=True,capture_output=True)
        ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(cert,key)
        server=HttpServer(("127.0.0.1",0),self.receiver,ctx)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        def call(route,body,headers=None,chunked=False):
            c=http.client.HTTPSConnection("127.0.0.1",server.server_port,context=ssl._create_unverified_context(),timeout=3)
            c.request("POST",route,body=body,headers=headers or {},encode_chunked=chunked)
            response=c.getresponse()
            status,raw=response.status,response.read()
            c.close()
            return status,json.loads(raw) if raw else None
        snap=self.receiver.control(dict(action="enable"))
        data={"info":{"alias":"Stock-shaped synthetic client"},"files":{
            "fixed":{"fileName":"a.nes","size":4},"chunk":{"fileName":"b.nes","size":4},
            "bad":{"fileName":"bad.nes","size":4,"sha256":"0"*64}}}
        code,prepared=call("/api/localsend/v2/prepare-upload?pin="+snap["pin"],json.dumps(data),{"Content-Type":"application/json"})
        self.assertEqual(code,200)
        def route(fid):
            return "/api/localsend/v2/upload?sessionId="+prepared["sessionId"]+"&fileId="+fid+"&token="+prepared["files"][fid]
        self.assertEqual(call(route("fixed"),b"1234")[0],200)
        self.assertEqual(call(route("chunk"),iter([b"12",b"34"]),chunked=True)[0],200)
        self.assertEqual(call(route("bad"),b"1234")[0],422)
        self.assertEqual(call(route("fixed"),b"1234")[0],409)


if __name__=="__main__":
    unittest.main()
