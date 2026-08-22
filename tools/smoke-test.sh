#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
BIN="$REPO_DIR/target/debug/zaman-sessiond"
UNIT="zaman-game.service"

cd "$REPO_DIR"

echo "[1/7] Formatting"
cargo fmt -- --check

echo "[2/7] Compiling"
cargo check
cargo test
cargo build

echo "[3/7] Checking command validation"
VALIDATION_LOG=$(mktemp /tmp/zaman-sessiond-validation.XXXXXX)
if "$BIN" -- sleep infinity >"$VALIDATION_LOG" 2>&1; then
    echo "FAIL: relative executable was accepted"
    cat "$VALIDATION_LOG"
    exit 1
fi

if ! grep -Fq "executable must be an absolute path" "$VALIDATION_LOG"; then
    echo "FAIL: relative executable produced the wrong error"
    cat "$VALIDATION_LOG"
    exit 1
fi
rm -f "$VALIDATION_LOG"

echo "[4/7] Checking supervisor preconditions"
if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT was already active; refusing to disturb it"
    exit 1
fi

SESSION_LOG=$(mktemp /tmp/zaman-sessiond-smoke.XXXXXX)
NATURAL_LOG=$(mktemp /tmp/zaman-sessiond-natural.XXXXXX)
SESSION_PID=""
OWNS_TEST_UNIT=0

cleanup() {
    if [ -n "$SESSION_PID" ] && kill -0 "$SESSION_PID" 2>/dev/null; then
        kill -TERM "$SESSION_PID" 2>/dev/null || true
        wait "$SESSION_PID" 2>/dev/null || true
    fi

    if [ "$OWNS_TEST_UNIT" -eq 1 ] && systemctl --user is-active --quiet "$UNIT"; then
        systemctl --user stop "$UNIT" || true
    fi

    rm -f "$SESSION_LOG" "$NATURAL_LOG"
}
trap cleanup EXIT HUP INT TERM

echo "[5/7] Checking natural game exit"
if ! "$BIN" -- /usr/bin/true >"$NATURAL_LOG" 2>&1; then
    echo "FAIL: natural-exit session returned an error"
    cat "$NATURAL_LOG"
    exit 1
fi

if ! grep -Fq "Session finished: game exited normally." "$NATURAL_LOG"; then
    echo "FAIL: natural game exit was not detected"
    cat "$NATURAL_LOG"
    exit 1
fi

echo "[6/7] Launching and stopping a supervised transient game"
"$BIN" -- /usr/bin/sleep infinity >"$SESSION_LOG" 2>&1 &
SESSION_PID=$!
OWNS_TEST_UNIT=1

attempt=0
while [ "$attempt" -lt 100 ]; do
    if systemctl --user is-active --quiet "$UNIT"; then
        break
    fi

    if ! kill -0 "$SESSION_PID" 2>/dev/null; then
        echo "FAIL: sessiond exited before the game became active"
        cat "$SESSION_LOG"
        exit 1
    fi

    attempt=$((attempt + 1))
    sleep 0.1
done

if ! systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT did not become active"
    cat "$SESSION_LOG"
    exit 1
fi

sleep 0.2
kill -INT "$SESSION_PID"
if ! wait "$SESSION_PID"; then
    echo "FAIL: sessiond returned an error"
    cat "$SESSION_LOG"
    SESSION_PID=""
    exit 1
fi
SESSION_PID=""

echo "[7/7] Verifying cleanup"
if systemctl --user is-active --quiet "$UNIT"; then
    echo "FAIL: $UNIT remained active"
    cat "$SESSION_LOG"
    exit 1
fi

if ! grep -Fq "Final session state: Idle." "$SESSION_LOG"; then
    echo "FAIL: sessiond did not return to Idle"
    cat "$SESSION_LOG"
    exit 1
fi

if ! grep -Fq "InterceptMode to 0" "$SESSION_LOG"; then
    echo "FAIL: InputPlumber cleanup was not observed"
    cat "$SESSION_LOG"
    exit 1
fi

cat "$SESSION_LOG"
echo "PASS: zaman-sessiond v0.1 supervisor smoke test"
