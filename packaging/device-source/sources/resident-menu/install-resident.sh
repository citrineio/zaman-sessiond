#!/bin/sh
# Run from the separately unpacked update directory as the graphical-session user.
set -eu
ZAMAN_UPDATE_SOURCE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ZAMAN_UPDATE_TARGET="$HOME/zaman-quickshell-prototype"
if [ "$(id -u)" = 0 ]; then
    echo "Run this as kadhem, without sudo; these are user services." >&2
    exit 1
fi
if [ "$ZAMAN_UPDATE_SOURCE" = "$ZAMAN_UPDATE_TARGET" ] || [ -L "$ZAMAN_UPDATE_TARGET" ]; then
    echo "Extract the archive into a separate update directory first; see RESIDENT-MENU.md." >&2
    exit 1
fi
(cd "$ZAMAN_UPDATE_SOURCE"; sha256sum -c SHA256SUMS)
test -d "$ZAMAN_UPDATE_TARGET"
test -x "$HOME/zaman-sessiond-publish/target/release/zaman-menu-dev-input"
if ! LC_ALL=C grep -a -Fq 'zaman-menu-helper-resident-v1' "$HOME/zaman-sessiond-publish/target/release/zaman-menu-dev-input"; then
    echo "Rebuild zaman-menu-dev-input from this archive first; see RESIDENT-MENU.md." >&2
    exit 1
fi
if [ "$("$HOME/zaman-sessiond-publish/target/release/zaman-menu-dev-input" --resident-version)" != zaman-menu-helper-resident-v1 ]; then
    echo "Rebuild zaman-menu-dev-input from this archive first; see RESIDENT-MENU.md." >&2
    exit 1
fi
/usr/bin/python3 -c 'import gi; from gi.repository import Gio, GLib; print("Gio ready")'
ZAMAN_UPDATE_BACKUP=$(mktemp -d "$HOME/zaman-resident-backup-$(date +%Y%m%d-%H%M%S)-XXXXXX")
mkdir -p "$ZAMAN_UPDATE_BACKUP/user-units"
for ZAMAN_UPDATE_UNIT in zaman-menu-shortcut.service zaman-quickshell.service; do
    systemctl --user is-enabled "$ZAMAN_UPDATE_UNIT" > "$ZAMAN_UPDATE_BACKUP/$ZAMAN_UPDATE_UNIT.enabled" 2>/dev/null || true
    if [ "$(systemctl --user show "$ZAMAN_UPDATE_UNIT" -p LoadState --value)" = loaded ]; then
        systemctl --user stop "$ZAMAN_UPDATE_UNIT"
    fi
    if [ -f "$HOME/.config/systemd/user/$ZAMAN_UPDATE_UNIT" ]; then
        cp -a "$HOME/.config/systemd/user/$ZAMAN_UPDATE_UNIT" "$ZAMAN_UPDATE_BACKUP/user-units/"
    fi
done
cp -a "$ZAMAN_UPDATE_TARGET" "$ZAMAN_UPDATE_BACKUP/prototype"
if [ -f "$HOME/.local/lib/zaman/zaman-menu-shortcut.py" ]; then
    cp -a "$HOME/.local/lib/zaman/zaman-menu-shortcut.py" "$ZAMAN_UPDATE_BACKUP/"
fi
printf 'Backup: %s\n' "$ZAMAN_UPDATE_BACKUP"
cp -a "$ZAMAN_UPDATE_SOURCE/." "$ZAMAN_UPDATE_TARGET/"
mkdir -p "$HOME/.config/systemd/user" "$HOME/.local/lib/zaman"
cp "$ZAMAN_UPDATE_TARGET/services/zaman-quickshell.service" "$HOME/.config/systemd/user/"
cp "$ZAMAN_UPDATE_TARGET/services/zaman-menu-shortcut.service" "$HOME/.config/systemd/user/"
cp "$ZAMAN_UPDATE_TARGET/shortcut/zaman-menu-shortcut.py" "$HOME/.local/lib/zaman/"
systemctl --user daemon-reload
systemctl --user enable --now zaman-quickshell.service
systemctl --user enable --now zaman-menu-shortcut.service
printf '%s\n' 'Resident menu enabled. Test Start + Select or F12, then B/Resume.'
