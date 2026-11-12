# Resident renderer and input helper

Your latest test reports about one second to open the menu, faster than the
previous build but still slower than the old menu. The previous architecture
kept Quickshell loaded while restarting the Rust input helper on every opening.
This update keeps BOTH processes loaded and uses read-only controller discovery
while hidden, rather than repeating the complete scan in the opening path.

While hidden, the worker holds no controller interception lease, does not listen
for menu navigation and does not change focus/display settings. An open request
uses its existing session/system bus connections and a recent controller
snapshot. It checks the current InputPlumber owner, source paths, device name,
D-Bus targets and interception mode before any writes. Unknown ALL ownership is
never stolen. Missing/stale/unavailable input still follows keyboard-only startup
and hotplug recovery. Device addition/removal and InputPlumber owner-change signals invalidate hidden
inventory; active-menu recovery still checks inventory every two seconds.

On close, the layer hides first; input, focus and display preview restoration run
as before. Only after restoration does the helper acknowledge idle. Repeated
opens never toggle closed or overlap leases. B/Resume and production Guide/Transfer
preemption retain their existing behavior. Controller loss/errors remain
recoverable without closing the current menu. The helper restarts after an
unexpected process exit; ordinary sessions reuse the same helper PID.

Hidden scans run at startup and when device/service signals invalidate the cache,
with a 15-second fallback refresh. This avoids repeated broad scans competing
with opening requests. Cached snapshots expire after 20 seconds and are never
trusted for identity/mode writes without live checks. A
first opening during worker startup, a missing cache or a daemon restart can
still take the full discovery path. Display responses are also rejected after
the originating page has closed/changed; existing QML request IDs remain distinct.
Actual Dragon latency is unmeasured for this revision; no instant-opening claim
is made. The supplied diagnostics distinguish helper work from renderer time.

## Update on Zaman

Close the game/menu and receive the newest archive in your home directory. Run
as kadhem without sudo. No new apt dependency is introduced. Rebuild only the
development input helper; the production daemon does not need rebuilding.

```bash
(
set -eu
cd "$HOME"
mkdir -p "$HOME/zaman-warm-update"
tar -xzf zaman-quickshell-prototype-hotplug-stability.tar.gz -C "$HOME/zaman-warm-update"
cd "$HOME/zaman-warm-update/zaman-quickshell-prototype"
sha256sum -c SHA256SUMS
python3 apply.py "$HOME/zaman-sessiond-publish"
cd "$HOME/zaman-sessiond-publish"
cargo test --locked --offline --bin zaman-menu-dev-input
cargo build --release --locked --offline --bin zaman-menu-dev-input
./target/release/zaman-menu-dev-input --resident-version
sh "$HOME/zaman-warm-update/zaman-quickshell-prototype/install-resident.sh"
)
```

Use the actual download filename if a suffix was added. The version command must
print `zaman-menu-helper-resident-v1`. Old binaries are rejected before the
installer changes services, without executing their old startup/input code.
apply.py backs up recognized previous input-helper/display-bridge sources and
rejects unknown edits. Tests/build precede prototype replacement and service
restart. Do not continue after a compiler/test failure; share its output.

The installer verifies hashes/prerequisites, backs up the existing prototype,
user units and listener and prints the backup folder. It edits no InputPlumber,
Sway, emulator or production-daemon configuration.

## Diagnose directly: no journal needed

Wait a few seconds after installation, then open/close the menu three times.
Read the latest renderer timings through IPC:

```bash
sh "$HOME/zaman-quickshell-prototype/run.sh" "$HOME/zaman-sessiond-publish" timings
```

The JSON contains menu status, helper idle state, `readyMs` and `visibleMs` for
the last opening. `-1` means the corresponding stage has not completed. These
measure from the renderer's IPC open request, not physical button-to-photon time.
The last timing remains readable after closing. The helper writes its own phase
measurements straight to this file, independent of journal/Quickshell log filters:

```bash
tail -n 80 "/run/user/$(id -u)/zaman-menu-timing.log"
```

Each opening begins with `[menu-startup] begin`; lines contain the stable helper
PID, phase duration and cumulative milliseconds. Warm openings should show
`cached-controller-snapshot` instead of `controller-discovery`. The file is
created by the first open attempt, appends within the user's runtime directory,
and disappears at reboot/logout. A small open-to-helper gap can come from IPC;
a large ready-to-visible gap points to the renderer rather than input setup.

Verify both processes stay alive and retain their PIDs across normal cycles:

```bash
systemctl --user show zaman-quickshell.service -p MainPID --value
pgrep -a -f '[/]target/release/zaman-menu-dev-input'
sh "$HOME/zaman-quickshell-prototype/run.sh" "$HOME/zaman-sessiond-publish" status
```

Idle status is `hidden`, with a helper running as `--resident`. Check game input
returns on close, unplug/reconnect preserves the current page, and Guide still
opens the production fallback. Early cancellation must never show the layer
later. F12 uses its existing Sway binding. Standalone Start/Select release-tap
limitations and simultaneous USB/Bluetooth handoff remain separate device tests.

## Revert

Close the menu and stop the resident renderer/shortcut service. Restore both
helper sources from their `.pre-update-*` backups printed by apply.py, then
rebuild just `zaman-menu-dev-input`. Restore the previous prototype from the
installer's printed backup directory so QML and the helper protocol match.
Restore saved user units/listener if needed, daemon-reload and start the previous
services. Do not mix the previous helper with this revision's QML.

## Verification here

Actual QML functions passed 556 controller/navigation assertions and 111
resident lifecycle cases, including 50 open/close cycles reusing one helper.
Eight actual launcher/stop integration tests passed with mocked QS/systemctl,
including direct timings and refusing an old helper without running it. Shortcut
logic checks passed; this container blocks its socket test. Source application,
known-source backups, unknown-edit rejection, syntax and archive checks passed.

Rust compilation/tests, native Qt and actual latency could not run here because
those toolchains are absent. Run the supplied tests/build on Dragon before
installation. No software was installed on Zaman by the assistant.

References: [Quickshell IPC](https://quickshell.org/docs/v0.3.0/types/Quickshell.Io/IpcHandler/)
and [process lifecycle](https://quickshell.org/docs/v0.3.0/types/Quickshell.Io/Process/).
