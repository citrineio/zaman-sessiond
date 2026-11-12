> Historical prototype notes. Use the repository root README for current features and setup.

# Zaman independent Quickshell prototype

See [Sprint 1 storefront candidate](../sprint1-storefronts.md) for the paired source branches and device acceptance.

## Resident renderer and input helper

Start with [RESIDENT-MENU.md](../../RESIDENT-MENU.md) for the current deployment.
Both Quickshell and the development input helper now stay loaded between menu
openings. The hidden helper performs read-only inventory scans; opening reuses
bus connections and a recent snapshot, then validates live identity/ownership
before interception. Closing restores input/focus/display and returns the helper
to idle. This update REQUIRES rebuilding zaman-menu-dev-input. Diagnostics use
run.sh's timings command and a direct runtime timing file; no journal is required.
The installer checks the new helper protocol before changing services and saves
a backup. Historical launch/build notes below describe older revisions; the
resident guide takes precedence.

Target: the user's existing Quickshell 0.3.0 on Dragon/Sway.
The input helper previously compiled and all nine tests passed on the Dragon.
B closing, Resume, Guide preemption into the production SDL fallback and the
simplified Settings layout were confirmed by the user. This revision uses native
Quickshell PipeWire integration for Audio. Source/package checks were performed;
no compilation, unit tests or QML runtime checks were run by the assistant.
This revision adds Display settings with a verified, timed Sway mode preview
inside the existing development input/focus helper. Bluetooth uses native
Quickshell discovery/device controls and an application-scoped pairing helper.
It also fixes run.sh to refresh a missing/stale Sway socket from the user manager.
The user confirmed that refreshing SWAYSOCK allowed the prior Network package to
launch; Wi-Fi password entry was not yet exercised. This Bluetooth revision has
not been compiled or run on the Dragon. Existing input/hotplug source is retained.
Three ownership/source-identity unit tests were added for the user to run.
The Display revision adds fifteen transaction/protocol tests: the expected input
binary total is now 27, plus two tests in the pairing binary. These are not results;
the assistant did not compile, run tests or launch QML.
Native volume adjustment was confirmed working by the user. Audio
mute has not yet been tested. An output-switch request from DisplayPort to
Headphones was shown timing out; server metadata and jack connection status
are pending before diagnosing or changing its backend. The row is named Audio output to
clarify that it chooses a device; it does not play a volume-test sound.

## Controller lifecycle failures never close the menu (2026-10-09)

This revision uses the supplied hotplug-stability archive as its baseline.
Controller removal, reconnect, source-path reuse, interception-mode changes,
input-service restarts, discovery/IPC errors, timeouts and failed input listeners
now reset held input and retry while the menu stays open on its current page.
A missing input service or system bus at startup permits keyboard-only use;
recovery retries create fresh bus connections when input is interrupted.
Warnings stay visible until usable controller input confirms recovery.

Deliberate B on the main menu, Resume/Return to library, Escape, Ctrl+C and
external foreground takeover retain their existing close behavior, as requested.
Guide still belongs to the production fallback, whose foreground takeover closes
this preview deliberately. Renderer failure and compositor/session failures
still use the existing shutdown and rollback path.

Restoration rights are discarded when the service owner or physical source
changes, or interception is released. A controller already intercepted by
another owner is skipped while the UI stays open. Acquisition verifies physical
identity again before writing. Listeners use the current daemon's unique bus
name and only targets belonging to acquired controllers. A failed listener is
reported even if another controller's listener remains alive. Disconnect alone
does not cancel a display preview; its normal timed rollback still applies.

Local validation: 556 lifecycle/navigation assertions passed using the actual
shell.qml JavaScript functions with mocked Qt objects. Python syntax, launcher
shell syntax, applicator update/idempotency/unknown-edit protection and archive
checks were also checked. Four Rust restoration-rights regression tests were
added (expected input-helper total: 41). Rust compilation/tests and Quickshell
runtime/device tests were NOT run here: Rust and Quickshell are unavailable in
this environment. Run the on-device commands below before treating hardware
recovery as verified. The JavaScript harness can also be run with:

```bash
node zaman-quickshell-prototype/tests/menu-controller-regression.cjs
```

Device checks: disconnect while holding a direction; receiver unplug; rapid
Bluetooth reconnect; input-service restart; source replacement; one controller
lost while another remains; keyboard-only startup with no input service; and
disconnect during a display preview. Expect the menu/page to remain visible,
held navigation to stop, and reconnect to restore navigation without relaunch.
Confirm deliberate B/Resume/Escape closing and normal input/focus restoration.

## Hotplug stability: deferred focus and warning recovery (helper rebuild required)

