#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
DAEMON="$REPO_DIR/target/debug/zaman-sessiond"
CTL="$REPO_DIR/target/debug/zamanctl"
SERVICE="com.kawnelectro.Zaman.Session1"
UNIT="zaman-game.service"

cd "$REPO_DIR"

echo "[1/4] Building Guide validation binaries"
cargo fmt
git diff --check
cargo test
cargo build --bins

echo "[2/4] Checking validation preconditions"
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

TEST_ROOT=$(mktemp -d /tmp/zaman-guide-validation.XXXXXX)
DAEMON_LOG="$TEST_ROOT/daemon.log"
CLIENT_LOG="$TEST_ROOT/client.log"
ROM_DIR="$TEST_ROOT/roms"
DAEMON_PID=""
CLIENT_PID=""
OWNS_TEST_UNIT=0

cleanup() {
    if [ -n "$CLIENT_PID" ] && kill -0 "$CLIENT_PID" 2>/dev/null; then
        "$CTL" stop >/dev/null 2>&1 || true
        wait "$CLIENT_PID" 2>/dev/null || true
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

echo "[3/4] Starting isolated daemon and supervised test game"
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

echo "[4/4] Press and release Guide once"
first_line=$(( $(wc -l <"$DAEMON_LOG") + 1 ))
if ! wait_for_new_log "Guide press requested the system menu" "$first_line"; then
    echo "FAIL: Guide press did not request the system menu"
    cat "$DAEMON_LOG"
    exit 1
fi
echo "MENU REQUEST RECEIVED"
if ! wait_for_new_log "Guide released" "$first_line"; then
    echo "FAIL: Guide release was not observed"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: Guide press stopped the game"
    exit 1
fi
echo "PASS: Guide press requested the menu and game remained active"

"$CTL" stop
wait "$CLIENT_PID"
CLIENT_PID=""

if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT remained active after validation cleanup"
    exit 1
fi

kill -TERM "$DAEMON_PID"
wait "$DAEMON_PID"
DAEMON_PID=""
OWNS_TEST_UNIT=0

cat "$DAEMON_LOG"
echo "PASS: Guide press validation completed"
