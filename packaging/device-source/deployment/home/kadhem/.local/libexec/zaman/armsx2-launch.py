#!/usr/bin/env python3
"""Resume a matching ARMSX2 state, or boot a game normally. Python stdlib only."""
import argparse
import configparser
from functools import reduce
import json
import os
from pathlib import Path
import re
import stat
import struct
import sys
import time


class LaunchError(RuntimeError):
    pass


def exact(stream, count):
    data = stream.read(count)
    if len(data) != count:
        raise LaunchError("Truncated disc image or game cache")
    return data


def folders(data_root):
    cfg = configparser.ConfigParser(interpolation=None, strict=False)
    with (data_root / "inis/PCSX2.ini").open() as stream:
        cfg.read_file(stream)
    result = {}
    for key, default in (("savestates", "sstates"), ("cache", "cache")):
        value = Path(cfg.get("Folders", key, fallback=default))
        result[key] = value if value.is_absolute() else data_root / value
    return result


def cache_identity(cache, rom):
    """Read the verified little-endian version 34 layout. Last duplicate wins."""
    if not cache.exists():
        return None
    if cache.stat().st_size > 128 * 1024 * 1024:
        raise LaunchError("Game cache exceeds 128 MiB")
    result = None
    info = rom.stat()
    with cache.open("rb") as stream:
        if struct.unpack("<II", exact(stream, 8)) != (0x45434C47, 34):
            raise LaunchError("Unsupported ARMSX2 game cache; expected version 34")
        while True:
            first = stream.read(4)
            if not first:
                break
            strings = []
            for index in range(5):
                raw_size = first if index == 0 else exact(stream, 4)
                if len(raw_size) != 4:
                    raise LaunchError("Truncated cache string length")
                size, = struct.unpack("<I", raw_size)
                if size > 1024 * 1024:
                    raise LaunchError("Oversized cache string")
                strings.append(exact(stream, size).decode("utf-8"))
            kind, region, size, modified, crc, rating = struct.unpack("<BBQQIB", exact(stream, 23))
            if kind >= 4 or region >= 30 or rating > 6:
                raise LaunchError("Invalid cache entry")
            path, serial = strings[:2]
            if Path(path).is_absolute() and Path(path).resolve() == rom:
                result = (serial, crc) if kind == 0 and size == info.st_size and modified == int(info.st_mtime) else None
    if result and not re.fullmatch(r"[A-Z0-9]{4}-[A-Z0-9]{5,20}", result[0]):
        raise LaunchError("Unsupported disc serial in cache")
    return result


