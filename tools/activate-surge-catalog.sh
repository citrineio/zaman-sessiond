#!/usr/bin/env bash
# Owner-run activation of the Surge catalog addition; no package installation.
set -Eeuo pipefail
umask 077

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary="$repo/target/release/zaman-sessiond"
config_root="${XDG_CONFIG_HOME:-$HOME/.config}"
state_root="${XDG_STATE_HOME:-$HOME/.local/state}"
dropin="$config_root/systemd/user/zaman-sessiond.service.d/99-sprint35-surge.conf"
installed_game=/usr/local/libexec/zaman/flatpak-game-session.py
installed_library=/usr/local/libexec/zaman/pegasus-library-sync.py

fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
[[ $(id -u) != 0 ]] || fail 'Run as kadhem, not root.'
[[ -x $binary ]] || fail "Build missing: $binary"
for file in "$repo/tools/flatpak-game-session.py" "$repo/tools/pegasus-library-sync.py"; do
    [[ -f $file ]] || fail "Source helper missing: $file"
done
systemctl --user is-active --quiet zaman-sessiond.service || fail 'sessiond must be active before this update.'
for unit in zaman-game.service zaman-installed-game.service zaman-transfer.service \
            zaman-transfer-gui.service zaman-store-dukkan.service zaman-store-steam.service; do
    state=$(systemctl --user show "$unit" -p ActiveState --value)
    case "$state" in
        inactive|failed|'') ;;
        *) fail "Close $unit normally before activation (state: $state)." ;;
    esac
done
for file in "$installed_game" "$installed_library"; do
    [[ -f $file ]] || fail "Installed baseline missing: $file"
done

printf '\n=== Back up active configuration and helpers ===\n'
sudo -v
mkdir -p "$state_root/zaman/surge-catalog-backups"
backup=$(mktemp -d "$state_root/zaman/surge-catalog-backups/before-surge.XXXXXXXX")
sudo cp -a -- "$installed_game" "$backup/flatpak-game-session.py"
sudo cp -a -- "$installed_library" "$backup/pegasus-library-sync.py"
previous_dropin=0
if [[ -e $dropin || -L $dropin ]]; then
    cp -a -- "$dropin" "$backup/99-sprint35-surge.conf"
    previous_dropin=1
fi
systemctl --user cat zaman-sessiond.service > "$backup/service-before.txt"
printf 'Backup: %s\n' "$backup"

rollback() {
    result=${1:-$?}
    trap - ERR INT TERM
    printf '\nActivation failed; restoring previous configuration from %s\n' "$backup" >&2
    sudo install -m 0755 "$backup/flatpak-game-session.py" "$installed_game" || true
    sudo install -m 0755 "$backup/pegasus-library-sync.py" "$installed_library" || true
    if (( previous_dropin )); then
        cp -a -- "$backup/99-sprint35-surge.conf" "$dropin" || true
    else
        rm -f -- "$dropin"
    fi
    systemctl --user daemon-reload || true
    systemctl --user restart zaman-sessiond.service || true
    exit "$result"
}
trap 'rollback "$?"' ERR
trap 'rollback 130' INT
trap 'rollback 143' TERM

printf '\n=== Deploy matching catalog helpers ===\n'
sudo install -m 0755 "$repo/tools/flatpak-game-session.py" "$installed_game"
sudo install -m 0755 "$repo/tools/pegasus-library-sync.py" "$installed_library"
mkdir -p -- "$(dirname -- "$dropin")"
cat > "$dropin" <<CONF
[Service]
ExecStart=
ExecStart=$binary
Environment=ZAMAN_INSTALLED_GAME_HELPER=$installed_game
Environment=ZAMAN_PEGASUS_LIBRARY_SYNC_HELPER=$installed_library
CONF

printf '\n=== Activate rebuilt daemon ===\n'
systemctl --user daemon-reload
systemctl --user restart zaman-sessiond.service
systemctl --user is-active --quiet zaman-sessiond.service
pid=$(systemctl --user show zaman-sessiond.service -p MainPID --value)
actual=$(readlink -f -- "/proc/$pid/exe")
if [[ $actual != "$binary" ]]; then
    printf 'Unexpected running binary: %s\n' "$actual" >&2
    false # Enter the rollback trap.
fi
zamanctl library sync
trap - ERR INT TERM
printf '\nSurge catalog activated. Backup: %s\nDaemon: %s\n' "$backup" "$actual"
printf 'Reopen the rebuilt Dukkan, install Surge Engine, then test Play/Guide/Exit.\n'
