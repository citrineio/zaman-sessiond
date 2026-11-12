#!/usr/bin/env bash
# Run as the device owner AFTER both builds and their tests pass.
set -Eeuo pipefail
umask 077
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
dukkan=${1:-"$repo/../dukkan/build-shared-catalog/dukkan"}
dukkan=$(realpath -- "$dukkan")
config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
state_root=${XDG_STATE_HOME:-"$HOME/.local/state"}
binary="$repo/target/shared-catalog/release/zaman-sessiond"
cli="$repo/target/shared-catalog/release/zamanctl"
catalog=/usr/local/share/zaman/catalog.json
libexec=/usr/local/libexec/zaman
dropin="$config_root/systemd/user/zaman-sessiond.service.d/zz-shared-catalog.conf"
store_config="$config_root/zaman/storefronts.d/dukkan.toml"
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
trap 'printf "ERROR at line %s (exit %s)\n" "$LINENO" "$?" >&2' ERR
[[ $(id -u) != 0 ]] || fail 'Run as your normal user, not root.'
for path in "$repo" "$dukkan" "$config_root" "$state_root"; do
    [[ $path = /* && $path != *$'\n'* && $path != *'"'* && $path != *'%'* && $path != *'\'* ]] || fail "Unsupported path: $path"
done
for file in "$binary" "$cli" "$dukkan"; do [[ -x $file ]] || fail "Build missing: $file"; done
[[ -f $store_config && ! -L $store_config ]] || fail "Dukkan config missing or symlink: $store_config"
systemctl --user is-active --quiet zaman-sessiond.service || fail 'sessiond must be active before activation.'
for unit in zaman-game.service zaman-installed-game.service zaman-transfer.service \
    zaman-transfer-gui.service zaman-store-dukkan.service zaman-store-steam.service; do
    state=$(systemctl --user show "$unit" -p ActiveState --value)
    case "$state" in inactive|failed|'') ;; *) fail "Close $unit normally before activation (state: $state)." ;; esac
done
# Check the candidate CLI and the exact file that will be deployed before writing.
selected_catalog="$repo/catalog/games.json"
if [[ -e $catalog ]]; then selected_catalog=$catalog; fi
ZAMAN_CATALOG_PATH="$selected_catalog" "$cli" catalog validate
ZAMAN_CATALOG_PATH="$selected_catalog" python3 "$repo/tools/zaman_catalog.py" validate
python3 - "$store_config" <<'PY'
import sys, tomllib
with open(sys.argv[1], 'rb') as stream: data=tomllib.load(stream)
assert isinstance(data.get('argv'),list) and data['argv'], 'Invalid Dukkan argv'
assert set(data) <= {'argv','env','workdir','native_guide'}, 'Unexpected config field; preserve and review manually'
PY
printf '\n=== Back up active helpers, CLI and configuration ===\n'
sudo -v
mkdir -p "$state_root/zaman/shared-catalog-backups"
backup=$(mktemp -d "$state_root/zaman/shared-catalog-backups/before-catalog.XXXXXXXX")
targets=("$libexec/zaman_catalog.py" "$libexec/flatpak-game-session.py" "$libexec/pegasus-library-sync.py" \
    /usr/local/bin/zamanctl "$catalog" "$dropin" "$store_config")
for i in "${!targets[@]}"; do
    file=${targets[$i]}
    [[ ! -L $file ]] || fail "Refusing to replace a symlink: $file"
    if [[ -e $file ]]; then
        [[ -f $file ]] || fail "Not a regular file: $file"
        sudo cp -a -- "$file" "$backup/$i"
        touch "$backup/$i.present"
    fi
done
systemctl --user cat zaman-sessiond.service > "$backup/service-before.txt"
# A durable rollback command is produced before any live changes.
{
    printf '#!/usr/bin/env bash\nset -Eeuo pipefail\n'
    declare -p backup targets
    cat <<'RESTORE'
[[ $(id -u) != 0 ]] || { echo 'Run as the device owner, not root.' >&2; exit 1; }
for unit in zaman-game.service zaman-installed-game.service zaman-transfer.service zaman-transfer-gui.service zaman-store-dukkan.service zaman-store-steam.service; do
    state=$(systemctl --user show "$unit" -p ActiveState --value)
    case "$state" in inactive|failed|'') ;; *) echo "Close $unit before restoring." >&2; exit 1 ;; esac
done
sudo -v
systemctl --user stop zaman-sessiond.service
for i in "${!targets[@]}"; do
    file=${targets[$i]}
    if [[ -f $backup/$i.present ]]; then
        sudo cp -a -- "$backup/$i" "$file"
    else
        sudo rm -f -- "$file"
    fi
done
systemctl --user daemon-reload
systemctl --user restart zaman-sessiond.service
systemctl --user is-active --quiet zaman-sessiond.service
printf 'Restored configuration from %s\n' "$backup"
RESTORE
} > "$backup/restore.sh"
chmod 0700 "$backup/restore.sh"
printf 'Backup: %s\nRollback command: bash %q\n' "$backup" "$backup/restore.sh"
rollback() {
    result=$1
    trap - ERR INT TERM
    printf '\nActivation failed; restoring the backup.\n' >&2
    if ! bash "$backup/restore.sh"; then
        printf 'Automatic restore failed. Run: bash %q\n' "$backup/restore.sh" >&2
    fi
    exit "$result"
}
trap 'rollback "$?"' ERR
trap 'rollback 130' INT
trap 'rollback 143' TERM
printf '\n=== Deploy catalog and readers ===\n'
systemctl --user stop zaman-sessiond.service
sudo install -d -m 0755 "$libexec" /usr/local/share/zaman
# Preserve an existing valid catalog, including operator additions.
if [[ ! -e $catalog ]]; then sudo install -m 0644 "$repo/catalog/games.json" "$catalog"; fi
sudo install -m 0644 "$repo/tools/zaman_catalog.py" "$libexec/zaman_catalog.py"
sudo install -m 0755 "$repo/tools/flatpak-game-session.py" "$libexec/flatpak-game-session.py"
sudo install -m 0755 "$repo/tools/pegasus-library-sync.py" "$libexec/pegasus-library-sync.py"
sudo install -m 0755 "$cli" /usr/local/bin/zamanctl
mkdir -p -- "$(dirname -- "$dropin")"
cat > "$dropin" <<CONF
[Service]
ExecStart=
ExecStart="$binary"
Environment=ZAMAN_CATALOG_PATH=$catalog
Environment=ZAMAN_INSTALLED_GAME_HELPER=$libexec/flatpak-game-session.py
Environment=ZAMAN_PEGASUS_LIBRARY_SYNC_HELPER=$libexec/pegasus-library-sync.py
CONF
python3 - "$store_config" "$dukkan" "$catalog" "$libexec" <<'PY'
import json, os, pathlib, sys, tempfile, tomllib
path=pathlib.Path(sys.argv[1]); data=tomllib.loads(path.read_text())
data['argv'][0]=sys.argv[2]
env=data.setdefault('env',{}); env['ZAMAN_CATALOG_PATH']=sys.argv[3]; env['ZAMAN_CATALOG_LIBEXEC']=sys.argv[4]
# Keep existing environment, arguments and working directory.
lines=[f'{key} = {json.dumps(value,ensure_ascii=False)}' for key,value in data.items() if key!='env']
lines+=['','[env]']+[f'{json.dumps(key)} = {json.dumps(value,ensure_ascii=False)}' for key,value in env.items()]
text='\n'.join(lines)+'\n'; tomllib.loads(text)
fd,temp=tempfile.mkstemp(dir=path.parent,prefix='.dukkan-catalog-')
try:
    with os.fdopen(fd,'w') as stream: stream.write(text)
    os.chmod(temp,path.stat().st_mode & 0o777); os.replace(temp,path)
finally:
    if os.path.exists(temp): os.unlink(temp)
PY
printf '\n=== Activate and verify the running daemon ===\n'
systemctl --user daemon-reload
systemctl --user restart zaman-sessiond.service
systemctl --user is-active --quiet zaman-sessiond.service
pid=$(systemctl --user show zaman-sessiond.service -p MainPID --value)
actual=$(readlink -f -- "/proc/$pid/exe")
[[ $actual = "$binary" ]] || { printf 'Unexpected running binary: %s\n' "$actual" >&2; false; }
ZAMAN_CATALOG_PATH="$catalog" /usr/local/bin/zamanctl catalog validate
ZAMAN_CATALOG_PATH="$catalog" /usr/local/bin/zamanctl library sync
trap - ERR INT TERM
printf '\n=== Shared catalog activated ===\nDaemon: %s\nDukkan: %s\nCatalog: %s\nRollback: bash %q\n' "$actual" "$dukkan" "$catalog" "$backup/restore.sh"