This revision addresses the reported ControllersPage line 103 delayed-evaluation
exceptions and repeated loss warnings. Focus-animation callbacks previously
captured ListView delegates in Qt.callLater lambdas. Model replacement can
destroy those delegates before evaluation. Deferred scrolling now uses a timer
owned by each page and reads the page's current selected index, with visibility
and range checks. The same delegate callback pattern was replaced in Controllers,
Audio, Network, Bluetooth, Display and the main/settings list. The page timer is
destroyed with its page; it never captures a row's id or index.

Controller discovery previously cleared a loss episode as soon as a snapshot
reported normalized input. That did not prove the new listener worked, allowing
listener failures to immediately warn again. The loss warning now remains latched
until a finite, recognized controller button press reaches the presented menu.
Keyboard input, Guide, release events and invalid values do not clear it. Press
a navigation/accept/back button after reconnecting to confirm recovery. Listener
retries continue while the menu remains open; failures within one loss episode
do not produce repeated warnings. A new loss after confirmed working input still
warns again.

Two additional regression tests cover the recovery proof conditions. Expected
input-bin count is now 37. Source/package checks were performed; compilation,
unit tests and QML/device checks were NOT run by the assistant. Rebuild only the
development input helper. Verify on Controllers while repeatedly unplugging and
reconnecting, leave/reenter the page during row animation, then test keyboard
closing and actual controller navigation after reconnect. Retain old failed test
observations and record this as a new run.

## Controller loss: keep open and recover (helper rebuild required)

The prior controller-loss package automatically closed the preview because the
assistant misinterpreted the requested warning. This revision replaces that
policy: the menu stays open, stops held controller navigation, shows a warning
under its heading and remains usable with the physical keyboard. Reconnect the
controller without restarting the preview. Detection/discovery uses the existing
two-second poll; recovery is not instantaneous.

Controller-scoped errors during discovery or acquisition are recoverable. The
helper discards the failed scan and retries, retaining old lease identities for
guarded cleanup until a complete snapshot arrives. Removed/replaced identities
are then pruned, new controllers are intercepted only if another owner has not
already intercepted them, and input listeners are recreated. A reused runtime
path with a different source is handled as a new controller after pruning its
old lease. The warning clears after a usable controller button press. Repeated
failed scans do not repeat the same console warning within one loss episode.
Startup discovery also retries hotplug races within its existing timeout.

Controller loss alone no longer sends closing/closed, hides the layer, changes
the current page or cancels a display preview. Its countdown still runs and
reverts on expiry. Explicit B/Escape/Resume closing, Guide preemption, Ctrl+C and
actual failures retain the existing cleanup/rollback behavior. Cleanup skips
vanished objects; permission, transport, timeout and ownership errors during
input recovery are shown as warnings and retried. Keyboard-only startup remains supported.

The seven prior loss/error regression tests are retained with recovery-oriented
names. One additional test checks acquiring a replacement after discarding the
old lease while rejecting another owner's ALL mode. Expected input-bin total:
35 tests in that revision (37 with the hotplug stability update above). The assistant performed source/package checks only; compilation, unit
tests and QML/device checks were NOT run. Rebuild zaman-menu-dev-input after
apply.py. No installation or production service change is needed.

For the test checklist, disconnect cases should again expect the menu to stay
open and recover automatically. Preserve the previous failed observations and
start a new run. Check controller/receiver unplug, held-direction disconnect,
rapid reconnect, keyboard closing without a controller, another surviving
controller, library/fullscreen-game focus, and disconnect during a display
preview. Expect a warning, stopped held navigation, no UnknownObject ERROR,
working controller navigation after reconnect and no stranded input on close.

## Contrast update (QML only)

The panel gradient now uses 254/255 alpha throughout to suppress background
lettering while retaining its dark blue tint. Secondary text is brighter on
all menu/settings pages and darker on the sand focus highlight. Disabled Display
rows use explicit muted text colors instead of fading the entire text column;
their existing explanatory text and action guards are preserved.

The user supplied direct 1920 x 1080 and 2560 x 1440 screenshots. Original-pixel
inspection showed no obvious whole-menu blur at either resolution. This change
addresses translucency and contrast; it does not alter text rendering, sizes,
focus animation, controller navigation, backend code or display transactions.
No compilation or device/QML runtime testing was performed for this revision.
The assistant checked the visual edit scope, colors and package integrity.

With the existing Display helper already built, replace the prototype QML and
restart the preview using run.sh. No helper rebuild is needed for this QML-only
update. On the Dragon, check background titles no longer compete with menu text,
secondary labels are readable, and an unchanged mode still cannot be previewed.
Compare at 1080p and 1440p with output scale 1, and check focused/disabled rows.

## Display (compilation and device validation pending)

