#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
DAEMON="$REPO_DIR/target/debug/zaman-sessiond"
CTL="$REPO_DIR/target/debug/zamanctl"
MENU="$REPO_DIR/target/debug/zaman-menu"
SERVICE="com.kawnelectro.Zaman.Session1"
UNIT="zaman-game.service"

cd "$REPO_DIR"

echo "[1/5] Building Guide validation binaries"
if [ "${ZAMAN_SKIP_BUILD:-0}" = "1" ]; then
    echo "Using binaries already qualified by the smoke test"
else
    cargo fmt
    git diff --check
    cargo test --locked
    cargo build --locked --bins
fi

echo "[2/5] Checking validation preconditions"
if busctl --user --no-pager --no-legend list |
    awk -v service="$SERVICE" '$1 == service { found=1 } END { exit !found }'; then
    echo "FAIL: $SERVICE is already owned"
    exit 1
fi
if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT is already active; refusing to disturb it"
    exit 1
fi
if ! systemctl is-active --quiet inputplumber.service; then
    echo "FAIL: inputplumber.service is not active"
    exit 1
fi

USER_ENVIRONMENT=$(systemctl --user show-environment)
SESSION_WAYLAND_DISPLAY=$(
    printf '%s\n' "$USER_ENVIRONMENT" |
        sed -n 's/^WAYLAND_DISPLAY=//p' |
        tail -n 1
)
SESSION_XDG_RUNTIME_DIR=$(
    printf '%s\n' "$USER_ENVIRONMENT" |
        sed -n 's/^XDG_RUNTIME_DIR=//p' |
        tail -n 1
)
if [ -z "$SESSION_WAYLAND_DISPLAY" ] || [ -z "$SESSION_XDG_RUNTIME_DIR" ]; then
    echo "FAIL: the user manager has no active Zaman Wayland session"
    exit 1
fi
if [ ! -S "$SESSION_XDG_RUNTIME_DIR/$SESSION_WAYLAND_DISPLAY" ]; then
    echo "FAIL: the imported Wayland socket does not exist"
    exit 1
fi

TEST_ROOT=$(mktemp -d /tmp/zaman-guide-validation.XXXXXX)
DAEMON_LOG="$TEST_ROOT/daemon.log"
CLIENT_LOG="$TEST_ROOT/client.log"
MENU_LOG="$TEST_ROOT/menu.log"
ROM_DIR="$TEST_ROOT/roms"
DAEMON_PID=""
CLIENT_PID=""
MENU_PID=""
OWNS_TEST_UNIT=0

