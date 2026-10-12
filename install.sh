#!/usr/bin/env bash
# Install built 0.6.12 binaries on the Zaman kiosk user's machine.
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
input_policy_names=(zaman-inputplumber-policy 50-zaman-management.conf)
input_policy_sources=(dist/libexec/zaman-inputplumber-policy dist/systemd/system/inputplumber.service.d/50-zaman-management.conf)
input_policy_targets=(/usr/libexec/zaman-inputplumber-policy /etc/systemd/system/inputplumber.service.d/50-zaman-management.conf)
input_policy_modes=(0755 0644)
input_policy_existed=(0 0)

for source in "${input_policy_sources[@]}"; do
    [[ -f $source ]] || die "Input policy missing: $source"
done
systemctl is-active --quiet inputplumber.service || die 'inputplumber.service is not active; inspect it before installing.'

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
[[ $previous_version == 0.6.7 || $previous_version == 0.6.8 || $previous_version == 0.6.9 || $previous_version == 0.6.10 || $previous_version == 0.6.11 || $previous_version == 0.6.12 ]] || die "Expected installed version 0.6.7 through 0.6.12, found $previous_version."
for unit in "${units[@]}"; do
    systemctl --user is-active --quiet "$unit" || die "$unit is not active; inspect it before installing."
done

# Obtain credentials and finish the backup before stopping services.
sudo -v
input_management_reply=$(sudo busctl --system --timeout=2s get-property \
    org.shadowblip.InputPlumber /org/shadowblip/InputPlumber/Manager \
    org.shadowblip.InputManager ManageAllDevices)
case $input_management_reply in
    'b true') input_previous_management=true ;;
    'b false') input_previous_management=false ;;
    *) die "Unexpected InputPlumber management readback: $input_management_reply" ;;
esac
backup_root=${XDG_STATE_HOME:-"$HOME/.local/state"}/zaman-sessiond/backups
mkdir -p -- "$backup_root"
backup=$(mktemp -d "$backup_root/$previous_version-before-0.6.12.XXXXXXXX")
for i in "${!names[@]}"; do
    sudo cp -a -- "${targets[i]}" "$backup/${names[i]}"
done
for i in "${!input_policy_names[@]}"; do
    if sudo test -e "${input_policy_targets[i]}" || sudo test -L "${input_policy_targets[i]}"; then
        input_policy_existed[i]=1
        sudo cp -a -- "${input_policy_targets[i]}" "$backup/${input_policy_names[i]}"
    fi
done
printf '%s\n' "${input_policy_existed[@]}" > "$backup/input-policy-existed"
printf '%s\n' "$input_previous_management" > "$backup/input-management"
printf '%s\n' "$previous_version" > "$backup/previous-version"
printf 'Backup: %s\n' "$backup"

changed=0
input_policy_changed=0
rollback() {
    local rc=$?
    trap - EXIT INT TERM
    if (( changed )); then
        printf 'Install failed (status %s); restoring binaries from %s\n' "$rc" "$backup" >&2
        systemctl --user stop zaman-menu.service zaman-sessiond.service || true
        for i in "${!names[@]}"; do
            sudo install -m 0755 -- "$backup/${names[i]}" "${targets[i]}" || true
        done
        if (( input_policy_changed )); then
            for i in "${!input_policy_names[@]}"; do
                if (( input_policy_existed[i] )); then
                    sudo cp -a -- "$backup/${input_policy_names[i]}" "${input_policy_targets[i]}" || true
                else
                    sudo rm -f -- "${input_policy_targets[i]}" || true
                fi
            done
            sudo systemctl daemon-reload || true
            sudo systemctl restart inputplumber.service || true
            sudo /bin/sh "$script_dir/${input_policy_sources[0]}" "$input_previous_management" || true
        fi
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
input_policy_changed=1
for i in "${!input_policy_names[@]}"; do
    sudo install -D -m "${input_policy_modes[i]}" -- "${input_policy_sources[i]}" "${input_policy_targets[i]}"
done
sudo systemctl daemon-reload
sudo systemctl restart inputplumber.service
systemctl is-active --quiet inputplumber.service || die 'InputPlumber did not restart successfully.'
systemctl --user start zaman-sessiond.service zaman-menu.service
for unit in "${units[@]}"; do systemctl --user is-active --quiet "$unit"; done
installed_version=$(/usr/local/bin/zamanctl version)
[[ $installed_version == 0.6.12 ]] || die "Version check failed: $installed_version"
trap - EXIT INT TERM
printf 'Installed Zaman Sessiond %s. Backup: %s\n' "$installed_version" "$backup"