Settings > Display has Display, Resolution, Refresh rate, Preview changes and
Back rows. Up/down selects; left/right chooses; A/Enter cycles a selector or
activates an action. Mouse arrows choose either direction. The existing gold
highlight, icon enlargement and 120 ms row growth are preserved. Only active
Sway outputs and their advertised modes appear. Refresh rates retain exact
millihertz (for example 59.940 Hz). No custom modelines are generated.

Nothing changes while choosing. Preview applies only the selected mode, verifies
it through GET_OUTPUTS, then opens a 15-second Keep/Revert screen. Revert is
selected by default. Keep verifies the same output identity and active mode
again before saving. Expiry, B/Escape returning to Settings, closing the menu,
Guide preemption, renderer EOF/heartbeat loss and SIGINT/SIGTERM invalidate the
transaction and request rollback. Rapid close/reopen rejects earlier requests.
The countdown runs independently of QML polling and the helper's timer owns the
actual deadline. Stale preview tokens are decimal strings, never JS numbers.

The transaction backend is a private copy adapted from the supplied display.rs,
placed under src/bin/zaman-menu-dev/. Production src/display.rs is not changed
and production menu D-Bus methods are not called. The existing development input
helper owns its lifetime so unloading a page does not kill rollback. Blocking
Sway IPC/file writes run on a separate worker, not in the controller-input loop,
including during hotplug discovery. Quickshell handles the layer-shell UI;
the helper uses Sway IPC for checked command replies and independent rollback.
No new dependency, package, service or standalone display binary is added.

Keep atomically updates the existing compatible preference file,
${XDG_CONFIG_HOME:-$HOME/.config}/zaman/display.json, preserving preferences for
other displays. It does not edit/reload Sway configuration. Opening the prototype
or page only reads preferences; it NEVER applies saved modes. The supplied
production daemon has its own startup preference restore; this package does not
restart it or guarantee restoration against every external Sway configuration.
Unreadable/malformed preferences block saving rather than silently replacing
them. Changing output requires matching connector, make, model and serial.

If the display disconnects or a different display takes its connector, rollback
does not write to the replacement. If another program changes the mode to a
third mode during preview, that external mode is left untouched and Keep fails.
IPC recovery failures are reported and rollback is retried while the helper is
alive; shutdown also retries with an eight-second budget (individual IPC waits
may extend it). SIGKILL, helper/system crash or an unavailable compositor cannot
guarantee recovery. If needed, restore a known advertised mode manually from
another terminal with swaymsg; do not force a custom mode.

This update REQUIRES rebuilding zaman-menu-dev-input. An older helper shows a
rebuild notice in Display instead of waiting forever for an unsupported command.
The Bluetooth pairing binary is unchanged. No compilation, unit tests, QML
runtime or hardware validation was performed here.

### Display device checks

1. Open from both Pegasus and a fullscreen game. Compare output names, current
   resolution and refresh against `swaymsg -t get_outputs`. Selecting options
   must not change the picture before Preview.
2. Choose a known supported alternative. Preview, verify the picture, then Keep.
   Reopen Display and inspect the active mode and display.json. Only explicit
   Keep may update preferences.
3. Preview another supported mode and do nothing for 15 seconds. Verify exact
   previous resolution AND fractional refresh are restored and preferences did
   not change. Test Revert explicitly as well.
4. Preview then B/Escape out, close the menu, Ctrl+C, and separately Guide into
   the fallback. Confirm rollback, focus restoration and controller navigation.
   Repeat Back immediately after Preview, before the apply response arrives.
5. Rapidly leave/reenter Display during a request. No stale Keep or response may
   save/change a newer transaction. Disconnect/reconnect the selected display;
   no saved mode should be sent blindly to a replacement monitor.
6. Test low-resolution scrolling, a stationary pointer during controller use,
   long monitor names and fractional/high refresh rates actually advertised by
   the device. Unsupported HDR, scaling, VRR and orientation controls are not
   offered in this revision.

## Bluetooth adapter discovery fix

The first Bluetooth screenshot showed no adapter and the startup notice that the
adapter changed. Source inspection found a race: assigning defaultAdapter from
onAdaptersChanged could capture null before default selection settled, breaking
the property binding. The selected adapter now uses a persistent read-only binding
that follows an explicit selection, then the native default, then the first
available adapter. Cleanup runs on an actual selected-adapter change. Initial
adapter arrival no longer reports that an adapter was removed. This QML-only fix
needs no Rust rebuild. Hardware availability still needs checking if it remains
empty after relaunch; no service is restarted by this fix.

## Bluetooth (compilation and device validation pending)

Settings > Bluetooth uses Quickshell.Bluetooth for adapter/device discovery,
power, connect/disconnect, trust (automatic reconnection), unblock, forget and
reported battery data. Paired and discovered devices are distinct from Connected;
a green dot follows the native connected state, and battery is shown only when
batteryAvailable is true. Names are plain text; selection tracks device paths.
Multiple adapters can be selected with the adapter row. No packages are installed.

