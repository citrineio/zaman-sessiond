import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location("transfer_deploy",Path(__file__).resolve().parents[1]/"deploy.py")
deploy=importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        self.before=b"prior accepted UI"
        self.after=b"candidate UI"
        self.target=self.root/"live.qml"
        self.backup=self.root/"backup"
        self.target.write_bytes(self.after)
        self.backup.write_bytes(self.before)
        self.record=dict(phase="prepared",changes=[dict(target=str(self.target),
            before=hashlib.sha256(self.before).hexdigest(),after=hashlib.sha256(self.after).hexdigest(),
            backup=str(self.backup),mode=0o644)])
        (self.root/"activation.json").write_text(json.dumps(self.record))

    def test_rollback_restores_only_recorded_accepted_file(self):
        with patch.object(deploy,"ROOT",self.root),patch.object(deploy,"run"),patch.object(deploy.subprocess,"run"):
            deploy.rollback()
        self.assertEqual(self.target.read_bytes(),self.before)
        self.assertEqual(json.loads((self.root/"activation.json").read_text())["phase"],"rolled-back")

    def test_rollback_refuses_to_overwrite_newer_work_before_stopping_anything(self):
        self.target.write_bytes(b"owner's later edits")
        with patch.object(deploy,"ROOT",self.root),patch.object(deploy.subprocess,"run") as run:
            with self.assertRaisesRegex(RuntimeError,"newer work"):
                deploy.rollback()
            run.assert_not_called()
        self.assertEqual(self.target.read_bytes(),b"owner's later edits")

    def test_symlink_target_refused(self):
        link=self.root/"link"
        link.symlink_to(self.target)
        with self.assertRaisesRegex(RuntimeError,"symlink"):
            deploy.digest(link)

    def test_active_rollback_can_recover_a_broken_renderer(self):
        def call(args):
            method=args[-1]
            data={"Status":["Idle"],"ForegroundStatus":["library","", ""],
                  "StorefrontStatus":[""],"InstalledGameStatus":["","","","Exited"]}[method]
            return json.dumps(dict(data=data))
        with patch.object(deploy,"output",side_effect=call),patch.object(deploy,"run") as run:
            deploy.idle(dict(live="unused",sessiond="unused"),rollback=True)
        self.assertEqual(run.call_args.args[0][-2:],["stop","zaman-quickshell.service"])


if __name__=="__main__":
    unittest.main()
