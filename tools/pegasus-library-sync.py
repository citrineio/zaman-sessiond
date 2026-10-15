#!/usr/bin/env python3
"""Reconcile curated installed user Flatpaks into Pegasus's Native Games.

This tool owns only zaman-native-games.metadata.pegasus.txt and its marker files.
It never edits Pegasus settings, ROM metadata, or the Steam collection.
"""

import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import secrets
import struct
import subprocess
import sys
import tempfile
import zlib


GAMES = (
    ("org.supertuxproject.SuperTux", "SuperTux", "Platformer",
     "A jump-and-run game starring Tux the Penguin"),
    ("net.supertuxkart.SuperTuxKart", "SuperTuxKart", "Racing",
     "A kart racing game with Tux and friends"),
    ("org.wesnoth.Wesnoth", "The Battle for Wesnoth", "Strategy",
     "A turn-based fantasy strategy game"),
    ("org.openttd.OpenTTD", "OpenTTD", "Simulation",
     "An open source transport simulation game"),
    ("org.opensurge2d.OpenSurge", "Surge Engine", "Platformer",
     "Play Surge the Rabbit, a retro platformer, and create your own games"),
)
METADATA_NAME = "zaman-native-games.metadata.pegasus.txt"
SIGNAL_SUFFIX = ".signal"
ICON_SIZES = (512, 256, 128, 64, 48, 32)
MAX_IMAGE_BYTES = 16 * 1024 * 1024
# Verified upstream data/install paths; absence simply uses the exported icon.
PACKAGED_LOGOS = {
    "org.supertuxproject.SuperTux": "share/supertux2/images/engine/menu/logo.png",
    "net.supertuxkart.SuperTuxKart": "share/supertuxkart/data/gui/icons/logo_slim.png",
    "org.wesnoth.Wesnoth": "share/wesnoth/images/misc/logo.png",
}


def png_bytes(path: Path) -> bytes | None:
    """Ignore unavailable/truncated title art and fall through to an app icon.

    Flatpak exported icons are symlinks; read them without modifying the export.
    A PNG-only shared contract avoids requiring optional Qt SVG plugins.
    """
    try:
        with path.open("rb") as stream:
            data = stream.read(MAX_IMAGE_BYTES + 1)
        if len(data) > MAX_IMAGE_BYTES or data[:8] != b"\x89PNG\r\n\x1a\n":
            return None
        offset, width, height, has_pixels = 8, 0, 0, False
        while offset + 12 <= len(data):
            length = struct.unpack_from(">I", data, offset)[0]
            end = offset + length + 12
            if end > len(data):
                return None
            kind = data[offset + 4:offset + 8]
            payload = data[offset + 8:end - 4]
            checksum = struct.unpack_from(">I", data, end - 4)[0]
            if zlib.crc32(kind + payload) & 0xffffffff != checksum:
                return None
            if offset == 8:
                if kind != b"IHDR" or length != 13:
                    return None
                width, height = struct.unpack_from(">II", payload)
                if not (0 < width <= 8192 and 0 < height <= 8192):
                    return None
            if kind == b"IDAT":
                has_pixels = True
            if kind == b"IEND":
                return data if has_pixels and length == 0 and end == len(data) else None
            offset = end
    except OSError:
        pass
    return None


def artwork_sources(app_id: str) -> dict[str, bytes]:
    data = xdg_dir("XDG_DATA_HOME", Path.home() / ".local/share")
    logo = png_bytes(data / "zaman/game-artwork" / app_id / "logo.png")
    if logo is None and app_id in PACKAGED_LOGOS:
        logo = png_bytes(data / "flatpak/app" / app_id / "aarch64/stable/active/files"
                         / PACKAGED_LOGOS[app_id])
    icon_root = data / "flatpak/exports/share/icons/hicolor"
    icon = next((image for size in ICON_SIZES
                 if (image := png_bytes(icon_root / f"{size}x{size}/apps/{app_id}.png"))), None)
    # Supply common roles because Pegasus themes choose their own card asset.
    identity = logo or icon
    if not identity:
        return {}
    return {"logo": identity, "boxFront": identity, "tile": identity}


def artwork_path(directory: Path, image: bytes) -> Path:
    return directory / (hashlib.sha256(image).hexdigest() + ".png")


def publish_artwork(directory: Path, image: bytes) -> None:
    if directory.is_symlink():
        raise RuntimeError("Native artwork directory is a symlink; refusing to write")
    directory.mkdir(parents=True, exist_ok=True)
    path = artwork_path(directory, image)
    if path.is_symlink():
        raise RuntimeError("Native artwork file is a symlink; refusing to write")
    if path.is_file() and path.read_bytes() == image:
        return
    fd, temporary = tempfile.mkstemp(prefix=".artwork-", dir=directory)
    try:
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), 0o644)
            stream.write(image)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def xdg_dir(variable: str, default: Path) -> Path:
    value = os.environ.get(variable)
    path = Path(value) if value else default
    if not path.is_absolute():
        raise ValueError(f"{variable} must be an absolute path")
    return path


def paths() -> tuple[Path, Path, Path]:
    home = Path.home()
    config = xdg_dir("XDG_CONFIG_HOME", home / ".config")
    data = xdg_dir("XDG_DATA_HOME", home / ".local/share")
    state = xdg_dir("XDG_STATE_HOME", home / ".local/state")
    return (config / "pegasus-frontend/metafiles" / METADATA_NAME,
            data / "zaman/pegasus-native-games/refs",
            state / "zaman/pegasus-library-sync")