Find devices starts a 30-second discovery period. Put the controller/accessory
in pairing mode first, select it, then choose Pair and connect. Cached discoveries
may outlive the device's presence; the UI does not claim every listed device is
currently nearby. Discovery is stopped on leaving the page, pairing, or closing.
An existing scan started elsewhere is observed without claiming ownership. The
Bluetooth item stays instantiated after first opening until the preview closes,
so a delayed StartDiscovery response can be stopped after returning to Settings.
If native discovery fails without a state change, close/reopen the preview to
clear the outstanding scan request before retrying; 0.3.0 exposes no scan-error
signal to QML. No device is made discoverable and no incoming-pairing policy is
changed by opening this page.

Quickshell 0.3.0's documented Bluetooth API has Pair/CancelPair but no pairing-agent
prompt API. Its Pair method relies on the system default agent when none is
registered by its own connection. To display PINs and confirmation codes inside
this menu without taking over the default agent, the new zaman-menu-dev-pair
helper registers an application-scoped BlueZ Agent1 and calls Pair from that SAME
D-Bus connection. This helper is only started for explicit pairing, not for
status refresh or other Bluetooth controls. Its agent method handling is adapted
from the supplied prior Zaman menu source; it does not call or modify that menu.

- Matching-code and authorization prompts default to Cancel. Confirm only the
  code shown on the selected accessory. No numeric-comparison request is
  automatically accepted.
- Displayed keyboard codes retain leading zeros and show entered-digit progress
  when BlueZ supplies it. Type that code ON THE OTHER DEVICE and press Enter.
- Requested PINs use the shared controller keyboard. Six-digit passkeys use a
  numeric layout; physical keyboard typing and Enter also work. B cancels the
  pairing operation. Entered credentials are not written to logs or argv.
- RegisterAgent never requests default-agent status. Prompts accept only the
  selected path and the captured BlueZ unique bus owner. Before pairing, address
  and adapter are rechecked. Identity reads, registration and Pair are pinned to
  that same owner so a restarted service cannot receive the old request.
- JSON prompt tokens reject stale replies. EOF, cancel, SIGTERM/SIGINT, missing
  five-second heartbeat, 90-second unanswered prompt and 120-second Pair timeout
  terminate the attempt. Pending reply channels are released before unregistering
  the agent. Best-effort CancelPairing and cleanup use bounded waits.
- Pair success is confirmed by BlueZ's Paired property. Native Quickshell Connect
  follows only for the same surviving device while the page remains visible.
  Pairing never automatically changes trust; use Automatic reconnection to opt in.
- Disconnect, Forget and Bluetooth Off require a second, explicit selection and
  explain the possible loss of the input device. Pairing must finish/cancel before
  another operation can begin. B remains available during ordinary operations.
- Native mutations have a 20-second UI deadline. A timeout is not cancellation;
  device state continues to update. The native library can update some setter
  properties optimistically; validate power and trust against actual device
  behavior/backend state, not only immediate local visual feedback.
- If the module is absent, Bluetooth shows an unavailable/Back screen. If the
  helper has not been built, the rest of Bluetooth remains usable and Pair shows
  an actionable message. No automatic installation or build is attempted.

Two pairing-helper unit tests are included for sender/device checks and reply
validation. They have NOT been run here. Existing input-helper tests remain for
the user's build (expected 27 input tests + 2 pairing tests, not a claimed result).

### Bluetooth device checks

1. Open Settings > Bluetooth, compare the paired/connected list and any reported
   battery, and check adapter switching if more than one adapter exists.
2. Select Find devices, put a spare controller into pairing mode, select it and
   choose Pair and connect. Verify Paired and Connected independently and that
   the existing InputPlumber hotplug path brings navigation online.
3. Enable Automatic reconnection for that paired device; turn the controller off
   and on, then verify reconnection. This UI does not create InputPlumber mappings
   for unsupported hardware.
4. With a keyboard or phone that supports confirmation, check matching-code
   Cancel/Confirm and six-digit leading zeros. For a Bluetooth keyboard, type a
   displayed code on that keyboard; test requested PIN entry where applicable.
5. B during pairing must cancel. Close the entire menu or press Guide during a
   prompt and confirm the fallback/input focus still works and no pairing agent
   remains active. Unplug the adapter or move the device out of range mid-pair.
6. With another working input device available, exercise Disconnect, Forget and
   Bluetooth Off: first cancel each confirmation, then confirm the intended one.
7. Start searching and immediately go Back; check discovery stops even if its
   start response was delayed. Reopen the page, search again, and check timeout.
