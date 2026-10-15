#!/usr/bin/env python3
"""Behavioral tests for the generated Pegasus collection."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("pegasus-library-sync.py")
SPEC = importlib.util.spec_from_file_location("pegasus_library_sync", SCRIPT)
sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sync)


class PegasusLibrarySyncTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        env = patch.dict(os.environ, {
            "HOME": str(self.root),
            "XDG_CONFIG_HOME": str(self.root / "config"),
            "XDG_DATA_HOME": str(self.root / "data"),
            "XDG_STATE_HOME": str(self.root / "state"),
        })
        env.start()
        self.addCleanup(env.stop)
        self.metadata, self.markers, _ = sync.paths()
        steam = self.metadata.parent / "steam.metadata.pegasus.txt"
        steam.parent.mkdir(parents=True)
        steam.write_text("collection: Steam\ngame: Steam\n")
        self.steam = steam
        self.theme = self.metadata.parent.parent / "themes/zaman"
        self.theme.mkdir(parents=True)
        (self.theme / "theme.qml").write_text("import QtQuick 2.12\n")

    def signals(self):
        return list((self.theme / "library-refresh").glob("*.signal"))

    def flatpak(self, refs, code=0):
        return patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess(
            ["flatpak"], code, refs, "unavailable" if code else ""))

    def test_two_installed_games_and_repeated_sync(self):
        refs = ("Ref\n"
                "app/net.supertuxkart.SuperTuxKart/aarch64/stable\n"
                "app/org.supertuxproject.SuperTux/aarch64/stable\n"
                "app/org.wesnoth.Wesnoth/x86_64/stable\n")
        with self.flatpak(refs):
            self.assertIn("2 installed, updated", sync.reconcile())
            contents = self.metadata.read_text()
            self.assertIn("game: SuperTuxKart", contents)
            self.assertIn("game: SuperTux", contents)
            self.assertNotIn("game: The Battle for Wesnoth", contents)
            self.assertIn("/usr/local/bin/zamanctl installed-game launch", contents)
            self.assertEqual(len(list(self.markers.glob("*.game"))), 2)
            first_signal = self.signals()
            self.assertEqual(len(first_signal), 1)
            self.assertIn("unchanged", sync.reconcile())
            self.assertEqual(contents, self.metadata.read_text())
            self.assertEqual(first_signal, self.signals())
        self.assertEqual(self.steam.read_text(), "collection: Steam\ngame: Steam\n")

    def test_external_removal_prunes_only_owned_entry(self):
        first = ("app/net.supertuxkart.SuperTuxKart/aarch64/stable\n"
                 "app/org.supertuxproject.SuperTux/aarch64/stable\n")
        with self.flatpak(first):
            sync.reconcile()
        with self.flatpak("app/org.supertuxproject.SuperTux/aarch64/stable\n"):
            sync.reconcile()
        self.assertNotIn("game: SuperTuxKart", self.metadata.read_text())
        self.assertFalse((self.markers / "net.supertuxkart.SuperTuxKart.game").exists())
        self.assertTrue((self.markers / "org.supertuxproject.SuperTux.game").exists())
        self.assertEqual(len(self.signals()), 1)
        self.assertTrue(self.steam.exists())

    def test_zero_installed_games_removes_only_generated_collection(self):
        with self.flatpak("Ref\napp/org.supertuxproject.SuperTux/aarch64/stable\n"):
            sync.reconcile()
        with self.flatpak("Ref\n"):
            self.assertIn("0 installed, updated", sync.reconcile())
        self.assertFalse(self.metadata.exists())
        self.assertEqual(list(self.markers.glob("*.game")), [])
        self.assertEqual(len(self.signals()), 1)
        self.assertTrue(self.steam.exists())

    def test_flatpak_failure_or_malformed_ref_preserves_library(self):
        with self.flatpak("app/org.supertuxproject.SuperTux/aarch64/stable\n"):
            sync.reconcile()
        original = self.metadata.read_bytes()
        with self.flatpak("", code=1):
            with self.assertRaises(RuntimeError):
                sync.reconcile()
        with self.flatpak("unexpected output\n"):
            with self.assertRaises(RuntimeError):
                sync.reconcile()
        self.assertEqual(original, self.metadata.read_bytes())

    def test_dry_run_does_not_create_generated_files(self):
        with self.flatpak("app/org.supertuxproject.SuperTux/aarch64/stable\n"):
            self.assertIn("dry-run: 1 installed, change", sync.reconcile(True))
        self.assertFalse(self.metadata.exists())
        self.assertFalse(self.markers.exists())
        self.assertFalse(self.signals())

    def test_changed_sync_rotates_signal_filename(self):
        with self.flatpak("app/org.supertuxproject.SuperTux/aarch64/stable\n"):
            sync.reconcile()
        first = self.signals()
        self.assertEqual(len(first), 1)
        with self.flatpak("app/org.openttd.OpenTTD/aarch64/stable\n"):
            sync.reconcile()
        second = self.signals()
        self.assertEqual(len(second), 1)
        self.assertNotEqual(first, second)
        self.assertFalse(first[0].exists())

    def test_refuse_owned_metadata_symlink(self):
        self.metadata.symlink_to(self.steam)
        with self.flatpak("app/org.supertuxproject.SuperTux/aarch64/stable\n"):
            with self.assertRaises(RuntimeError):
                sync.reconcile()
        self.assertEqual(self.steam.read_text(), "collection: Steam\ngame: Steam\n")


if __name__ == "__main__":
    unittest.main()
