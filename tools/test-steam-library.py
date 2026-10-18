#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
from unittest.mock import patch
import zaman_steam_library as lib


def binary_fixture(apps, version=41):
    keys = []
    def key_list(obj):
        for key, value in obj.items():
            if key not in keys:
                keys.append(key)
            if isinstance(value, dict):
                key_list(value)
    for app, obj in apps.items():
        key_list({'appinfo': {'appid': int(app), **obj}})
    def encode(obj):
        data = b''
        for key, value in obj.items():
            name = struct.pack('<I', keys.index(key)) if version == 41 else key.encode() + b'\0'
            if isinstance(value, dict):
                data += b'\0' + name + encode(value)
            elif isinstance(value, int):
                data += b'\x02' + name + struct.pack('<I', value)
            else:
                data += b'\x01' + name + str(value).encode() + b'\0'
        return data + b'\x08'
    entries = b''
    for app, obj in apps.items():
        payload = encode({'appinfo': {'appid': int(app), **obj}})
        if version == 39:
            header = struct.pack('<IIIIQ20sI', int(app), 40 + len(payload), 0, 0, 0, b'\0' * 20, 0)
        else:
            header = struct.pack('<IIIIQ20sI20s', int(app), 60 + len(payload), 0, 0, 0, b'\0' * 20, 0, b'\0' * 20)
        entries += header + payload
    entries += b'\0' * 4
    prefix = struct.pack('<II', 0x07564400 + version, 1)
    if version == 41:
        prefix += struct.pack('<Q', 16 + len(entries))
        entries += struct.pack('<I', len(keys)) + b''.join(key.encode() + b'\0' for key in keys)
    return prefix + entries


class LibraryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'Steam root'
        (self.root / 'steamapps/common').mkdir(parents=True)
        (self.root / 'appcache').mkdir()
        self.registry([self.root])
        self.apps = {}
        self.env = patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.base / 'config'),
            'XDG_DATA_HOME': str(self.base / 'data'), 'XDG_STATE_HOME': str(self.base / 'state'),
            'ZAMAN_STEAM_ROOT': str(self.root)})
        self.env.start(); self.addCleanup(self.env.stop)

    def registry(self, paths):
        body = '"libraryfolders" {' + ''.join('"%d" {"path" %s}' % (i, json.dumps(str(p))) for i, p in enumerate(paths)) + '}'
        (self.root / 'steamapps/libraryfolders.vdf').write_text(body)

    def game(self, app='712345', kind='game', library=None, flags=4, name='Synthetic game'):
        library = library or self.root
        (library / 'steamapps/common' / app).mkdir(parents=True, exist_ok=True)
        path = library / 'steamapps' / ('appmanifest_' + app + '.acf')
        path.write_text('"AppState" {"appid" "%s" "name" %s "installdir" "%s" "StateFlags" "%s"}' % (app, json.dumps(name), app, flags))
        if kind is not None:
            self.apps[app] = {'common': {'type': kind, 'name': name}}
        self.cache()
        return path

    def cache(self, version=41):
        (self.root / 'appcache/appinfo.vdf').write_bytes(binary_fixture(self.apps, version))

    def test_arbitrary_new_game_is_automatically_included(self):
        self.game('3987123')
        self.assertEqual(set(lib.scan(self.root)['games']), {'3987123'})

    def test_application_types_exclude_tools_and_dlc_without_id_lists(self):
        for app, kind in [('712345', 'Game'), ('712346', 'Tool'), ('712347', 'DLC'), ('712348', 'Demo')]:
            self.game(app, kind)
        result = lib.scan(self.root)
        self.assertEqual(set(result['games']), {'712345', '712348'})
        self.assertEqual({r['type'] for r in result['excluded']}, {'tool', 'dlc'})

    def test_unknown_metadata_is_explicit_not_guessed(self):
        self.game(kind=None, name='A name that sounds like a game')
        result = lib.scan(self.root)
        self.assertEqual(result['games'], {})
        self.assertEqual(result['unclassified'], ['712345'])

    def test_all_supported_appinfo_versions(self):
        self.game()
        for version in (39, 40, 41):
            with self.subTest(version=version):
                self.cache(version)
                self.assertIn('712345', lib.scan(self.root)['games'])

    def test_second_registered_library_and_alias_deduplication(self):
        other = self.base / 'External library'
        self.game(library=other)
        alias = self.base / 'alias'; alias.symlink_to(other, target_is_directory=True)
        self.registry([self.root, other, alias])
        result = lib.scan(self.root)
        self.assertEqual(len(result['libraries']), 2)
        self.assertEqual(result['games']['712345']['library'], str(other))

    def test_duplicate_app_prefers_ready_install(self):
        other = self.base / 'second'
        self.game(flags=6)
        self.game(library=other)
        self.registry([self.root, other])
        self.assertEqual(lib.scan(self.root)['games']['712345']['library'], str(other))

    def test_uninstall_removes_entry(self):
        path = self.game(); before = lib.scan(self.root)
        path.unlink()
        self.assertEqual(lib.scan(self.root, before)['games'], {})

    def test_missing_registered_drive_preserves_entry_as_unavailable(self):
        other = self.base / 'disk'
        self.game(library=other); self.registry([self.root, other])
        before = lib.scan(self.root)
        other.rename(self.base / 'unmounted')
        after = lib.scan(self.root, before)
        self.assertIn('712345', after['games'])
        self.assertFalse(after['games']['712345']['available'])

    def test_removed_library_is_not_preserved_forever(self):
        other = self.base / 'disk'
        self.game(library=other); self.registry([self.root, other]); before = lib.scan(self.root)
        self.registry([self.root])
        self.assertEqual(lib.scan(self.root, before)['games'], {})

    def test_empty_underlying_mount_directory_is_not_an_uninstall(self):
        other = self.base / 'disk'
        self.game(library=other); self.registry([self.root, other]); before = lib.scan(self.root)
        other.rename(self.base / 'detached-drive')
        (other / 'steamapps').mkdir(parents=True)
        after = lib.scan(self.root, before)
        self.assertIn('712345', after['games'])
        self.assertFalse(after['games']['712345']['available'])

    def test_partial_new_install_is_not_presented_as_ready(self):
        self.game(flags=6)
        self.assertEqual(lib.scan(self.root)['games'], {})

    def test_existing_game_updating_is_kept_with_unavailable_status(self):
        self.game(); before = lib.scan(self.root)
        self.game(flags=6)
        after = lib.scan(self.root, before)
        self.assertFalse(after['games']['712345']['available'])

    def test_truncated_manifest_preserves_only_known_entry(self):
        path = self.game(); before = lib.scan(self.root)
        path.write_text('"AppState" {')
        after = lib.scan(self.root, before)
        self.assertFalse(after['games']['712345']['available'])
        self.assertEqual(len(after['diagnostics']), 1)

    def test_manifest_identity_and_path_traversal_are_refused(self):
        path = self.game()
        for value in ['"AppState" {"appid" "1"}', '"AppState" {"appid" "712345" "installdir" "../bad"}']:
            path.write_text(value)
            with self.assertRaises(ValueError):
                lib.manifest(path, self.root)

    def test_corrupt_appinfo_leaves_published_metadata_unchanged(self):
        self.game(); lib.sync()
        output = lib.locations()['metadata']; original = output.read_bytes()
        (self.root / 'appcache/appinfo.vdf').write_bytes(b'broken')
        with self.assertRaises(ValueError):
            lib.sync()
        self.assertEqual(output.read_bytes(), original)

    def test_dry_run_performs_no_output_writes(self):
        self.game(); lib.sync(dry_run=True)
        self.assertFalse(lib.locations()['state'].exists())
        self.assertFalse(lib.locations()['metadata'].exists())

    def test_repeat_sync_does_not_rewrite_metadata_or_signal(self):
        self.game()
        theme = lib.locations()['config'] / 'pegasus-frontend/themes/zaman'
        theme.mkdir(parents=True); (theme / 'theme.qml').touch()
        lib.sync(); output = lib.locations()['metadata']
        old_stamp = lib.stamp(output)
        signals = list((theme / 'steam-library-refresh').iterdir())
        self.assertFalse(lib.sync()['changed'])
        self.assertEqual(lib.stamp(output), old_stamp)
        self.assertEqual(list((theme / 'steam-library-refresh').iterdir()), signals)

    def test_refresh_and_marker_removal_follow_uninstall(self):
        path = self.game(); lib.sync(); path.unlink(); lib.sync()
        self.assertEqual(lib.locations()['metadata'].read_text(), '')
        self.assertEqual(list(lib.locations()['refs'].glob('*.game')), [])

    def test_changed_art_gets_a_new_content_address(self):
        self.game()
        art = self.root / 'appcache/librarycache/712345/hash_library_600x900.jpg'
        art.parent.mkdir(parents=True); art.write_bytes(b'picture one')
        lib.sync(); first = lib.locations()['metadata'].read_text()
        self.assertIn('assets.boxFront:', first)
        art.write_bytes(b'picture two')
        self.assertTrue(lib.sync()['changed'])
        self.assertNotEqual(lib.locations()['metadata'].read_text(), first)
        self.assertEqual(len(list((lib.locations()['refs'].parent / 'artwork').iterdir())), 1)

    def test_title_newlines_cannot_inject_launch_directives(self):
        self.game(name='Title\nlaunch: evil')
        inventory = lib.scan(self.root)
        text = lib.render(inventory, self.base / 'refs', self.base / 'helper with spaces.py')
        self.assertEqual(text.count('\nlaunch:'), 1)
        self.assertIn('"' + str(self.base / 'helper with spaces.py') + '" launch 712345', text)

    def test_missing_registry_preserves_inventory_by_failing_scan(self):
        self.game()
        (self.root / 'steamapps/libraryfolders.vdf').unlink()
        with self.assertRaises(ValueError):
            lib.scan(self.root)

    def test_text_parser_handles_quotes_comments_and_nested_objects(self):
        self.assertEqual(lib.text_vdf('//comment\n"A" {"B" "say \\"hi\\""}'), {'a': {'b': 'say "hi"'}})
        for text in ['"A" {', '"A" "unterminated', '"A" }']:
            with self.assertRaises(ValueError):
                lib.text_vdf(text)


if __name__ == '__main__':
    unittest.main()