8. Reopen Audio, Network and Controllers and confirm their prior navigation and
   controls, including Wi-Fi password entry. Device testing remains with the user.

## Native Network (device validation pending)

Settings > Network uses Quickshell.Networking directly. The supplied production
menu source uses NetworkManager; Quickshell 0.3.0 supports that backend. This
revision does not replace the network service, use nmcli polling, install a
module, or change Rust code. The optional QML module loads only when Network is
opened. An unavailable module shows a Back screen; an absent backend or adapter
is shown within the page. A standalone iwd-only system is not supported by this
native module; do not install or migrate services just to test this preview.

- Live Ethernet status and Wi-Fi names, signal levels, security, saved status,
  and connected state. Each adapter's networks are shown with its interface
  name when more than one Wi-Fi adapter is present. Unknown/hidden SSIDs are not
  exposed as joinable entries. Stable name ordering avoids signal-based jumping.
- A green dot and Connected label follow the actual network.connected property.
  Internet reachability is shown separately from local connection status, using
  the existing backend connectivity result. The page does not enable connectivity
  probes or claim internet access when checks are disabled or unknown.
- Wi-Fi on/off, automatic scanning while the page is open, Connect, Disconnect,
  and Forget. Scanning restores the previous scannerEnabled value on leaving.
  Disconnect uses the device API documented to inhibit automatic reconnection;
  check this carefully on the installed 0.3.0 build. No autoconnect profile flag
  is rewritten by the UI.
- Connect first tries stored credentials. A NoSecrets failure opens the masked
  password keyboard for WPA/WPA2 Personal and WPA3 SAE. Enter password explicitly
  permits replacing an obsolete saved password. Enterprise/WEP setup, hidden
  networks, manual IP/DNS configuration and captive-portal browser login are
  outside this revision. Existing configured profiles can be tried normally.
- Passwords: D-pad moves through the grid; A enters a key. ABC, symbols, Space,
  Delete and Join are the bottom row. A physical keyboard also types directly;
  Enter joins and Backspace deletes. B/Escape cancels password entry. Printable
  ASCII is supported; non-ASCII passphrases are outside this initial keyboard.
  Passwords are masked and cleared on leaving/submitting; no secret is sent to
  a shell, input helper, log or command argument. The networking backend may save
  successful credentials for future connections.
- Forget defaults to Keep saved network and requires selecting a second action.
  Its text makes clear that ALL profiles for that network will be removed and
  an active connection may be interrupted. Other profiles are not selected.
- Only one UI request is tracked at a time. Connection confirmation waits up to
  45 seconds; other operations up to 10 seconds. Failures and missing targets
  show an error instead of claiming success. A timeout does not cancel a request
  already accepted by NetworkManager. Back stays available during an operation;
  leaving the page does not undo a connection request or disconnect the device.
- Backend objects, not cached numeric IDs, are used for writes. Network/adapter
  disappearance closes password entry and clears its text. Native service/build
  lifetime handling must still be checked on the Dragon.

The Network page itself needs no Rust changes; this combined Display package
does require rebuilding the input helper. Source review and archive integrity checks only
were performed here. No unit tests, compilation, QML runtime or device tests ran.

### Network device checks

1. Open Network from both the library and a fullscreen game. Confirm Ethernet,
   Wi-Fi and connected SSID against the existing system status. B should return
   through network details and Settings; Guide must still open the fallback.
2. Connect to a saved network. Watch for the green Connected label only after
   connection, then leave and reopen Network to confirm the status remains live.
3. Join a new WPA2 network entirely with the controller. Try a wrong password,
   then correct it. Test uppercase, symbols and spaces; also test keyboard entry.
4. Disconnect, wait and check it stays disconnected; explicitly reconnect. Toggle
   Wi-Fi off/on and confirm the actual radio/connection state follows the page.
5. Test Forget on a disposable saved network: first cancel, then confirm. Verify
   saved status disappears and the next connection asks for credentials.
6. Remove a Wi-Fi adapter or make the selected network disappear during details,
   password entry and connection. Confirm an error/back path, no stale writes,
   and preserved controller/keyboard navigation. Close/reopen during a request.
7. Confirm long network names render as plain text and do not push controls out
   of view. Check scrolling and the existing row-growth effect with many networks.
8. If the module fails to load, capture the terminal's QML errors. Do not install
   anything automatically; Audio and other menu pages should remain available.

## Controllers

Settings > Controllers lists the actual InputPlumber composite names, menu-input
availability, current game-input targets and active profile. Up/down selects a
controller and the detail area follows that device; Back returns to Settings.
No battery percentage, transport type or unverified physical model is invented.
Names may be the composite configuration's label, rather than the USB/Bluetooth
advertised name. This page observes the existing mappings; it does not edit them.
Only devices managed by InputPlumber are listed.

