"""Run the actual activation script against fake services and an isolated filesystem."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import tomllib

SCRIPT = Path(__file__).with_name('activate-shared-catalog.sh')


class ActivationTests(unittest.TestCase):
    def fixture(self, root, fail=False):
        repo = root/'sessiond'; tools=repo/'tools'; tools.mkdir(parents=True)
        lib=root/'local/libexec/zaman'; lib.mkdir(parents=True)
        (root/'local/bin').mkdir()
        config=root/'config'; store=config/'zaman/storefronts.d/dukkan.toml'; store.parent.mkdir(parents=True)
        store.write_text('argv = ["/old/dukkan", "--example"]\nworkdir = "/preserve"\n[env]\nKEEP = "yes"\n')
        (repo/'catalog').mkdir(); shutil.copy(SCRIPT.parent.parent/'catalog/games.json',repo/'catalog/games.json')
        for file in ['zaman_catalog.py','flatpak-game-session.py','pegasus-library-sync.py']:
            shutil.copy(SCRIPT.parent/file, tools/file)
        for file in ['flatpak-game-session.py','pegasus-library-sync.py']:
            (lib/file).write_text('old '+file)
        binary=repo/'target/shared-catalog/release/zaman-sessiond'; binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\nexit 0\n'); binary.chmod(0o755)
        cli=binary.with_name('zamanctl'); cli.write_text('#!/bin/sh\necho "CLI $*"\nexit 0\n'); cli.chmod(0o755)
        (root/'local/bin/zamanctl').write_text('old cli')
        dukkan=root/'dukkan/build-shared-catalog/dukkan'; dukkan.parent.mkdir(parents=True); dukkan.write_text('old'); dukkan.chmod(0o755)
        script=tools/SCRIPT.name
        script.write_text(SCRIPT.read_text().replace('/usr/local',str(root/'local')))
        fake=root/'fake'; fake.mkdir()
        commands={
          'id':'#!/bin/sh\necho 1000\n',
          'sudo':'#!/bin/sh\nif [ "$1" = "-v" ]; then exit 0; fi\nexec "$@"\n',
          'systemctl':'''#!/bin/sh
case "$*" in
 *MainPID*) echo 12345;;
 *ActiveState*) echo inactive;;
 *cat*) echo '[Service]'; echo 'ExecStart=/old/sessiond';;
esac
exit 0
''',
          'readlink':f'#!/bin/sh\necho "{root / "wrong" if fail else binary}"\n'
        }
        for name,content in commands.items():
            p=fake/name;p.write_text(content);p.chmod(0o755)
        env={**os.environ,'HOME':str(root),'XDG_CONFIG_HOME':str(config),'XDG_STATE_HOME':str(root/'state'), 'PATH':str(fake)+':'+os.environ['PATH']}
        return script, env, store, lib, binary

    def run_script(self, script, env):
        return subprocess.run(['bash',str(script)],env=env,text=True,capture_output=True,timeout=20)

    def test_activation_and_manual_rollback_preserve_old_config(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); script,env,store,lib,binary=self.fixture(root)
            before=store.read_bytes(); result=self.run_script(script,env)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            self.assertIn('Shared catalog activated',result.stdout)
            self.assertEqual(tomllib.loads(store.read_text())['env']['KEEP'], 'yes')
            self.assertIn('build-shared-catalog/dukkan',store.read_text())
            catalog=root/'local/share/zaman/catalog.json'; self.assertTrue(catalog.exists())
            restore=next((root/'state').rglob('restore.sh'))
            result=self.run_script(restore,env);self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            self.assertEqual(store.read_bytes(),before)
            self.assertFalse(catalog.exists()); self.assertFalse((lib/'zaman_catalog.py').exists())
            self.assertEqual((lib/'flatpak-game-session.py').read_text(),'old flatpak-game-session.py')
            self.assertEqual((root/'local/bin/zamanctl').read_text(),'old cli')

    def test_wrong_running_binary_rolls_back_automatically(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);script,env,store,lib,binary=self.fixture(root,True);before=store.read_bytes()
            result=self.run_script(script,env)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('restoring the backup',result.stderr)
            self.assertEqual(store.read_bytes(),before)
            self.assertFalse((root/'local/share/zaman/catalog.json').exists())
            self.assertEqual((root/'local/bin/zamanctl').read_text(),'old cli')

    def test_invalid_existing_catalog_aborts_without_changes(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);script,env,store,lib,binary=self.fixture(root);before=store.read_bytes()
            catalog=root/'local/share/zaman/catalog.json';catalog.parent.mkdir(parents=True);catalog.write_text('{')
            result=self.run_script(script,env)
            self.assertNotEqual(result.returncode,0)
            self.assertEqual(store.read_bytes(),before)
            self.assertEqual((root/'local/bin/zamanctl').read_text(),'old cli')
            self.assertFalse((root/'state').exists())

    def test_valid_operator_catalog_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);script,env,store,lib,binary=self.fixture(root)
            catalog=root/'local/share/zaman/catalog.json';catalog.parent.mkdir(parents=True)
            content='{"schema_version":1,"games":[{"id":"org.example.Operator"}]}\n';catalog.write_text(content)
            result=self.run_script(script,env)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            self.assertEqual(catalog.read_text(),content)


if __name__=='__main__': unittest.main()
