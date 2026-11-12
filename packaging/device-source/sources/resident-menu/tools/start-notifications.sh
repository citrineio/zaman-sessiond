#!/bin/sh
set -eu
zaman_notification_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
zaman_notification_environment=$(systemctl --user show-environment)
if [ -z "${XDG_RUNTIME_DIR:-}" ]; then XDG_RUNTIME_DIR=$(printf '%s\n' "$zaman_notification_environment" | sed -n 's/^XDG_RUNTIME_DIR=//p'); fi
if [ -z "${WAYLAND_DISPLAY:-}" ]; then WAYLAND_DISPLAY=$(printf '%s\n' "$zaman_notification_environment" | sed -n 's/^WAYLAND_DISPLAY=//p'); fi
if [ -z "${XDG_RUNTIME_DIR:-}" ] || [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo 'ERROR: graphical environment not imported; inspect systemctl --user show-environment.' >&2
    exit 1
fi
case "$WAYLAND_DISPLAY" in /*) zaman_notification_socket=$WAYLAND_DISPLAY;; *) zaman_notification_socket=$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY;; esac
if [ ! -S "$zaman_notification_socket" ]; then echo 'ERROR: Wayland socket is unavailable.' >&2; exit 1; fi
export XDG_RUNTIME_DIR WAYLAND_DISPLAY
export ZAMAN_NOTIFICATION_CLIENT="$zaman_notification_root/tools/zaman-notifications"
exec /usr/bin/python3 "$ZAMAN_NOTIFICATION_CLIENT" serve
