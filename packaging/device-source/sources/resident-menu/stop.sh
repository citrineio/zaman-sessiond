#!/bin/sh
# Called while the renderer is alive, before systemd sends it SIGTERM.
set -eu
ZAMAN_STOP_PROJECT=${1:-"$HOME/zaman-sessiond-publish"}
ZAMAN_STOP_PROTO=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if ! sh "$ZAMAN_STOP_PROTO/run.sh" "$ZAMAN_STOP_PROJECT" close; then
    # No IPC peer remains. There is no live renderer to ask to close.
    exit 0
fi
while :; do
    ZAMAN_STOP_STATUS=$(sh "$ZAMAN_STOP_PROTO/run.sh" "$ZAMAN_STOP_PROJECT" status) || exit 0
    if [ "$ZAMAN_STOP_STATUS" = hidden ]; then exit 0; fi
    sleep 0.1
done
