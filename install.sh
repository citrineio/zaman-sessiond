#!/usr/bin/env bash
# Install built 0.6.7 binaries on the Zaman kiosk user's machine.
set -Eeuo pipefail
umask 077

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
[[ $(id -u) -ne 0 ]] || die 'Run as the logged-in kiosk user, not root.'
[[ -n ${XDG_RUNTIME_DIR:-} ]] || die 'XDG_RUNTIME_DIR is missing; run from the kiosk user session.'
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd -- "$script_dir"

names=(zaman-sessiond zaman-menu zamanctl)
sources=(target/release/zaman-sessiond target/release/zaman-menu target/release/zamanctl)
targets=(/usr/libexec/zaman-sessiond /usr/libexec/zaman-menu /usr/local/bin/zamanctl)
units=(zaman-sessiond.service zaman-menu.service)

for source in "${sources[@]}"; do
    [[ -f $source && -x $source ]] || die "Build missing: $source"
done
for target in "${targets[@]}"; do
    [[ -f $target ]] || die "Installed baseline missing: $target"
done
for unit in zaman-game.service zaman-transfer.service zaman-transfer-gui.service; do
    state=$(systemctl --user show "$unit" -p ActiveState --value) || die "Cannot inspect $unit"
    case $state in inactive|failed|'') ;; *) die "Stop $unit first (state: $state)." ;; esac
done
if pgrep -f '^(/usr/bin/)?python3 .*zaman_transfer\.py([[:space:]]|$)' >/dev/null; then
    die 'Stop the command-line Zaman transfer session first.'
fi
previous_version=$(/usr/local/bin/zamanctl version)
[[ $previous_version == 0.6.6 || $previous_version == 0.6.7 ]] || die "Expected installed version 0.6.6 or 0.6.7, found $previous_version."
for unit in "${units[@]}"; do
    systemctl --user is-active --quiet "$unit" || die "$unit is not active; inspect it before installing."
done

# Obtain credentials and finish the backup before stopping services.
sudo -v
backup_root=${XDG_STATE_HOME:-"$HOME/.local/state"}/zaman-sessiond/backups
mkdir -p -- "$backup_root"
backup=$(mktemp -d "$backup_root/$previous_version-before-0.6.7.XXXXXXXX")
for i in "${!names[@]}"; do
    sudo cp -a -- "${targets[i]}" "$backup/${names[i]}"
done
printf '%s\n' "$previous_version" > "$backup/previous-version"
printf 'Backup: %s\n' "$backup"

changed=0
rollback() {
    local rc=$?
    trap - EXIT INT TERM
    if (( changed )); then
        printf 'Install failed (status %s); restoring binaries from %s\n' "$rc" "$backup" >&2
        systemctl --user stop zaman-menu.service zaman-sessiond.service || true
        for i in "${!names[@]}"; do
            sudo install -m 0755 -- "$backup/${names[i]}" "${targets[i]}" || true
        done
        systemctl --user start zaman-sessiond.service zaman-menu.service || true
    fi
    exit "$rc"
}
trap rollback EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

changed=1
systemctl --user stop zaman-menu.service zaman-sessiond.service
for i in "${!names[@]}"; do
    sudo install -m 0755 -- "${sources[i]}" "${targets[i]}"
done
systemctl --user start zaman-sessiond.service zaman-menu.service
for unit in "${units[@]}"; do systemctl --user is-active --quiet "$unit"; done
installed_version=$(/usr/local/bin/zamanctl version)
[[ $installed_version == 0.6.7 ]] || die "Version check failed: $installed_version"
trap - EXIT INT TERM
printf 'Installed Zaman Sessiond %s. Backup: %s\n' "$installed_version" "$backup"
