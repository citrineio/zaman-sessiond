#!/usr/bin/env python3
"""The shared curated-catalog contract. No embedded game list or fallback."""
import argparse
import json
import os
from pathlib import Path
import re
import sys

DEFAULT_PATH = '/usr/local/share/zaman/catalog.json'
MAX_BYTES = 1024 * 1024
ID = re.compile(r'[A-Za-z][A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]+){2,}', re.ASCII)
FIELDS = {'name': 160, 'genre': 80, 'summary': 600, 'tag': 80, 'package_logo': 512}


def valid_id(value):
    return isinstance(value, str) and len(value) <= 255 and ID.fullmatch(value) is not None


def catalog_path():
    path = Path(os.environ.get('ZAMAN_CATALOG_PATH', DEFAULT_PATH))
    if not path.is_absolute():
        raise ValueError('ZAMAN_CATALOG_PATH must be absolute')
    return path


def parse(data):
    if len(data) > MAX_BYTES:
        raise ValueError('catalog exceeds 1 MiB')
    root = json.loads(data)
    if not isinstance(root, dict) or set(root) != {'schema_version', 'games'}:
        raise ValueError('expected schema_version and games')
    if isinstance(root['schema_version'], bool) or root['schema_version'] != 1:
        raise ValueError('unsupported catalog schema_version')
    if not isinstance(root['games'], list) or len(root['games']) > 512:
        raise ValueError('games must be an array of at most 512 entries')
    games, seen = [], set()
    for row in root['games']:
        if not isinstance(row, dict) or set(row) - ({'id', 'enabled'} | set(FIELDS)):
            raise ValueError('unknown game field or invalid game object')
        app = row.get('id')
        if not valid_id(app) or app in seen:
            raise ValueError('invalid or duplicate game ID')
        seen.add(app)
        enabled = row.get('enabled', True)
        if not isinstance(enabled, bool):
            raise ValueError('enabled must be boolean')
        game = {'id': app, 'enabled': enabled}
        for field, limit in FIELDS.items():
            value = row.get(field, '')
            if not isinstance(value, str) or len(value) > limit or any(ord(c) < 32 or ord(c) == 127 for c in value):
                raise ValueError(f'invalid {field} for {app}')
            game[field] = value.strip()
        game['name'] = game['name'] or app.rsplit('.', 1)[-1]
        game['genre'] = game['genre'] or 'Game'
        logo = game['package_logo']
        if logo and (logo.startswith('/') or '\\' in logo or any(p in ('', '.', '..') for p in logo.split('/'))):
            raise ValueError(f'package_logo must be a safe relative path for {app}')
        games.append(game)
    return games


def load(path=None):
    path = Path(path) if path is not None else catalog_path()
    with path.open('rb') as stream:
        return parse(stream.read(MAX_BYTES + 1))


def enabled_games():
    return [game for game in load() if game['enabled']]


def require_enabled(app):
    for game in enabled_games():
        if game['id'] == app:
            return game
    raise ValueError('application is outside the enabled curated catalog')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate', 'path', 'list'])
    args = parser.parse_args()
    try:
        if args.action == 'path':
            print(catalog_path())
        else:
            games = load()
            if args.action == 'list':
                for game in games:
                    print(f"{game['id']}\t{'enabled' if game['enabled'] else 'disabled'}\t{game['name']}")
            else:
                print(f"Catalog valid: {len(games)} entries, {sum(g['enabled'] for g in games)} enabled ({catalog_path()})")
    except (OSError, ValueError) as error:
        print(f'Catalog error: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