def installed_ids(flatpak: str = "/usr/bin/flatpak") -> set[str]:
    result = subprocess.run(
        [flatpak, "list", "--user", "--app", "--columns=ref:f"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        encoding="utf-8", errors="replace", timeout=25, check=False,
        env={**os.environ, "LC_ALL": "C"},
    )
    if result.returncode != 0:
        raise RuntimeError("Flatpak list failed: " + result.stderr.strip()[:240])
    wanted = {game[0] for game in GAMES}
    found = set()
    for raw in result.stdout.splitlines():
        ref = raw.strip()
        if not ref or ref == "Ref":
            continue
        parts = ref.removeprefix("app/").split("/")
        if len(parts) != 3 or not all(parts):
            raise RuntimeError("Flatpak list returned a malformed ref; library unchanged")
        app_id, arch, branch = parts
        if app_id in wanted and arch == "aarch64" and branch == "stable":
            found.add(app_id)
    return found


def marker_path(marker_dir: Path, app_id: str) -> Path:
    return marker_dir / (app_id + ".game")


def publish_theme_signal(metadata: Path) -> bool:
    """Tell the active Zaman Pegasus theme to rescan its in-process game model.

    A new filename is used because Qt 5's FolderListModel watches directory
    entries. Keeping one signal bounds directory size across transactions.
    """
    theme = metadata.parent.parent / "themes/zaman"
    if theme.is_symlink():
        raise RuntimeError("Zaman Pegasus theme is a symlink; refusing to signal")
    if not (theme / "theme.qml").is_file():
        return False
    signals = theme / "library-refresh"
    if signals.is_symlink():
        raise RuntimeError("Pegasus refresh directory is a symlink; refusing to signal")
    signals.mkdir(mode=0o755, exist_ok=True)
    path = signals / (secrets.token_hex(16) + SIGNAL_SUFFIX)
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o644)
    os.close(fd)
    for old in signals.glob("*" + SIGNAL_SUFFIX):
        if old != path:
            old.unlink()
    return True


def render(ids: set[str], marker_dir: Path, artwork: dict | None = None) -> str:
    if not ids:
        return ""
    lines = ["# Generated by Zaman; do not edit this file.",
             "collection: Native Games", "shortname: native", ""]
    for app_id, name, genre, summary in GAMES:
        if app_id not in ids:
            continue
        lines.extend((f"game: {name}",
                      f"file: {marker_path(marker_dir, app_id)}",
                      f"genre: {genre}", f"summary: {summary}",
                      f"launch: /usr/local/bin/zamanctl installed-game launch {app_id}"))
        for role, image in (artwork or {}).get(app_id, {}).items():
            path = artwork_path(marker_dir.parent / "artwork", image)
            # A newline would create a second metadata directive.
            if not any(ord(character) < 32 for character in str(path)):
                lines.append(f"assets.{role}: {path}")
        lines.append("")
    return "\n".join(lines) + "\n"


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".zaman-native-games-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            os.fchmod(stream.fileno(), 0o644)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def reconcile(dry_run: bool = False, flatpak: str = "/usr/bin/flatpak") -> str:
    metadata, marker_dir, state_dir = paths()
    if metadata.is_symlink() or marker_dir.is_symlink():
        raise RuntimeError("Zaman library path is a symlink; refusing to sync")
    if dry_run:
        return _reconcile(metadata, marker_dir, installed_ids(flatpak), True)
    state_dir.mkdir(parents=True, exist_ok=True)
    with (state_dir / "sync.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return _reconcile(metadata, marker_dir, installed_ids(flatpak), False)


def _reconcile(metadata: Path, marker_dir: Path, ids: set[str], dry_run: bool) -> str:
    artwork = {app_id: artwork_sources(app_id) for app_id in sorted(ids)}
    desired = render(ids, marker_dir, artwork)
    artwork_dir = marker_dir.parent / "artwork"
    images = {image for roles in artwork.values() for image in roles.values()}
    missing_artwork = any(png_bytes(artwork_path(artwork_dir, image)) != image for image in images)
    current = metadata.read_text(encoding="utf-8") if metadata.exists() else ""
    missing = [marker_path(marker_dir, item[0]) for item in GAMES
               if item[0] in ids and not marker_path(marker_dir, item[0]).is_file()]
    stale = [marker_path(marker_dir, item[0]) for item in GAMES
             if item[0] not in ids and marker_path(marker_dir, item[0]).exists()]
    for marker in missing + stale:
        if marker.is_symlink():
            raise RuntimeError(f"Zaman marker is a symlink: {marker}")
    changed = current != desired or bool(missing or stale) or missing_artwork
    if dry_run:
        return f"dry-run: {len(ids)} installed, {'change' if changed else 'unchanged'}"

    # Publish complete images before metadata; image hashes also invalidate Qt's
    # URL cache and rotate the reload signal when a logo changes at the same path.
    for image in images:
        publish_artwork(artwork_dir, image)
    # Create new markers before publishing metadata that references them.
    marker_dir.mkdir(parents=True, exist_ok=True)
    for marker in missing:
        fd = os.open(marker, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o644)
        os.close(fd)
    if desired and current != desired:
        atomic_write(metadata, desired)
    elif not desired and metadata.exists():
        metadata.unlink()
    # Unpublished stale markers may be removed after metadata is updated.
    for marker in stale:
        marker.unlink()
    signalled = publish_theme_signal(metadata) if changed else False
    message = f"{len(ids)} installed, {'updated' if changed else 'unchanged'}"
    if signalled:
        message += "; Pegasus live refresh requested"
    return message


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sync", choices=["sync"])
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        print(reconcile(args.dry_run))
        return 0
    except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"Pegasus library sync failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