class ISO:
    """Bounded ISO9660 reader for ordinary 2048-byte-sector .iso files."""
    def __init__(self, stream):
        self.stream = stream
        self.size = os.fstat(stream.fileno()).st_size
        self.root = None
        for sector in range(16, 80):
            data = self.read(sector * 2048, 2048)
            if data[1:6] != b"CD001" or data[6] != 1:
                raise LaunchError("Unsupported ISO9660 volume descriptor")
            if data[0] == 1:
                if struct.unpack_from("<H", data, 128)[0] != 2048:
                    raise LaunchError("Unsupported ISO block size")
                self.root = self.record(data[156:156 + data[156]])
                break
            if data[0] == 255:
                break
        if self.root is None:
            raise LaunchError("No primary ISO9660 volume")

    def read(self, offset, size):
        if offset < 0 or size < 0 or offset + size > self.size:
            raise LaunchError("ISO extent outside disc image")
        self.stream.seek(offset)
        return exact(self.stream, size)

    def record(self, data):
        if len(data) < 34 or data[0] != len(data) or 33 + data[32] > len(data):
            raise LaunchError("Invalid ISO directory record")
        if data[1] or data[26] or data[27] or data[25] & 0x80:
            raise LaunchError("Unsupported ISO extended/interleaved/multi-extent file")
        extent, size = struct.unpack_from("<I", data, 2)[0], struct.unpack_from("<I", data, 10)[0]
        if extent != struct.unpack_from(">I", data, 6)[0] or size != struct.unpack_from(">I", data, 14)[0]:
            raise LaunchError("Conflicting ISO extent fields")
        return extent * 2048, size, data[25], data[33:33 + data[32]]

    def locate(self, path):
        current = self.root
        parts = path.replace("\\", "/").strip("/").split("/")
        if len(parts) > 16 or any(p in ("", ".", "..") for p in parts):
            raise LaunchError("Unsupported boot executable path")
        for part in parts:
            if not current[2] & 2 or current[1] > 16 * 1024 * 1024:
                raise LaunchError("Invalid/oversized ISO directory")
            data = self.read(current[0], current[1])
            offset, matches = 0, []
            while offset < len(data):
                count = data[offset]
                if count == 0:
                    offset = (offset // 2048 + 1) * 2048
                    continue
                if offset % 2048 + count > 2048:
                    raise LaunchError("ISO record crosses a sector")
                rec = self.record(data[offset:offset + count])
                name = rec[3].split(b";", 1)[0].decode("ascii")
                if name.upper() == part.split(";", 1)[0].upper():
                    matches.append(rec)
                offset += count
            if len(matches) != 1:
                raise LaunchError("Missing or ambiguous ISO file: " + part)
            current = matches[0]
        if current[2] & 2:
            raise LaunchError("Boot file is a directory")
        return current

    def file(self, path, limit):
        entry = self.locate(path)
        if entry[1] > limit:
            raise LaunchError("ISO file exceeds read limit: " + path)
        return self.read(entry[0], entry[1])


def iso_identity(rom):
    with rom.open("rb") as stream:
        iso = ISO(stream)
        cnf = iso.file("SYSTEM.CNF", 64 * 1024).decode("ascii")
        boot = None
        for line in cnf.splitlines():
            key, separator, value = line.partition("=")
            if separator and key.strip() == "BOOT2":
                boot = value.strip()
        if not boot:
            raise LaunchError("SYSTEM.CNF has no PS2 BOOT2 executable")
        match = re.fullmatch(r"cdrom0?:[\\/]?(.*)", boot, re.IGNORECASE)
        if not match:
            raise LaunchError("Unsupported PS2 BOOT2 device")
        elfpath = match.group(1).split(";", 1)[0]
        serial_match = re.fullmatch(r"([A-Za-z0-9]{4})[_-]([A-Za-z0-9]{3})\.([A-Za-z0-9]{2,17})", elfpath.replace("\\", "/").split("/")[-1])
        if not serial_match:
            raise LaunchError("Unsupported boot executable serial")
        serial = (serial_match[1] + "-" + serial_match[2] + serial_match[3]).upper()
        elf = iso.file(elfpath, 64 * 1024 * 1024)
        if len(elf) < 52 or elf[:6] != b"\x7fELF\x01\x01":
            raise LaunchError("Boot executable is not a little-endian ELF32 file")
        # ARMSX2's ElfObject::GetCRC XORs complete little-endian u32 words.
        crc = reduce(lambda value, item: value ^ item[0], struct.iter_unpack("<I", elf[:len(elf) // 4 * 4]), 0)
        return serial, crc


def plan(app_run, data_root, rom):
    app_run = app_run.expanduser().resolve(strict=True)
    data_root = data_root.expanduser().resolve(strict=True)
    rom = rom.expanduser().resolve(strict=True)
    if not app_run.is_file() or not os.access(app_run, os.X_OK) or not rom.is_file():
        raise LaunchError("Expected executable AppRun and regular ROM file")
    before = rom.stat()
    paths = folders(data_root)
    if rom.suffix.lower() == ".iso":
        serial, crc = iso_identity(rom)
        source = "iso"
    else:
        identity = cache_identity(paths["cache"] / "gamelist.cache", rom)
        if identity is None:
            raise LaunchError("No current cached disc identity for this format. Scan the game in ARMSX2 first; no saves were changed.")
        serial, crc = identity
        source = "cache-v34"
    after = rom.stat()
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
        raise LaunchError("ROM changed during identification; retry when copying is complete")
    state = paths["savestates"] / f"{serial} ({crc:08X}).resume.p2s"
    # Preserve AppRun's existing profile selection. In this build -datapath
    # appends an ARMSX2 subdirectory; passing data_root would select a new profile.
    args = [str(app_run), "-batch", "-fullscreen"]
    mode = "boot"
    try:
        info = state.stat()
    except FileNotFoundError:
        if state.is_symlink():
            raise LaunchError("Resume state is a broken symlink; refusing normal boot")
    else:
        if not stat.S_ISREG(info.st_mode) or not info.st_size:
            raise LaunchError("Existing resume state is empty or not a regular file; refusing normal boot")
        with state.open("rb") as stream:
            stream.read(1)
        args += ["-statefile", str(state)]
        mode = "resume"
    args += ["--", str(rom)]
    return {"mode": mode, "identity_source": source, "serial": serial, "crc": f"{crc:08X}", "state": str(state), "rom": str(rom), "argv": args}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app-run", required=True, type=Path)
    parser.add_argument("--data-root", required=True, type=Path)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("rom", type=Path)
    args = parser.parse_args()
    try:
        decision = plan(args.app_run, args.data_root, args.rom)
        if args.dry_run:
            print(json.dumps(decision, indent=2))
            return
        message = f"[ps2-launch] {decision['mode']} serial={decision['serial']} crc={decision['crc']} source={decision['identity_source']}"
        print(message, file=sys.stderr, flush=True)
        # A direct log remains usable when service journaling is unavailable.
        try:
            runtime = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"))
            fd = os.open(runtime / "zaman-ps2-launch.log", os.O_WRONLY | os.O_CREAT | os.O_APPEND | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "a") as log:
                log.write(time.strftime("%Y-%m-%dT%H:%M:%S%z ") + message + "\n")
        except OSError:
            pass
        # Replace this process: sessiond's transient-unit ownership and signal path stay intact.
        os.execv(decision["argv"][0], decision["argv"])
    except (OSError, UnicodeError, ValueError, configparser.Error, LaunchError) as error:
        parser.exit(1, "PS2 launch refused: " + str(error) + "\n")


if __name__ == "__main__":
    main()
