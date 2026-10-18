"""Offline Steam inventory and Pegasus output. Python standard library only.

Application identity and type come from Steam, never a maintained game list.
File formats: Valve KeyValues; appinfo versions 39, 40 and 41 (0x27..0x29).
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import struct
import tempfile

METADATA = 'zaman-steam-games.metadata.pegasus.txt'
APP_TYPES = {'game', 'demo'}


def app_id(value):
    value = str(value)
    if not re.fullmatch('[1-9][0-9]{0,9}', value) or int(value) > 0xffffffff:
        raise ValueError('Expected a canonical positive uint32 App ID')
    return value


def xdg(name, suffix):
    path = Path(os.environ.get(name) or Path.home() / suffix)
    if not path.is_absolute():
        raise ValueError(name + ' must be absolute')
    return path


def locations():
    config = xdg('XDG_CONFIG_HOME', '.config')
    data = xdg('XDG_DATA_HOME', '.local/share')
    state = xdg('XDG_STATE_HOME', '.local/state')
    return {'config': config, 'refs': data / 'zaman/pegasus-steam-games/refs',
            'metadata': config / 'pegasus-frontend/metafiles' / METADATA,
            'state': state / 'zaman/steam-library-sync',
            'launcher': Path.home() / '.local/libexec/zaman/steam-integration/steam-integration.py'}


def read_text(path):
    with path.open('rb') as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    if len(data) > 4 * 1024 * 1024:
        raise ValueError('Text VDF exceeds size limit')
    return data.decode('utf-8-sig')


def text_vdf(text):
    """Parse nesting, comments and quoted escapes; reject truncated snapshots."""
    tokens, i = [], 0
    while i < len(text):
        if text[i].isspace():
            i += 1
        elif text.startswith('//', i):
            end = text.find('\n', i)
            i = len(text) if end < 0 else end + 1
        elif text[i] in '{}':
            tokens.append((text[i], None)); i += 1
        elif text[i] == '"':
            i += 1; out = []
            while i < len(text) and text[i] != '"':
                c = text[i]; i += 1
                if c == '\\':
                    if i == len(text):
                        raise ValueError('Truncated VDF escape')
                    c = text[i]; i += 1
                    c = {'n': '\n', 'r': '\r', 't': '\t', '"': '"', '\\': '\\'}.get(c, '\\' + c)
                out.append(c)
            if i == len(text):
                raise ValueError('Unterminated VDF string')
            tokens.append(('string', ''.join(out))); i += 1
        else:
            start = i
            while i < len(text) and not text[i].isspace() and text[i] not in '{}':
                i += 1
            tokens.append(('string', text[start:i]))
    index = 0
    def obj(nested=False, depth=0):
        nonlocal index
        if depth > 64:
            raise ValueError('VDF nesting exceeds limit')
        result = {}
        while index < len(tokens):
            kind, key = tokens[index]; index += 1
            if kind == '}' and nested:
                return result
            if kind != 'string' or index == len(tokens):
                raise ValueError('Invalid VDF key/value')
            kind, value = tokens[index]; index += 1
            if kind == '{':
                value = obj(True, depth + 1)
            elif kind != 'string':
                raise ValueError('Invalid VDF value')
            result[key.lower()] = value
        if nested:
            raise ValueError('Unclosed VDF object')
        return result
    return obj()


def stamp(path):
    s = path.stat()
    return (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns)


def stable_text(path):
    before = stamp(path)
    result = text_vdf(read_text(path))
    if stamp(path) != before:
        raise ValueError('Steam rewrote a VDF during inspection; retry next scan')
    return result


def steam_root(explicit=None):
    if explicit:
        candidates = [Path(explicit).expanduser()]
    elif os.environ.get('ZAMAN_STEAM_ROOT'):
        candidates = [Path(os.environ['ZAMAN_STEAM_ROOT']).expanduser()]
    else:
        home = Path.home()
        candidates = [home / '.steam/root', home / '.steam/steam',
                      xdg('XDG_DATA_HOME', '.local/share') / 'Steam']
    found = sorted({str(p.resolve()) for p in candidates
                    if (p / 'steamapps').is_dir() and (p / 'appcache').is_dir()})
    if len(found) != 1:
        raise RuntimeError('Steam root is missing or ambiguous; set ZAMAN_STEAM_ROOT to its actual location')
    return Path(found[0])


def libraries(root):
    result = {root.resolve()}
    paths = [root / 'steamapps/libraryfolders.vdf', root / 'config/libraryfolders.vdf']
    found = False
    for path in paths:
        if not path.exists():
            continue
        found = True
        data = stable_text(path).get('libraryfolders')
        if not isinstance(data, dict):
            raise ValueError('Invalid Steam library registry')
        for key, entry in data.items():
            if not key.isdigit():
                continue
            value = entry.get('path') if isinstance(entry, dict) else entry
            if not isinstance(value, str) or not value or any(ord(c) < 32 for c in value):
                raise ValueError('Invalid registered library path')
            p = Path(value)
            if not p.is_absolute():
                raise ValueError('Relative Steam library path')
            result.add(p.resolve())
    if not found:
        raise ValueError('Steam library registry is unavailable; preserving previous inventory')
    return sorted(result, key=str)


def manifest(path, library):
    match = re.fullmatch(r'appmanifest_([1-9][0-9]*)\.acf', path.name)
    if not match:
        raise ValueError('Invalid manifest filename')
    app = app_id(match[1])
    data = stable_text(path).get('appstate')
    if not isinstance(data, dict) or data.get('appid') != app:
        raise ValueError('Manifest App ID mismatch')
    directory = data.get('installdir', '')
    if (not directory or directory in ('.', '..') or '/' in directory or '\\' in directory
            or any(ord(c) < 32 for c in directory)):
        raise ValueError('Invalid Steam installation directory')
    flags = int(data.get('stateflags', '0'))
    # 4 = fully installed; any other state is not advertised as ready.
    # Download byte counters can be historical, so do not use those counters.
    ready = flags == 4 and (library / 'steamapps/common' / directory).is_dir()
    return {'app': app, 'name': data.get('name') or 'Steam App ' + app,
            'library': str(library), 'manifest': str(path), 'installdir': directory,
            'ready': ready}


class BinaryKV:
    def __init__(self, data, start, end, keys=None):
        self.data, self.pos, self.end, self.keys = data, start, end, keys
        self.nodes = 0

    def take(self, size):
        if size < 0 or self.pos + size > self.end:
            raise ValueError('Truncated binary KeyValues')
        value = self.data[self.pos:self.pos + size]; self.pos += size
        return value

    def cstring(self):
        end = self.data.find(b'\0', self.pos, min(self.end, self.pos + 65537))
        if end < 0:
            raise ValueError('Unterminated binary string')
        value = self.data[self.pos:end].decode('utf-8', errors='replace')
        self.pos = end + 1
        return value

    def obj(self, depth=0):
        if depth > 64:
            raise ValueError('Binary KeyValues nesting limit')
        out = {}
        while True:
            self.nodes += 1
            if self.nodes > 200000:
                raise ValueError('Binary KeyValues node limit')
            kind = self.take(1)[0]
            if kind == 8:
                return out
            if self.keys is None:
                key = self.cstring().lower()
            else:
                index = struct.unpack('<I', self.take(4))[0]
                if index >= len(self.keys):
                    raise ValueError('Invalid appinfo string-table index')
                key = self.keys[index]
            if kind == 0:
                value = self.obj(depth + 1)
            elif kind == 1:
                value = self.cstring()
            elif kind in (2, 3, 4, 6):
                value = struct.unpack('<f' if kind == 3 else '<I', self.take(4))[0]
            elif kind == 7:
                value = struct.unpack('<Q', self.take(8))[0]
            elif kind == 10:
                value = struct.unpack('<q', self.take(8))[0]
            else:
                raise ValueError('Unsupported binary KeyValues type ' + str(kind))
            out[key] = value


def appinfo(path, wanted):
    before = stamp(path)
    if not 12 <= before[2] <= 1024 * 1024 * 1024:
        raise ValueError('Appinfo size outside supported bounds')
    result = {}
    # Take a bounded byte snapshot: an in-place Steam rewrite cannot SIGBUS
    # this process, as it could with a live mmap of appinfo.vdf.
    with path.open('rb') as stream:
        data = stream.read(before[2] + 1)
        if len(data) != before[2]:
            raise ValueError('Appinfo changed size during inspection')
        magic, universe = struct.unpack_from('<II', data)
        if magic not in (0x07564427, 0x07564428, 0x07564429) or universe != 1:
            raise ValueError('Unsupported Steam appinfo format')
        keys, offset, limit = None, 8, len(data)
        if magic == 0x07564429:
            table = struct.unpack_from('<Q', data, 8)[0]
            if not 20 <= table <= len(data) - 4:
                raise ValueError('Invalid appinfo string-table offset')
            count = struct.unpack_from('<I', data, table)[0]
            if count > 2000000:
                raise ValueError('Appinfo string-table limit')
            reader = BinaryKV(data, table + 4, len(data))
            keys = [reader.cstring().lower() for _ in range(count)]
            offset, limit = 16, table
        header_size = 48 if magic == 0x07564427 else 68
        terminated = False
        while offset + 4 <= limit:
            number = struct.unpack_from('<I', data, offset)[0]
            if not number:
                terminated = True; break
            if offset + header_size > limit:
                raise ValueError('Truncated appinfo record header')
            size = struct.unpack_from('<I', data, offset + 4)[0]
            end = offset + 8 + size
            if end < offset + header_size or end > limit:
                raise ValueError('Invalid appinfo record length')
            app = str(number)
            if app in wanted:
                parsed = BinaryKV(data, offset + header_size, end, keys).obj().get('appinfo', {})
                if not isinstance(parsed, dict) or str(parsed.get('appid')) != app:
                    raise ValueError('Appinfo identity mismatch')
                result[app] = parsed
            offset = end
        if not terminated:
            raise ValueError('Appinfo terminal record missing')
    if stamp(path) != before:
        raise ValueError('Appinfo changed during inspection; retry next scan')
    return result


def scan(root, previous=None):
    previous = previous or {}
    previous_games = previous.get('games', {}) if previous.get('root') == str(root) else {}
    previous_identities = previous.get('library_identities', {}) if previous.get('root') == str(root) else {}
    registered = libraries(root)
    candidates, unavailable, diagnostics = {}, [], []
    identities = {str(path): previous_identities[str(path)] for path in registered if str(path) in previous_identities}
    for library in registered:
        apps = library / 'steamapps'
        # An absent steamapps directory is not evidence that its games uninstalled.
        if not apps.is_dir():
            unavailable.append(str(library)); continue
        try:
            paths = sorted(Path(entry.path) for entry in os.scandir(apps)
                           if re.fullmatch(r'appmanifest_[1-9][0-9]*\.acf', entry.name))
            info = apps.stat()
            identity = [info.st_dev, info.st_ino]
            if not paths and str(library) in previous_identities and identity != previous_identities[str(library)]:
                # An unmounted drive can expose an empty underlying directory.
                unavailable.append(str(library)); continue
            identities[str(library)] = identity
            for path in paths:
                try:
                    row = manifest(path, library)
                    candidates.setdefault(row['app'], []).append(row)
                except (OSError, ValueError) as error:
                    diagnostics.append({'manifest': str(path), 'error': str(error)})
                    # Preserve only this previously known entry while Steam writes.
                    for app, old in previous_games.items():
                        if old.get('manifest') == str(path):
                            candidates.setdefault(app, []).append({**old, 'ready': False, 'uncertain': True})
        except OSError:
            unavailable.append(str(library))
    metadata = appinfo(root / 'appcache/appinfo.vdf', set(candidates))
    games, excluded, unknown = {}, [], []
    for app, rows in sorted(candidates.items(), key=lambda item: int(item[0])):
        info = metadata.get(app, {})
        common = info.get('common', {})
        kind = str(common.get('type', '')).lower()
        if not kind:
            old = previous_games.get(app)
            if old and old.get('type') in APP_TYPES:
                kind = old['type']
            else:
                unknown.append(app); continue
        if kind not in APP_TYPES:
            excluded.append({'app': app, 'type': kind}); continue
        row = next((r for r in rows if r.get('ready')), rows[0])
        if not row.get('ready') and app not in previous_games:
            continue
        games[app] = {**row, 'name': common.get('name') or row['name'], 'type': kind,
                      'available': row.get('ready', False)}
    for app, old in previous_games.items():
        if app not in games and old.get('library') in unavailable:
            games[app] = {**old, 'available': False}
    return {'version': 1, 'root': str(root), 'libraries': list(map(str, registered)),
            'library_identities': identities,
            'unavailable_libraries': unavailable, 'games': games,
            'excluded': excluded, 'unclassified': unknown, 'diagnostics': diagnostics}


def clean(value):
    return ' '.join(str(value).splitlines()).replace('\x00', '')


def artwork(root, app):
    cache = root / 'appcache/librarycache'
    # Steam has used both flat and per-App-ID/hash layouts. Roles are generic.
    patterns = {'boxFront': ['{app}_library_600x900*.*', '{app}_library_capsule*.*', '{app}/**/*library_600x900*.*'],
                'background': ['{app}_library_hero*.*', '{app}/**/*library_hero*.*'],
                'banner': ['{app}_header*.*', '{app}/**/*header*.*'],
                'logo': ['{app}_logo*.*', '{app}/**/*logo*.*']}
    result = {}
    for role, values in patterns.items():
        candidates = [p for pattern in values for p in cache.glob(pattern.format(app=app))
                      if p.suffix.lower() in ('.png', '.jpg', '.jpeg', '.webp') and p.is_file()]
        if candidates:
            result[role] = str(max(candidates, key=lambda p: (p.stat().st_mtime_ns, str(p))))
    if 'boxFront' not in result and 'banner' in result:
        result['boxFront'] = result['banner']
    return result


def quoted(path):
    value = str(path)
    if any(ord(c) < 32 for c in value):
        raise ValueError('Control character in launch path')
    # Pegasus uses QProcess argument splitting, not a shell.
    if '"' in value or '\\' in value:
        raise ValueError('Quote/backslash in helper path is unsupported')
    return '"' + value + '"'


def render(inventory, refs, launcher, art=None):
    if any(ord(c) < 32 for c in str(refs)):
        raise ValueError('Control character in marker path')
    if not inventory['games']:
        return ''
    lines = ['# Generated by Zaman Steam integration; do not edit.', 'collection: Steam', 'shortname: steam', '']
    for app, game in sorted(inventory['games'].items(), key=lambda item: (str(item[1]['name']).casefold(), int(item[0]))):
        app_id(app)
        lines += ['game: ' + clean(game['name']), 'file: ' + str(refs / (app + '.game')),
                  'launch: /usr/bin/python3 ' + quoted(launcher) + ' launch ' + app]
        if not game.get('available'):
            lines.append('summary: Installation unavailable or updating. Steam will verify it before launch.')
        for role, path in (art or {}).get(app, {}).items():
            if not any(ord(c) < 32 for c in path):
                lines.append('assets.' + role + ': ' + path)
        lines.append('')
    return '\n'.join(lines) + '\n'


def atomic(path, content, mode=0o600):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise RuntimeError('Refusing to replace symlink ' + str(path))
    fd, temporary = tempfile.mkstemp(prefix='.zaman-steam-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as out:
            out.write(content.encode() if isinstance(content, str) else content)
            os.fchmod(out.fileno(), mode); out.flush(); os.fsync(out.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def signal_refresh(config):
    theme = config / 'pegasus-frontend/themes/zaman'
    if not (theme / 'theme.qml').is_file():
        return False
    directory = theme / 'steam-library-refresh'
    if theme.is_symlink() or directory.is_symlink():
        raise RuntimeError('Unexpected symlink in Pegasus refresh path')
    directory.mkdir(exist_ok=True)
    path = directory / (secrets.token_hex(16) + '.signal')
    path.touch(exist_ok=False)
    # Same protocol, separate directory: Steam and Native Games cannot prune
    # each other's refresh token while publishing concurrently.
    for old in directory.glob('*.signal'):
        if old != path:
            old.unlink(missing_ok=True)
    return True


def previous_inventory(path):
    try:
        if path.stat().st_size > 8 * 1024 * 1024:
            raise ValueError('Inventory exceeds size bound')
        data = json.loads(path.read_text())
        if data.get('version') != 1 or not isinstance(data.get('games'), dict):
            raise ValueError('Unsupported inventory format')
        return data
    except FileNotFoundError:
        return {}


def sync(root=None, dry_run=False, launcher=None):
    root = steam_root(root)
    paths = locations()
    state = paths['state'] / 'inventory.json'
    lock = None
    if not dry_run:
        paths['state'].mkdir(parents=True, exist_ok=True)
        lock = (paths['state'] / 'sync.lock').open('a')
        fcntl.flock(lock, fcntl.LOCK_EX)
    try:
        inventory = scan(root, previous_inventory(state))
        art, images = {}, {}
        for app in inventory['games']:
            art[app] = {}
            for role, source in artwork(root, app).items():
                source = Path(source)
                try:
                    with source.open('rb') as stream:
                        data = stream.read(16 * 1024 * 1024 + 1)
                    if not data or len(data) > 16 * 1024 * 1024:
                        continue
                    # Content-addressed files avoid Qt reusing an old image URL.
                    target = paths['refs'].parent / 'artwork' / (hashlib.sha256(data).hexdigest() + source.suffix.lower())
                    images[target] = data
                    art[app][role] = str(target)
                except OSError:
                    pass
        desired = render(inventory, paths['refs'], launcher or paths['launcher'], art)
        current = paths['metadata'].read_text() if paths['metadata'].exists() else ''
        missing = [paths['refs'] / (app + '.game') for app in inventory['games']
                   if not (paths['refs'] / (app + '.game')).is_file()]
        changed = desired != current or bool(missing)
        if not dry_run:
            if paths['refs'].is_symlink() or paths['metadata'].is_symlink():
                raise RuntimeError('Unexpected symlink in Steam output paths')
            paths['refs'].mkdir(parents=True, exist_ok=True)
            for target, data in images.items():
                if not target.is_file():
                    atomic(target, data, 0o644)
            for marker in missing:
                marker.touch(exist_ok=False)
            if current != desired:
                atomic(paths['metadata'], desired, 0o644)
            atomic(state, json.dumps(inventory, ensure_ascii=False, indent=2) + '\n')
            # Prune only our own markers, after publishing the replacement list.
            for old in paths['refs'].glob('*.game'):
                if old.stem not in inventory['games']:
                    old.unlink()
            for old in (paths['refs'].parent / 'artwork').glob('*'):
                if old.is_file() and old not in images:
                    old.unlink()
            if changed:
                signal_refresh(paths['config'])
        return {**inventory, 'changed': changed, 'dry_run': dry_run}
    finally:
        if lock:
            lock.close()
