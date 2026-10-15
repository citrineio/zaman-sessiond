# Shared curated game catalog

Dukkan, sessiond, the transaction relay, managed-game helper, and Pegasus exporter
read `/usr/local/share/zaman/catalog.json`. The only shipped catalog is
`catalog/games.json` in this repository. There is no embedded fallback game list.
`ZAMAN_CATALOG_PATH` selects an absolute alternative file for development.
Deployment explicitly sets this same path for sessiond and Dukkan. CLI commands
use the default path unless this variable is set in their shell.

The catalog controls membership and optional fallback labels. Flathub/AppStream
still supplies Dukkan's live name, description, screenshots and icon. Locally
available title artwork takes precedence over application icons. The optional
`package_logo` field supplies the PNG path inside the installed Flatpak's `files`
directory; there is no longer a per-game logo lookup table in either program.
All packages remain user-installed Flathub aarch64/stable applications; catalog
entries cannot supply commands, remotes or alternative architectures.

```json
{
  "schema_version": 1,
  "games": [
    {"id": "org.opensurge2d.OpenSurge"}
  ]
}
```

Only `id` is required. Optional fields: `enabled` (boolean, default true), `name`,
`genre`, `summary`, `tag`, `package_logo` (strings). Name defaults to the final ID
component, genre to `Game`, other text to empty. Empty games arrays are valid.
The installed initial catalog has SuperTux, SuperTuxKart, Wesnoth, OpenTTD and
Surge Engine. An existing valid installed catalog is preserved during activation.

Readers reject unknown fields, unsupported schemas, wrong types, duplicate or
unsafe IDs, control characters, unsafe logo paths, files over 1 MiB and more than
512 entries. Text limits: name 160, genre/tag 80, summary 600, logo path 512 Unicode
characters. IDs are ASCII, at most 255 characters, with three or more dot-separated
components, and start with a letter. Optional strings are trimmed. Logo paths are
relative, with no empty, dot or parent segments or backslashes.

Missing/invalid JSON denies new transactions and launches. Dukkan retains the
last displayed list, shows an error and rechecks authorization before an action.
Refresh discards disabled/removed rows and ignores replies from earlier catalog
generations. Pegasus validates a full snapshot before any writes, preserving the
last good metadata/artwork on failure. A valid disabled or removed record prunes
only its generated Pegasus marker/entry; it does not uninstall the package.
Recovery and closing an already-owned game use its recorded nonce, exact Flatpak
instance and process identity, independently of current catalog membership.

## Build and activate on Zaman

Apply the supplied source patches first. These commands build into separate
paths, preserving previous binaries. They install no development packages.

```bash
bash <<'SH'
set -Eeuo pipefail
trap 'printf "ERROR at line %s (exit %s)\n" "$LINENO" "$?" >&2' ERR
root="$HOME/projects/zaman-sprint3.5"
printf '\n=== Dukkan checks and build ===\n'
cd "$root/dukkan"
python3 -m unittest discover -s tests
python3 tools/test-flatpak-transaction.py
node tests/keyboard-views.cjs
node tests/gallery-flow.cjs
cmake -S . -B build-shared-catalog -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
cmake --build build-shared-catalog --parallel 2
ctest --test-dir build-shared-catalog --output-on-failure
printf '\n=== sessiond checks and build ===\n'
cd "$root/zaman-sessiond"
python3 tools/test-zaman-catalog.py
python3 tools/test-pegasus-library-sync.py
python3 tools/test-flatpak-game-session.py
python3 tools/test-shared-catalog-activation.py
cargo test --locked --target-dir target/shared-catalog --lib --bin zaman-sessiond
cargo build --locked --release --target-dir target/shared-catalog --bin zaman-sessiond --bin zamanctl
printf '\n=== Builds and checks completed ===\n'
SH
```

Close games, transfer sessions, Dukkan and Steam normally before activating:

```bash
bash "$HOME/projects/zaman-sprint3.5/zaman-sessiond/tools/activate-shared-catalog.sh" \
  "$HOME/projects/zaman-sprint3.5/dukkan/build-shared-catalog/dukkan"
/usr/local/bin/zamanctl catalog path
/usr/local/bin/zamanctl catalog validate
/usr/local/bin/zamanctl catalog list
/usr/local/bin/zamanctl library sync --dry-run
/usr/local/bin/zamanctl storefront launch dukkan
```

Activation backs up the old installed helper files, CLI, catalog, Dukkan TOML and
its own systemd drop-in. It updates Dukkan's executable path and catalog variables,
preserving other arguments, environment, workdir and native Guide setting. It
sets a `zz-shared-catalog.conf` user-service drop-in, verifies the running daemon's
`/proc/PID/exe`, then syncs Pegasus. Failure invokes rollback automatically. It
prints a durable `bash /absolute/path/restore.sh` command; close game/storefront
sessions before using it later. No menu, InputPlumber or compositor changes.
The rebuilt source directories must stay in place while these binaries are active.

## Add an ID later, without rebuilding

This edits a temporary file, validates it and installs it atomically. Replace the
ID to add a different game. Enabling an existing ID preserves its other fields.

```bash
bash <<'SH'
set -Eeuo pipefail
trap 'printf "ERROR at line %s (exit %s)\n" "$LINENO" "$?" >&2' ERR
catalog=/usr/local/share/zaman/catalog.json
sudo -v
candidate=$(mktemp)
trap 'rm -f -- "$candidate"' EXIT
python3 - "$catalog" "$candidate" org.opensurge2d.OpenSurge <<'PY'
import json, pathlib, sys
source, target, app = sys.argv[1:]
data = json.loads(pathlib.Path(source).read_text())
for entry in data['games']:
    if entry['id'] == app:
        entry['enabled'] = True
        break
else:
    data['games'].append({'id': app})
pathlib.Path(target).write_text(json.dumps(data, indent=2) + '\n')
PY
ZAMAN_CATALOG_PATH="$candidate" /usr/local/bin/zamanctl catalog validate
stamp=$(date +%Y%m%d-%H%M%S)
sudo cp -a -- "$catalog" "$catalog.before-$stamp"
sudo install -m 0644 "$candidate" "$catalog.new"
sudo mv -f -- "$catalog.new" "$catalog"
/usr/local/bin/zamanctl library sync
printf '\nPress X (or F5) in Dukkan to reload the game list.\n'
SH
```

Set `enabled` to false or remove the entry to withdraw a game, then validate and
publish in the same way. New entries must actually be available from Flathub for
aarch64/stable to enable Install. Catalog membership does not claim controller
compatibility; that is a separate qualification task.

## Acceptance

On the device, check Dukkan lists all five initial entries including Surge;
install/uninstall preserves artwork for other games; detail gallery and the
single live progress track still work; installed games launch and close through
Guide. Add a new ID and refresh without rebuild/restart. Disable it and sync,
checking that its Pegasus entry disappears while the package remains installed.
The daemon reads the file on each launch reservation, so later catalog edits
need no daemon restart. Keyboard/controller compatibility inside individual
Flatpaks remains outside this change.
