"""Exercise real Git checkpoints without the owner's device or network."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT.parent
SCRIPT = ROOT / 'tools/checkpoint-sprint1.py'
spec = importlib.util.spec_from_file_location('checkpoint', SCRIPT)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class CheckpointTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.manifest = json.loads(SCRIPT.with_name('sprint1-checkpoint.json').read_text())
        self.repos = {}
        for name in self.manifest['paths']:
            target = self.root / name
            shutil.copytree(WORKSPACE / name, target,
                            ignore=shutil.ignore_patterns('.git', 'target', 'build', '__pycache__'))
            subprocess.run(['git', 'init', '-q', '-b', 'sprint1/storefronts', str(target)], check=True)
            mod.git(target, 'config', 'user.name', 'Test')
            mod.git(target, 'config', 'user.email', 'test@example.com')
            mod.git(target, 'add', '.')
            mod.git(target, 'commit', '-qm', 'Fixture baseline')
            self.repos[name] = target

    def test_checkpoint_commits_only_allowlisted_sources_and_keeps_backups(self):
        repo = self.repos['zaman-sessiond']
        original = repo / 'tools/steam-foreground-handoff.py'
        mod.git(repo, 'rm', '--cached', str(original.relative_to(repo)))
        mod.git(repo, 'commit', '-qm', 'Untrack failed experiment')
        (repo / 'unrelated.txt').write_text('private untracked experiment')
        (repo / 'README.md').write_text((repo / 'README.md').read_text() + '\nCheckpoint fixture change\n')
        mod.preflight(self.repos, self.manifest)
        backup = self.root / 'backup'
        mod.checkpoint(self.repos, self.manifest, backup, {}, SCRIPT)
        refs = mod.commit_and_tag(self.repos, 'snapshot/test-accepted')
        self.assertEqual(set(refs), set(self.repos))
        names = mod.git(repo, 'ls-tree', '-r', '--name-only', 'HEAD').decode().splitlines()
        self.assertNotIn('unrelated.txt', names)
        self.assertNotIn('tools/steam-foreground-handoff.py', names)
        self.assertTrue(original.exists())
        self.assertTrue((backup / 'zaman-sessiond/before.bundle').is_file())
        self.assertTrue((backup / 'zaman-sessiond/files/README.md').is_file())
        self.assertIn(b'Checkpoint fixture change', (backup / 'zaman-sessiond/working.patch').read_bytes())
        mod.preflight(self.repos, self.manifest)
        # A repeated source checkpoint has no new staged changes or empty commit.
        mod.checkpoint(self.repos, self.manifest, self.root / 'repeat-backup', {}, SCRIPT)
        repeated = mod.commit_and_tag(self.repos, 'snapshot/test-repeat')
        self.assertEqual(refs, repeated)
        values = json.loads((self.repos['zaman-menu'] / 'SHA256.json').read_text())
        for relative, expected in values.items():
            self.assertEqual(mod.digest((self.repos['zaman-menu'] / relative).read_bytes()), expected)

    def test_unknown_staged_file_is_refused(self):
        repo = self.repos['dukkan']
        (repo / 'unrelated.txt').write_text('unrelated')
        mod.git(repo, 'add', 'unrelated.txt')
        with self.assertRaisesRegex(RuntimeError, 'unrelated staged'):
            mod.preflight(self.repos, self.manifest)

    def test_changed_tested_source_is_refused(self):
        (self.repos['zaman-menu'] / 'qml/shell.qml').write_text('different runtime')
        with self.assertRaisesRegex(RuntimeError, 'tested recovery'):
            mod.preflight(self.repos, self.manifest)

    def test_unknown_document_edit_is_refused(self):
        (self.repos['dukkan'] / 'docs/SESSION-INTEGRATION.md').write_text('owner edit')
        with self.assertRaisesRegex(RuntimeError, 'unexpected document edit'):
            mod.preflight(self.repos, self.manifest)

    def test_symlink_is_refused(self):
        path = self.repos['zaman-menu'] / 'qml/shell.qml'
        path.unlink()
        path.symlink_to(WORKSPACE / 'zaman-menu/qml/shell.qml')
        with self.assertRaisesRegex(RuntimeError, 'Unsafe path'):
            mod.preflight(self.repos, self.manifest)


if __name__ == '__main__':
    unittest.main()