The existing development helper reads the CompositeDevice D-Bus properties and
sends the list to QML through the existing input bridge. No additional process,
dependency, daemon change or command-line polling is added.
The existing concurrent two-second inventory watch now updates the list and
re-subscribes after hotplug. It captures the prior interception mode for each new
controller before acquiring it, removes vanished identities from the restoration
set, and rechecks source identity before restoring an original mode. New sources
already in ALL mode are not claimed. Existing touched sources leaving ALL mode,
service-owner changes and changed physical-source identities discard stale
restoration rights and retry without closing the preview.

Inventory reads and new-device interception writes are polled alongside input,
close commands, renderer heartbeats and foreground preemption. Held navigation
is cleared on inventory changes. With no managed controllers, the menu remains
usable with the keyboard; reconnecting a supported controller should restore
navigation after the next inventory check. Device validation is required.

Bluetooth pairing is now available in Settings > Bluetooth, using native device
controls plus the scoped prompt helper described above. Controller remapping is
still a later integration. Guide keeps the production fallback;
the optional F12 Sway binding continues to launch this same run.sh script. An
unused-controller-button launcher remains pending the Dragon's capability output.

## Native Audio

- AudioPage.qml imports Quickshell.Services.Pipewire. No audio helper process,
  command-line audio calls, JSON bridge or polling is used by the page.
- Pipewire.nodes.values supplies the live output list. The page filters audio
  sinks and excludes application streams. Descriptions/nicknames/names label
  outputs. Candidate and pending selections hold PwNode objects, avoiding
  cached numeric node IDs and replacement-device ambiguity.
- Pipewire.defaultAudioSink identifies the actual active default output.
  PwObjectTracker binds that node while Audio is open. Volume and mute are read
  only when the service is initially synced, the node is ready and audio channels
  are present. Null/default transitions leave those controls unavailable.
- Volume writes node.audio.volume directly in five-percentage-point steps,
  bounded to 0–1 (0–100%). Existing levels above 100% are displayed as reported;
  this page will not increase them. Mute is independent and retains volume.
  A on Volume has no side effect. Native setters provide local/live property
  updates; device validation must confirm actual playback and server state.
- Audio output uses left/right to choose a device; A writes
  Pipewire.preferredDefaultAudioSink. That is a preference, not guaranteed
  routing: Active is shown only for defaultAudioSink. A 2.5-second one-shot
  timeout reports a preference that has not become active; pending selection
  removal reports disconnection. Existing playing streams follow system policy.
- The page has four rows: Volume, Mute/Unmute, Audio output and Back. Live
  updates replace Refresh. Navigation and held-repeat timing remain immediate
  with a 220 ms initial delay and 70 ms repeat interval.
- B/Escape and Back return to Settings. Left/right operate volume/output on
  Audio; the list and other pages retain their normal Left/back behavior.
- Audio is loaded on demand via Loader. If the existing Quickshell build lacks
  the PipeWire module, Audio shows an unavailable message and Back still works.
  The rest of the menu does not import this optional module eagerly. Nothing is
  installed or downloaded to resolve an unavailable integration.
- No new Rust binary is needed. The previously proposed development audio
  helper is no longer bundled, referenced by the launcher or required to build.
  This archive does not delete any old development files from the user's project.

## Main menu and Settings

Main menu: Resume/Return to library, Settings, Reboot, Shutdown, Exit Game.
Exit Game stays last. Settings contains Audio, Controllers, Network, Bluetooth,
Display and Appearance. Audio has real controls and Controllers has a live device
overview. Network has native connection management and Bluetooth has device and
pairing controls. Display has timed mode previews. Appearance, power and Exit Game remain
placeholders until their backends are integrated.
Appearance has a palette icon and a future theme/color/interface customization
page. B/Escape returns one level and restores the previous selected category.
From the main menu it closes the preview. No Guide integration or boot service
is added; the production SDL menu remains the fallback.

## Focus animation

MenuFocusRow animates actual delegate height over 120 ms with OutCubic easing.
The focused row gains 12 design pixels of height, so ListView pushes its
neighbors apart as it grows. The old row shrinks when focus moves. Main rows
are 66 pixels normally and 78 when focused; Audio and Controllers add the same
12 pixels to their different base heights. Sizes follow the existing UI scale.
The label gains two design pixels of font size and the icon gains two pixels;
text is rendered at its actual rounded font size. A shared progress value also
drives the card's small horizontal expansion. Gold/sand selection changes
immediately, with no input delay or queued animation. Initial/hidden rows snap.

The selected row is kept in view after expansion. Pointer tracking compares
window coordinates so rows moving underneath a stationary mouse do not keep
changing focus. The new effect applies to Main, Settings, Audio and Controllers.

