"""Shared contract and live-edit authorization, with no deployed file required."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zaman_catalog as catalog


class CatalogTests(unittest.TestCase):
    def data(self, *games, version=1):
        return json.dumps({'schema_version': version, 'games': list(games)}).encode()

    def test_id_only_defaults_and_disabled(self):
        row, = catalog.parse(self.data({'id': 'org.example.Game'}))
        self.assertEqual((row['name'], row['genre'], row['enabled']), ('Game', 'Game', True))
        self.assertEqual(catalog.parse(self.data()), [])
        self.assertFalse(catalog.parse(self.data({'id': 'org.example.Game', 'enabled': False}))[0]['enabled'])

    def test_reject_bad_schema_types_ids_duplicates_and_injection(self):
        bad = [b'[]', b'{}', b'{', self.data(version=True), self.data(version=2),
               self.data({'id': 'org.example.Game'}, {'id': 'org.example.Game', 'enabled': False})]
        for value in ['../game', '--all', 'org.Game', 'org.exa mple.Game', 'org.example.Game\n', 'a.' + 'x' * 254 + '.Game']:
            bad.append(self.data({'id': value}))
        for change in [{'enabled': 1}, {'name': 2}, {'name': 'x'*161}, {'summary': 'safe\nlaunch: unsafe'},
                       {'summary': 'bad\x7f'}, {'package_logo': '../logo.png'}, {'package_logo': '/logo.png'},
                       {'package_logo': 'share//logo.png'}, {'package_logo': 'share\\logo.png'}, {'command': 'arbitrary'}]:
            bad.append(self.data({'id': 'org.example.Game', **change}))
        bad += [self.data(*[{'id': f'org.example.Game{i}'} for i in range(513)]), b' ' * (catalog.MAX_BYTES+1)]
        for value in bad:
            with self.subTest(value=value[:100]), self.assertRaises((ValueError, UnicodeError)):
                catalog.parse(value)

    def test_no_fallback_missing_relative_or_malformed(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'catalog.json'
            with patch.dict(os.environ, {'ZAMAN_CATALOG_PATH': str(path)}):
                with self.assertRaises(OSError): catalog.load()
                path.write_text('{')
                with self.assertRaises(ValueError): catalog.require_enabled('org.example.Game')
            with patch.dict(os.environ, {'ZAMAN_CATALOG_PATH': 'catalog.json'}):
                with self.assertRaises(ValueError): catalog.load()

    def test_fresh_authorization_after_add_disable_remove_and_repair(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'catalog.json'
            with patch.dict(os.environ, {'ZAMAN_CATALOG_PATH': str(path)}):
                for games, allowed in [([], False), ([{'id': 'org.example.New'}], True),
                                       ([{'id': 'org.example.New', 'enabled': False}], False), ([], False),
                                       ([{'id': 'org.example.New', 'name': 'New Title'}], True)]:
                    temporary = path.with_suffix('.tmp')
                    temporary.write_bytes(self.data(*games)); temporary.replace(path)
                    if allowed:
                        self.assertEqual(catalog.require_enabled('org.example.New')['id'], 'org.example.New')
                    else:
                        with self.assertRaises(ValueError): catalog.require_enabled('org.example.New')

    def test_canonical_catalog_includes_opensurge_and_unique_ids(self):
        games = catalog.load(Path(__file__).resolve().parents[1] / 'catalog/games.json')
        self.assertEqual(len(games), 5)
        self.assertIn('org.opensurge2d.OpenSurge', [g['id'] for g in games])


if __name__ == '__main__':
    unittest.main()
