#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
DAEMON="$REPO_DIR/target/debug/zaman-sessiond"
CTL="$REPO_DIR/target/debug/zamanctl"
SERVICE="com.kawnelectro.Zaman.Session1"
PATH_OBJECT="/com/kawnelectro/Zaman/Session1"
INTERFACE="com.kawnelectro.Zaman.Session1"
UNIT="zaman-game.service"

cd "$REPO_DIR"

echo "[1/8] Formatting and compiling"
cargo fmt
git diff --check
cargo check --all-targets
cargo test
cargo build --bins

echo "[2/8] Checking service preconditions"
if busctl --user --no-pager --no-legend list | awk -v service="$SERVICE" '$1 == service { found=1 } END { exit !found }'; then
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

TEST_ROOT=$(mktemp -d /tmp/zaman-sessiond-v02.XXXXXX)
DAEMON_LOG="$TEST_ROOT/daemon.log"
FAST_LOG="$TEST_ROOT/fast.log"
SLOW_LOG="$TEST_ROOT/slow.log"
ROM_DIR="$TEST_ROOT/roms"
DAEMON_PID=""
CLIENT_PID=""
OWNS_TEST_UNIT=0

cleanup() {
    if [ -n "$CLIENT_PID" ] && kill -0 "$CLIENT_PID" 2>/dev/null; then
        kill -TERM "$CLIENT_PID" 2>/dev/null || true
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
printf 'test ROM\n' >"$ROM_DIR/Fast Game.nes"
printf 'test ROM\n' >"$ROM_DIR/Slow Game.slow"

cat >"$TEST_ROOT/systems.d/fast.toml" <<'EOF'
schema = 1
[system]
id = "fast"
name = "Fast smoke system"
extensions = ["nes"]
EOF

cat >"$TEST_ROOT/systems.d/slow.toml" <<'EOF'
schema = 1
[system]
id = "slow"
name = "Slow smoke system"
extensions = ["slow"]
EOF

cat >"$TEST_ROOT/emulators.d/fast.toml" <<'EOF'
schema = 1
[emulator]
id = "fast-emulator"
name = "Fast smoke emulator"
version = "1"
systems = ["fast"]
priority = 100
[emulator.exec]
argv = ["/usr/bin/true", "{rom.path}"]
EOF

cat >"$TEST_ROOT/emulators.d/slow.toml" <<'EOF'
schema = 1
[emulator]
id = "slow-emulator"
name = "Slow smoke emulator"
version = "1"
systems = ["slow"]
priority = 100
[emulator.exec]
argv = ["/usr/bin/sleep", "infinity"]
EOF

echo "[3/8] Starting isolated D-Bus daemon"
ZAMAN_REGISTRY_DIRS="$TEST_ROOT" "$DAEMON" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

attempt=0
SERVICE_READY=0
while [ "$attempt" -lt 100 ]; do
    if busctl --user --no-pager --no-legend list | awk -v service="$SERVICE" '$1 == service { found=1 } END { exit !found }'; then
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

if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "FAIL: zaman-sessiond is not running"
    cat "$DAEMON_LOG"
    exit 1
fi

echo "[4/8] Checking D-Bus contract"
if ! INTROSPECTION=$(busctl --user introspect "$SERVICE" "$PATH_OBJECT" "$INTERFACE"); then
    echo "FAIL: unable to introspect $SERVICE"
    cat "$DAEMON_LOG"
    exit 1
fi
for member in Launch Status Stop Version; do
    if ! printf '%s\n' "$INTROSPECTION" | grep -Fq ".$member"; then
        echo "FAIL: D-Bus member $member is missing"
        printf '%s\n' "$INTROSPECTION"
        exit 1
    fi
done
if [ "$("$CTL" version)" != "0.2.0" ]; then
    echo "FAIL: wrong zaman-sessiond version"
    exit 1
fi

echo "[5/8] Checking registry-backed natural exit"
"$CTL" launch fast "$ROM_DIR/Fast Game.nes" >"$FAST_LOG" 2>&1
if ! "$CTL" status | grep -Fq "state=Idle"; then
    echo "FAIL: daemon did not return to Idle after natural exit"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! "$CTL" status | grep -Fq "result=game exited normally"; then
    echo "FAIL: natural exit result was not recorded"
    "$CTL" status
    exit 1
fi

echo "[6/8] Starting a supervised long-running game"
"$CTL" launch slow "$ROM_DIR/Slow Game.slow" >"$SLOW_LOG" 2>&1 &
CLIENT_PID=$!
OWNS_TEST_UNIT=1

attempt=0
while [ "$attempt" -lt 100 ]; do
    if systemctl --user is-active --quiet "$UNIT"; then
        break
    fi
    if ! kill -0 "$CLIENT_PID" 2>/dev/null; then
        echo "FAIL: launch client exited before $UNIT became active"
        cat "$SLOW_LOG"
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

echo "[7/8] Stopping through the product D-Bus API"
"$CTL" stop
wait "$CLIENT_PID"
CLIENT_PID=""

echo "[8/8] Verifying process and input cleanup"
if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT remained active"
    cat "$DAEMON_LOG"
    exit 1
fi
if ! "$CTL" status | grep -Fq "state=Idle"; then
    echo "FAIL: daemon did not return to Idle"
    "$CTL" status
    exit 1
fi
if grep -Fq "Discovered 0 composite device(s)" "$DAEMON_LOG"; then
    if ! grep -Fq "Guide supervision deferred" "$DAEMON_LOG"; then
        echo "FAIL: controller-free input degradation was not observed"
        cat "$DAEMON_LOG"
        exit 1
    fi
elif ! grep -Fq "InterceptMode to 0" "$DAEMON_LOG"; then
    echo "FAIL: InputPlumber cleanup was not observed"
    cat "$DAEMON_LOG"
    exit 1
fi

kill -TERM "$DAEMON_PID"
wait "$DAEMON_PID"
DAEMON_PID=""
OWNS_TEST_UNIT=0

cat "$DAEMON_LOG"
echo "PASS: zaman-sessiond v0.2 D-Bus and registry smoke test"