The animation and Network page remain QML-only. This Bluetooth update needs the
new development pairing helper built with the existing offline dependencies. The previously delivered Controllers helper still needs its
own build and device validation if those have not been completed yet.

## Update and launch

Close the preview with B/Escape. Receive the archive into the Dragon home folder.
The fallback filename below accommodates the extra .tar suffix seen on Android.
Run these commands on the Dragon; local validation above does not replace Rust/device tests:

```bash
(
set -eu
cd "$HOME"
ZAMAN_ARCHIVE="zaman-quickshell-prototype-hotplug-stability.tar.gz"
if [ ! -f "$ZAMAN_ARCHIVE" ]; then ZAMAN_ARCHIVE="$ZAMAN_ARCHIVE.tar"; fi
cp -a zaman-quickshell-prototype/qml "zaman-quickshell-prototype/qml.pre-controller-recovery-$(date +%Y%m%d-%H%M%S)"
tar -xzf "$ZAMAN_ARCHIVE"
python3 zaman-quickshell-prototype/apply.py "$HOME/zaman-sessiond-publish"
cd "$HOME/zaman-sessiond-publish"
cargo test --locked --offline --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
cargo build --release --locked --offline --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
zamanctl menu close
sh "$HOME/zaman-quickshell-prototype/run.sh" "$HOME/zaman-sessiond-publish"
)
```

The guarded applicator recognizes the previously delivered input-helper
versions and backs up before writing. It also adds src/bin/zaman-menu-dev-pair.rs
and src/bin/zaman-menu-dev/{display,display_bridge}.rs. Identical sources are
retained; unknown source edits are refused before any writes. No Cargo
manifest, installed binary, production source or service is changed. The helper
is executed from the project's target/release directory.

The launcher uses installed qs and reads missing graphical environment values
from the existing user systemd manager without changing it. It loads the
project's existing fonts privately, and falls back to system fonts if missing.
Original proof QML and production services are unchanged. A previously configured
F12 binding launches the updated menu through this same launcher after closing
this terminal-launched preview.

## Preserved appearance

The inset rounded charcoal panel, subtle slate-blue tint/border, warm-gold
selection cards, line icons and native/hinted text remain. Selection changes
are immediate; row height, label/icon size and card geometry follow a 120 ms transition. The clock and discovered-controller indicator remain;
connection/battery state is not invented. The UI states that the game is still
running, rather than claiming an authoritative pause state. Actual pause on
focus loss remains an emulator configuration requirement.

qml/assets/zaman-logo-source.png is the supplied original PNG, byte for byte.
ZamanMark.qml samples its bird/wordmark regions, maps brightness to alpha and
tints them warm ivory at draw time. It does not retrace or alter the artwork.
Assets stay inside the qml shell root to avoid blocked parent-relative paths.
The cached small mask is generated after source paint, using a qualified crop
rectangle and Qt's seven-argument pixel-write form. Header dimensions and logo
rendering are preserved in this row-growth revision.

## Preserved input/focus ownership

A separate Rust input helper acquires the controller interception lease and
forwards normalized D-pad/A/B events to this manual layer-shell overlay above
tiled Pegasus or fullscreen games. It does not change fullscreen or tiling.
D-Bus foreground and inventory checks are persistent concurrently polled futures
so they do not stall input. Inventory checks run every two seconds.

Resume/Return to library closes the preview. The renderer hides the exclusive
layer first; the input helper then restores Sway focus by container ID and PID
and restores the input lease. It handles EOF, SIGINT, SIGTERM, missing renderer
heartbeat and foreground preemption. Closing-button release has a 750 ms budget.
Guide stays assigned to the production SDL menu. If that menu or Transfer takes
over, this preview yields ownership. A private lock prevents duplicate previews.
Ordinary controller additions/removals refresh the input lease and list. Source
replacement or input-service/ownership changes reset input and recover while
the preview stays open. Appearance, power and Exit Game integration remain later work.

## First setup only

The development input binary and the new one-shot pairing binary use Rust. They rely on existing dependencies
and modules, without Cargo.toml/Cargo.lock changes. apply.py recognizes the
previous helper checksum, backs it up when updating and refuses unknown local
edits before writing. It changes no production source or installed binary.
This combined Display package requires updating/rebuilding the input helper;
the native Audio and Network pages themselves add no Rust dependencies:

```bash
python3 zaman-quickshell-prototype/apply.py "$HOME/zaman-sessiond-publish"
cd "$HOME/zaman-sessiond-publish"
cargo test --locked --offline --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
cargo build --release --locked --offline --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
```

Compilation and testing stay with the user. The input binary runs from target;
it is not installed in /usr/libexec.

## Focus animation device checks

1. Tap up/down on Main and Settings: the new highlight must appear immediately.
   The whole focused row should expand, its label/icon enlarge slightly, and
   neighboring rows move as the former selection contracts.
