#!/bin/sh
set -eu
ZAMAN_PROJECT_DIR=${1:-"$HOME/zaman-sessiond-publish"}
ZAMAN_MENU_MODE=${2:-open}
case "$ZAMAN_MENU_MODE" in
    --resident|open|close|toggle|status|timings) ;;
    *) echo "Usage: sh run.sh [project-directory] [--resident|open|close|toggle|status|timings]" >&2; exit 2 ;;
esac
ZAMAN_PROTO_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ZAMAN_MENU_DEV_INPUT="$ZAMAN_PROJECT_DIR/target/release/zaman-menu-dev-input"
if [ ! -x "$ZAMAN_MENU_DEV_INPUT" ]; then
    echo "Build the development helper first: cargo build --release --locked --offline --bin zaman-menu-dev-input" >&2
    exit 1
fi
ZAMAN_GRAPHICAL_ENV=$(systemctl --user show-environment | sed -n -e '/^SWAYSOCK=/p' -e '/^WAYLAND_DISPLAY=/p' -e '/^XDG_RUNTIME_DIR=/p')
if [ -z "${SWAYSOCK:-}" ] || [ ! -S "$SWAYSOCK" ]; then SWAYSOCK=$(printf '%s\n' "$ZAMAN_GRAPHICAL_ENV" | sed -n 's/^SWAYSOCK=//p'); fi
if [ -z "${WAYLAND_DISPLAY:-}" ]; then WAYLAND_DISPLAY=$(printf '%s\n' "$ZAMAN_GRAPHICAL_ENV" | sed -n 's/^WAYLAND_DISPLAY=//p'); fi
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then XDG_RUNTIME_DIR=$(printf '%s\n' "$ZAMAN_GRAPHICAL_ENV" | sed -n 's/^XDG_RUNTIME_DIR=//p'); fi
if [ -z "${SWAYSOCK:-}" ] || [ -z "${WAYLAND_DISPLAY:-}" ] || [ -z "${XDG_RUNTIME_DIR:-}" ]; then
    echo "The graphical-session environment is incomplete; run from the Sway session or inspect systemctl --user show-environment." >&2
    exit 1
fi
if [ ! -S "$SWAYSOCK" ]; then
    echo "The Sway socket is missing: $SWAYSOCK. Check the running graphical session." >&2
    exit 1
fi
ZAMAN_MENU_DEV_PAIR="$ZAMAN_PROJECT_DIR/target/release/zaman-menu-dev-pair"
if [ ! -x "$ZAMAN_MENU_DEV_PAIR" ]; then ZAMAN_MENU_DEV_PAIR=; fi
export SWAYSOCK WAYLAND_DISPLAY XDG_RUNTIME_DIR ZAMAN_MENU_DEV_INPUT ZAMAN_MENU_DEV_PAIR
export DUKKAN_TRANSACTION_CLIENT="${DUKKAN_TRANSACTION_CLIENT:-$HOME/projects/zaman-sprint5/dukkan/tools/transactions.py}"
export ZAMAN_NOTIFICATION_CLIENT="$ZAMAN_PROTO_DIR/tools/zaman-notifications"
export ZAMAN_MENU_SESSION_BRIDGE="$ZAMAN_PROTO_DIR/actions/zaman-menu-session.py"
# Load the project's existing fonts privately in QML; no font installation.
ZAMAN_MENU_DEV_FONT="$ZAMAN_PROJECT_DIR/assets/fonts/saira-condensed/SairaCondensed-Regular.ttf"
ZAMAN_MENU_DEV_MONO="$ZAMAN_PROJECT_DIR/assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"
if [ ! -r "$ZAMAN_MENU_DEV_FONT" ]; then ZAMAN_MENU_DEV_FONT=; fi
if [ ! -r "$ZAMAN_MENU_DEV_MONO" ]; then ZAMAN_MENU_DEV_MONO=; fi
export ZAMAN_MENU_DEV_FONT ZAMAN_MENU_DEV_MONO
export QT_QPA_PLATFORM=wayland
if [ "$ZAMAN_MENU_MODE" = --resident ]; then
    if ! LC_ALL=C grep -a -Fq 'zaman-menu-helper-resident-v1' "$ZAMAN_MENU_DEV_INPUT"; then
        echo "Rebuild zaman-menu-dev-input for the resident helper protocol; see RESIDENT-MENU.md." >&2
        exit 1
    fi
    # Startup is hidden; the persistent helper preloads read-only inventory.
    export ZAMAN_MENU_DEV_OPEN_ON_START=0
    exec qs --no-duplicate -p "$ZAMAN_PROTO_DIR/qml/shell.qml"
fi
if [ "$ZAMAN_MENU_MODE" != open ]; then
    exec qs -p "$ZAMAN_PROTO_DIR/qml/shell.qml" ipc call menu "$ZAMAN_MENU_MODE"
fi
# Normal openings contact the warm renderer. Start its service only if absent.
if qs -p "$ZAMAN_PROTO_DIR/qml/shell.qml" ipc call menu open 2>/dev/null; then
    exit 0
fi
systemctl --user start zaman-quickshell.service
ZAMAN_MENU_ATTEMPT=0
while [ "$ZAMAN_MENU_ATTEMPT" -lt 80 ]; do
    if qs -p "$ZAMAN_PROTO_DIR/qml/shell.qml" ipc call menu open 2>/dev/null; then
        exit 0
    fi
    ZAMAN_MENU_ATTEMPT=$((ZAMAN_MENU_ATTEMPT + 1))
    sleep 0.1
done
echo "The resident menu did not answer IPC; inspect journalctl --user -u zaman-quickshell.service." >&2
exit 1