cleanup() {
    if [ -n "$CLIENT_PID" ] && kill -0 "$CLIENT_PID" 2>/dev/null; then
        "$CTL" stop >/dev/null 2>&1 || true
        wait "$CLIENT_PID" 2>/dev/null || true
    fi
    if [ -n "$MENU_PID" ] && kill -0 "$MENU_PID" 2>/dev/null; then
        kill -TERM "$MENU_PID" 2>/dev/null || true
        wait "$MENU_PID" 2>/dev/null || true
    fi
    if [ -n "$DAEMON_PID" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
        kill -TERM "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    if [ "$OWNS_TEST_UNIT" -eq 1 ] && systemctl --user is-active --quiet "$UNIT"; then
        systemctl --user stop "$UNIT" || true
    fi
    rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$TEST_ROOT/systems.d" "$TEST_ROOT/emulators.d" "$ROM_DIR"
printf 'Guide validation ROM\n' >"$ROM_DIR/Guide Validation.guide"

cat >"$TEST_ROOT/systems.d/guide.toml" <<'EOF'
schema = 1
[system]
id = "guide"
name = "Guide validation system"
extensions = ["guide"]
EOF

cat >"$TEST_ROOT/emulators.d/guide.toml" <<'EOF'
schema = 1
[emulator]
id = "guide-emulator"
name = "Guide validation emulator"
version = "1"
systems = ["guide"]
priority = 100
[emulator.exec]
argv = ["/usr/bin/sleep", "infinity"]
EOF

echo "[3/5] Starting isolated daemon and supervised test game"
ZAMAN_REGISTRY_DIRS="$TEST_ROOT" "$DAEMON" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

attempt=0
SERVICE_READY=0
while [ "$attempt" -lt 100 ]; do
    if busctl --user --no-pager --no-legend list |
        awk -v service="$SERVICE" '$1 == service { found=1 } END { exit !found }'; then
        SERVICE_READY=1
        break
    fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
        echo "FAIL: zaman-sessiond exited during startup"
        cat "$DAEMON_LOG"
        exit 1
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

if [ "$SERVICE_READY" -ne 1 ]; then
    echo "FAIL: $SERVICE did not become ready"
    cat "$DAEMON_LOG"
    exit 1
fi

WAYLAND_DISPLAY="$SESSION_WAYLAND_DISPLAY" \
XDG_RUNTIME_DIR="$SESSION_XDG_RUNTIME_DIR" \
XDG_SESSION_TYPE=wayland \
DBUS_SESSION_BUS_ADDRESS="unix:path=$SESSION_XDG_RUNTIME_DIR/bus" \
SDL_VIDEODRIVER=wayland \
    "$MENU" >"$MENU_LOG" 2>&1 &
MENU_PID=$!

attempt=0
while [ "$attempt" -lt 100 ]; do
    if grep -Fq "zaman-menu ready" "$MENU_LOG"; then
        break
    fi
    if ! kill -0 "$MENU_PID" 2>/dev/null; then
        echo "FAIL: zaman-menu exited during startup"
        cat "$MENU_LOG"
        exit 1
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done
if ! grep -Fq "zaman-menu ready" "$MENU_LOG"; then
    echo "FAIL: zaman-menu did not become ready"
    cat "$MENU_LOG"
    exit 1
fi

"$CTL" launch guide "$ROM_DIR/Guide Validation.guide" >"$CLIENT_LOG" 2>&1 &
CLIENT_PID=$!
OWNS_TEST_UNIT=1

attempt=0
while [ "$attempt" -lt 100 ]; do
    if systemctl --user is-active --quiet "$UNIT"; then
        break
    fi
    if ! kill -0 "$CLIENT_PID" 2>/dev/null; then
        echo "FAIL: launch client exited before $UNIT became active"
        cat "$CLIENT_LOG"
        cat "$DAEMON_LOG"
        exit 1
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

if ! systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT did not become active"
    cat "$DAEMON_LOG"
    exit 1
fi
if grep -Fq "Discovered 0 composite device(s)" "$DAEMON_LOG"; then
    echo "FAIL: no controller was discovered; connect one and rerun"
    cat "$DAEMON_LOG"
    exit 1
fi

wait_for_new_log() {
    pattern=$1
    first_line=$2
    attempt=0
    while [ "$attempt" -lt 600 ]; do
        if tail -n +"$first_line" "$DAEMON_LOG" | grep -Fq "$pattern"; then
            return 0
        fi
        if ! systemctl --user is-active --quiet "$UNIT"; then
            echo "FAIL: game unit exited during Guide validation"
            return 1
        fi
        attempt=$((attempt + 1))
        sleep 0.1
    done
    return 1
}

wait_for_new_menu_log() {
    pattern=$1
    first_line=$2
    attempt=0
    while [ "$attempt" -lt 600 ]; do
        if tail -n +"$first_line" "$MENU_LOG" | grep -Fq "$pattern"; then
            return 0
        fi
        if ! kill -0 "$MENU_PID" 2>/dev/null; then
            echo "FAIL: zaman-menu exited during validation"
            return 1
        fi
        if ! systemctl --user is-active --quiet "$UNIT"; then
            echo "FAIL: game unit exited during menu validation"
            return 1
        fi
        attempt=$((attempt + 1))
        sleep 0.1
    done
    return 1
}

echo "[4/5] Press and release Guide once"
first_line=$(( $(wc -l <"$DAEMON_LOG") + 1 ))
menu_first_line=$(( $(wc -l <"$MENU_LOG") + 1 ))
if ! wait_for_new_log "Guide press requested the system menu" "$first_line"; then
    echo "FAIL: Guide press did not request the system menu"
    cat "$DAEMON_LOG"
    exit 1
fi
echo "MENU REQUEST RECEIVED"
if ! wait_for_new_menu_log "Opening menu generation=" "$menu_first_line"; then
    echo "FAIL: zaman-menu did not create its full-screen surface"
    cat "$MENU_LOG"
    exit 1
fi
if ! wait_for_new_log "menu lifecycle retains input ownership" "$first_line"; then
    echo "FAIL: Guide release was not observed"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! "$CTL" menu-status | grep -Fq "menu=open"; then
    echo "FAIL: D-Bus menu state did not become open"
    "$CTL" menu-status
    exit 1
fi
if ! tail -n +"$first_line" "$DAEMON_LOG" | grep -Fq "InterceptMode to 2"; then
    echo "FAIL: system menu did not retain InputPlumber ALL mode"
    cat "$DAEMON_LOG"
    exit 1
fi

echo "Resume is selected. Confirm that the menu is visible, then press the controller's primary accept button once."
if ! wait_for_new_menu_log "Activating Resume" "$menu_first_line"; then
    echo "FAIL: zaman-menu did not activate Resume"
    cat "$MENU_LOG"
    exit 1
fi
if ! wait_for_new_log "System menu resumed the game" "$first_line"; then
    echo "FAIL: Resume was not observed"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! "$CTL" menu-status | grep -Fq "menu=closed"; then
    echo "FAIL: D-Bus menu state did not close after Resume"
    "$CTL" menu-status
    exit 1
fi
if ! systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: Resume stopped the game"
    exit 1
fi
echo "PASS: Guide opened the menu; Resume restored gameplay"

echo "[5/5] Press and release Guide once"
first_line=$(( $(wc -l <"$DAEMON_LOG") + 1 ))
menu_first_line=$(( $(wc -l <"$MENU_LOG") + 1 ))
if ! wait_for_new_log "Guide press requested the system menu" "$first_line"; then
    echo "FAIL: second Guide press did not request the system menu"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! wait_for_new_log "menu lifecycle retains input ownership" "$first_line"; then
    echo "FAIL: second Guide release was not observed"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! "$CTL" menu-status | grep -Fq "menu=open"; then
    echo "FAIL: menu did not reopen"
    "$CTL" menu-status
    exit 1
fi

if ! wait_for_new_menu_log "Opening menu generation=" "$menu_first_line"; then
    echo "FAIL: zaman-menu did not reopen its full-screen surface"
    cat "$MENU_LOG"
    exit 1
fi
echo "Press Down once to select Exit Game, then press the controller's primary accept button once."
if ! wait_for_new_menu_log "Activating ExitGame" "$menu_first_line"; then
    echo "FAIL: zaman-menu did not activate Exit Game"
    cat "$MENU_LOG"
    exit 1
fi
wait "$CLIENT_PID"
CLIENT_PID=""

if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT remained active after ExitGame"
    exit 1
fi
if ! "$CTL" status | grep -Fq "result=exit requested from the system menu"; then
    echo "FAIL: ExitGame result was not recorded"
    "$CTL" status
    exit 1
fi
if ! "$CTL" menu-status | grep -Fq "menu=closed"; then
    echo "FAIL: menu remained open after ExitGame"
    "$CTL" menu-status
    exit 1
fi
echo "PASS: menu-selected ExitGame stopped the supervised test game"

kill -TERM "$MENU_PID"
wait "$MENU_PID" 2>/dev/null || true
MENU_PID=""
kill -TERM "$DAEMON_PID"
wait "$DAEMON_PID"
DAEMON_PID=""
OWNS_TEST_UNIT=0

cat "$MENU_LOG"
cat "$DAEMON_LOG"
echo "PASS: full-screen Guide menu validation completed"