2. Hold a direction, reverse it and release. Repeat timing should match the prior
   build, with no queued animation lag or movement after release.
3. Check Audio and Controllers, including the first/last row and scrolling. The
   enlarged selection must stay visible and rows must not overlap.
4. Leave the mouse stationary while navigating with the controller. Row motion
   must not steal focus. Move/click the mouse to verify normal pointer selection.
5. Validate on the Dragon; no compilation or QML runtime was run here.

## Controllers device checks

1. Open Settings > Controllers from Pegasus and a fullscreen game. Compare the
   names/profile/targets with `inputplumber devices list` and the device's info.
2. Keep the preview open and disconnect/reconnect a controller. The list should
   update after the next two-second scan; with no controller, use keyboard arrows
   and Escape. With the device reconnected, D-pad/A/B should work again.
3. Disconnect while holding down: selection must stop instead of repeating.
   Connect a second managed controller and confirm its navigation, then close
   the menu and confirm that both controllers return to their previous input modes.
4. B and the Back row return to Settings with Controllers selected. Resume and
   Escape close normally; Guide still preempts into the production SDL menu.
5. If the preview reports a service restart, changed source or another input owner,
   it must stay open and avoid restoring stale modes onto a replacement source.
   Controllers already intercepted by another owner must not be acquired.

## Audio device checks

1. From Pegasus and a fullscreen game, open Settings > Audio. The actual output,
   volume and mute should populate, with no Rust audio build or spawned wpctl /
   pw-dump calls. Terminal errors are useful if the module cannot load or fails.
2. Compare the displayed level to wpctl get-volume @DEFAULT_AUDIO_SINK@. Tap and
   hold left/right, reverse direction and release. Verify real playback and
   server readback, not just local property feedback. A on Volume must not mute.
3. Use Mute/Unmute. Change volume while muted, then unmute and confirm that level
   is retained. Change volume/mute externally and confirm the page updates live.
4. With multiple outputs, choose one without A: it must remain Not applied and
   routing must not change. A applies the preference. Verify the actual default
   and sound; removal/default changes must not cause writes to a replacement
   node with a reused numeric ID. Check B remains responsive during switching.
5. B returns to Settings preserving Audio selection; B again returns to the
   main menu preserving Settings. Resume, B/Escape closing and Ctrl+C must still
   restore keyboard/controller input and focus. Guide must still preempt into
   the production fallback. Check held direction stops on page transitions.

The nine previously passing input tests cover Guide edges, inventory identities
and foreground restoration policy. This revision adds three tests for hotplug
ownership and source reuse. Tests were added but not run by the assistant; the
Display adds fifteen more transaction/protocol tests, making the expected total
27 in this input binary. They do not validate real D-Bus/compositor/audio behavior.

## Remove prototype source

After closing, src/bin/zaman-menu-dev-input.rs, src/bin/zaman-menu-dev-pair.rs,
src/bin/zaman-menu-dev/ and
the prototype folder can be removed if desired. No daemon/service rollback or
package removal is needed. Any earlier audio helper files/binary are unused by
this revision and left untouched by the applicator.

## API references

- https://github.com/swaywm/sway/blob/master/sway/sway-output.5.scd
- https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd
- https://quickshell.org/docs/v0.3.0/types/Quickshell.I3/I3/

- https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Pipewire/Pipewire/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Pipewire/PwNode/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Pipewire/PwNodeAudio/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Pipewire/PwObjectTracker/
- https://quickshell.org/docs/v0.3.0/types/Quickshell/ObjectModel/
- https://quickshell.org/docs/v0.3.0/types/Quickshell/PanelWindow/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Wayland/WlrLayershell/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Io/Process/
- https://doc.qt.io/qt-6/qml-qtquick-loader.html
- https://doc.qt.io/qt-6/qml-qtquick-text.html
- https://doc.qt.io/qt-6/qml-qtquick-canvas.html
- https://doc.qt.io/qt-6/qml-qtquick-context2d.html

- https://inputplumber.readthedocs.io/en/latest/dbus-interface/composite_device/

- https://doc.qt.io/qt-6/qml-qtquick-behavior.html
- https://doc.qt.io/qt-6/qml-qtquick-numberanimation.html

- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/Networking/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/Network/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/WifiNetwork/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/WifiDevice/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Networking/NetworkDevice/

- https://quickshell.org/docs/v0.3.0/types/Quickshell.Bluetooth/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Bluetooth/BluetoothAdapter/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Bluetooth/BluetoothDevice/
- https://quickshell.org/docs/v0.3.0/types/Quickshell.Bluetooth/BluetoothDeviceState/
- https://bluez.readthedocs.io/en/latest/agent-api/
