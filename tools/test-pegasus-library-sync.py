#!/usr/bin/env python3
"""Behavioral tests for the generated Pegasus collection."""

import importlib.util
import os
import json
from pathlib import Path
import subprocess
import tempfile
import struct
import unittest
import zlib
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("pegasus-library-sync.py")
SPEC = importlib.util.spec_from_file_location("pegasus_library_sync", SCRIPT)
sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sync)


def png(red=0):
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes([0, red, 0, 0]))) + chunk(b"IEND", b""))


class PegasusLibrarySyncTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.catalog = self.root / "catalog.json"
        self.catalog.write_bytes((SCRIPT.parent.parent / "catalog/games.json").read_bytes())
        env = patch.dict(os.environ, {
            "HOME": str(self.root),
            "ZAMAN_CATALOG_PATH": str(self.catalog),
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

    def test_live_catalog_add_disable_remove_and_invalid_preservation(self):
        app = "org.example.NewGame"
        row = {"id": app, "name": "New Game"}
        self.catalog.write_text(json.dumps({"schema_version": 1, "games": [row]}))
        self.image(f"flatpak/exports/share/icons/hicolor/128x128/apps/{app}.png")
        with self.flatpak(f"app/{app}/aarch64/stable\n"):
            sync.reconcile()
            self.assertIn("game: New Game", self.metadata.read_text())
            self.assertTrue(sync.marker_path(self.markers, app).exists())
            snapshot = {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file() and p != self.catalog}
            for malformed in ["{", json.dumps({"schema_version": 1, "games": [row, row]})]:
                self.catalog.write_text(malformed)
                with self.assertRaises(ValueError): sync.reconcile()
                self.assertEqual(snapshot, {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file() and p != self.catalog})
            self.catalog.unlink()
            with self.assertRaises(OSError): sync.reconcile()
            self.assertEqual(snapshot, {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file() and p != self.catalog})
            self.catalog.write_text(json.dumps({"schema_version": 1, "games": [{**row, "enabled": False}]}))
            sync.reconcile()
            self.assertFalse(self.metadata.exists())
            self.assertFalse(sync.marker_path(self.markers, app).exists())
            self.catalog.write_text(json.dumps({"schema_version": 1, "games": [row]}))
            sync.reconcile()
            self.catalog.write_text(json.dumps({"schema_version": 1, "games": []}))
            sync.reconcile()
            self.assertFalse(sync.marker_path(self.markers, app).exists())
        self.assertEqual(self.steam.read_text(), "collection: Steam\ngame: Steam\n")

    def signals(self):
        return list((self.theme / "library-refresh").glob("*.signal"))

    def flatpak(self, refs, code=0):
        return patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess(
            ["flatpak"], code, refs, "unavailable" if code else ""))

    def image(self, relative, red=0):
        path = self.root / "data" / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(png(red))
        return path

    def test_offline_exported_icon_is_copied_for_all_theme_roles(self):
        app = "org.supertuxproject.SuperTux"
        original = self.image(f"flatpak/exports/share/icons/hicolor/128x128/apps/{app}.png")
        # Flatpak exports are symlinks. A real symlink must still be usable.
        target = original.with_suffix(".actual")
        original.rename(target)
        original.symlink_to(target)
        with self.flatpak(f"app/{app}/aarch64/stable\n"):
            sync.reconcile()
            contents = self.metadata.read_text()
            for role in ("logo", "boxFront", "tile"):
                line = next(x for x in contents.splitlines() if x.startswith(f"assets.{role}: "))
                cached = Path(line.split(": ", 1)[1])
                self.assertEqual(cached.read_bytes(), target.read_bytes())
                self.assertFalse(cached.is_symlink())
            signals = self.signals()
            self.assertIn("unchanged", sync.reconcile())
            self.assertEqual(signals, self.signals())

    def test_title_priority_corrupt_fallback_and_logo_change_refresh(self):
        app = "org.supertuxproject.SuperTux"
        self.image(f"flatpak/exports/share/icons/hicolor/128x128/apps/{app}.png", 1)
        packaged = self.image(f"flatpak/app/{app}/aarch64/stable/active/files/"
                              + next(g["package_logo"] for g in sync.enabled_games() if g["id"] == app), 2)
        self.assertEqual(sync.artwork_sources(app, next(g["package_logo"] for g in sync.enabled_games() if g["id"] == app))["logo"], packaged.read_bytes())
        title = self.image(f"zaman/game-artwork/{app}/logo.png", 3)
        self.assertEqual(sync.artwork_sources(app, next(g["package_logo"] for g in sync.enabled_games() if g["id"] == app))["logo"], title.read_bytes())
        with self.flatpak(f"app/{app}/aarch64/stable\n"):
            sync.reconcile()
            before, signal = self.metadata.read_text(), self.signals()
            title.write_bytes(png(4))
            self.assertIn("updated", sync.reconcile())
            self.assertNotEqual(before, self.metadata.read_text())
            self.assertNotEqual(signal, self.signals())
            title.write_bytes(b"not an image")
            packaged.write_bytes(png(2)[:-1])
            self.assertEqual(sync.artwork_sources(app, next(g["package_logo"] for g in sync.enabled_games() if g["id"] == app))["logo"], png(1))
            sync.reconcile()

    def test_artwork_dry_run_and_repair(self):
        app = "org.openttd.OpenTTD"
        self.image(f"flatpak/exports/share/icons/hicolor/128x128/apps/{app}.png")
        cache = self.markers.parent / "artwork"
        with self.flatpak(f"app/{app}/aarch64/stable\n"):
            sync.reconcile(True)
            self.assertFalse(cache.exists())
            sync.reconcile()
            asset = next(cache.glob("*.png"))
            signal = self.signals()
            asset.write_bytes(b"corrupt cache")
            self.assertIn("updated", sync.reconcile())
            self.assertEqual(asset.read_bytes(), png())
            self.assertNotEqual(signal, self.signals())

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
