# Sprint 4 candidate: tabs and notifications

This is a source candidate, not device acceptance. Pair with sessiond's Sprint 4
producer changes and Dukkan's terminal transaction event change. Preserve the
Sprint 3.5 shared catalog and installed-game/session contracts.

## Controls

The first opening starts on Home. Positional shoulders wrap Home / Notifications /
Settings from every menu section, including Audio, forms, details and confirmation
pages. Ctrl+PageUp/PageDown are keyboard equivalents. A held shoulder switches
once. Each tab remembers its section, parent route, selection and list scroll;
Home Audio -> Notifications -> Home returns to Audio. Audio controls and output
candidate are restored independently for Home and Settings. The resident menu
also remembers its last tab/section across closing and reopening; a renderer
restart starts on Home. Memory stays in the resident process and is not written
to system settings.

A display preview reverts on leaving Display, and Bluetooth hiding stops scanning
and cancels an active pairing prompt. Switching tabs preserves the network view
and a password draft while this menu opening is alive; closing releases the
native loaders and discards secrets. Reopening a closed menu returns to a
confirmation's parent, never an old destructive confirmation. Only an actual
exit/power request in flight temporarily blocks tab changes. Home retains
context-sensitive game/storefront routes, quick Audio and Power. Settings owns
Audio and the configuration pages.

Outside Notifications and text entry, X dismisses currently visible popups, keeping their internal history. Notifications
X dismisses and hides the selected record from the list; Y opens clear-history confirmation, defaulting
to Cancel. Keyboard X/Y do the same outside text entry. A opens a full detail
view; Up/Down scrolls long detail; B returns. New records cannot move the stable
selected ID or change an open detail/confirmation. Clear operates on the captured
ID set, so arrivals after opening the confirmation survive. Clearing never
cancels transactions or resolves pending imports. Opening Notifications marks
records read and marks arrivals read while browsing. Popups remain visible in
all menu sections, including Notifications; DND still suppresses routine alerts.
Switching presentation between the menu and game does not restart deadlines or
replay expired alerts. A shared keyed toast stack renders above the menu contents;
the standalone passive surface yields while the menu connection reports open.
Disconnecting that connection releases the presentation flag. Connected controller/network and Bluetooth-on
alerts use a muted green (#8AB9A1) icon/accent; Bluetooth-off is neutral blue-gray, with disconnect/error warnings
retaining their warning color.

The notification actions follow the effective controller mapping observed on the
user's device: ui_context dismisses (reported X), ui_action clears (reported Y).
Keyboard X/Y remain explicit dismiss/clear shortcuts. The keyboard editor retains
its previous face-event behavior; no global controller profile is modified. The paired helper adds
only ui_l1/ui_r1 to its allowlist; input ownership and Guide policy are unchanged.
Gameplay dismissal remains Guide then X (Steam: Start+Select then X). No gameplay
face button or new global chord is intercepted.

## One protocol owner and one history

`notifications/server.py` uses existing Python GIO to own
`org.freedesktop.Notifications`. `notification-shell.qml` reuses the trial's
layer-shell renderer pattern and menu icon component; it owns no notification
name. Both it and the resident menu consume the same bounded JSON snapshots over
a mode-0600 socket in the user runtime directory. `tools/zaman-notifications`
is the diagnostic client and QML bridge. Same-UID local producers use that socket;
notification clients use the standard bus protocol.

The earlier Quickshell 0.3.0 notification API did not reliably announce identical
replacement calls. Direct protocol handling makes every Notify call restart its
timer, including identical replacements; Quickshell no longer implements expiry.
Advertised capabilities are only `body`. Actions, markup, remote images, sounds
and persistence of client actions are not advertised. Native close reasons are
1=expired, 2=dismissed, 3=client-closed, 4=retention/resource removal. A historical
dismissal sends no second close signal. Live replacement/closure is scoped to the
originating D-Bus sender. Notifications never execute incoming commands or URLs.

Defaults: 3 seconds routine, 5 seconds warnings, 2 visible cards, 20 queued normal
cards. Explicit timeout values are milliseconds (0=persistent, -1=default).
Critical urgency always requires acknowledgement, even with a positive timeout.
Critical alerts preempt a routine popup, never another critical popup. Overflow
stays in history; normal clients may subsequently expire. At the hard budget of
100 simultaneously unacknowledged criticals, a new Notify is explicitly rejected
with an error rather than evicting an existing critical. Persisted older criticals
are archived on restart and never replayed.

History retains 100 records / seven days, with a 256 KiB file cap, bounded UTF-8
text and atomic mode-0600 writes at `$XDG_STATE_HOME/zaman/notifications.json`
(default `~/.local/state/zaman/notifications.json`). Old protocol IDs are never
live after restart. Corrupt/unsupported history starts empty with a diagnostic
error. Unacknowledged live criticals remain until acknowledged even beyond the
age budget. DND suppresses routine popups but retains history. Settings >
Notifications controls DND and the Steam shortcut reminder. `transfer-pending` is
a reserved typed informational hint; the actual Transfer backend is deferred.

The configured output defaults to HDMI-A-1. An absent output hides the passive
panel and retains history; it does not move alerts onto another display. Cards
are 420 design pixels wide with one title/one supporting line, local vector
icons, brief entry animation, no keyboard focus, empty mouse hit region and no
reserved area. Actual TV readability, compositor focus/input and 1080p/1440p
behavior require device checks. Stable card IDs preserve replacements without re-entry; entry and exit animate
over 120 ms while the visible stack remains bounded to two cards.

## Real producers

Sessiond owns read-only controller/network/battery event logic in
`tools/zaman-notification-events.py`, running in a separate user service. Events
coalesce within a bounded 200 ms window; startup, owner restart and failed snapshots rebaseline
without synthetic hotplug notifications. Logical composite identities exclude
virtual target pads. Controller events have a five-second same-event cooldown. Confirmed network
and Bluetooth transitions use unique IDs so rapid A/B/A or off/on changes are
not discarded.
Network terminal connection identities survive transitional states so deactivation
is reported once and switching networks is recognized even if the interface
remains connected. Wi-Fi names come from active-AP SSID bytes; active-profile
names are the fallback and identify wired connections. Roaming between APs of
the same SSID does not generate a network-change toast. Battery alerts use explicit connected Bluetooth gamepad/joystick telemetry,
20% crossing, 30% rearm and 15-minute cooldown. BlueZ adapter Powered changes
produce Bluetooth on/off alerts; adapter discovery and owner recovery do not. Unknown, wired and unclassified
telemetry is omitted. Other reliable battery transports can be added later.

The optional sessiond-owned `AudioNotificationEvents.qml` module observes actual
PipeWire default output and mute changes through the existing Quickshell module,
with 500 ms coalescing. Missing audio support cannot take down the server/menu.
Volume steps remain visible in Audio and do not create persistent notifications;
this candidate does not introduce a separate global volume OSD.

Dukkan emits once at its verified terminal transaction outcome (success requires
normal relay exit and its success result). A stopped/failed transaction is not
reported as success; detailed progress stays in Dukkan. It uses the standard
client symlink or explicit ZAMAN_NOTIFICATION_CLIENT, with a bounded asynchronous
child lifetime. Sessiond emits the optional Steam reminder once per managed
native-Guide client generation after an owned client window is focused. Returning
from games/menu does not resend it. Producer delivery is best-effort and bounded;
notification-service downtime does not block gameplay or replay old events later.

## Validation boundary

Pure Python model tests and source-extracted JavaScript tests cover protocol
state, retention, expiry/replacement, priority, read/clear semantics, navigation,
editing guards, Guide lifecycle, entry animation and storefront actions. The
private D-Bus integration test, Rust compilation/tests, C++ compilation/tests and
actual QML/compositor behavior must pass on Zaman before acceptance. See the
paired delivery README for exact preparation, activation and rollback commands.

The toast entry lives at the menu repository root so both `qml/` and
`notifications/` are inside the Quickshell configuration boundary. The optional
audio producer is copied from its canonical sessiond source into deployed qml/
during activation. Readiness uses renderer heartbeats exposed by the direct
Unix-socket status command; Quickshell IPC is not an activation requirement.

## Menu feedback update

The tab strip fills the entire header. Inactive tabs retain their existing base
widths; the active tab receives the remaining space. Widths animate for 180 ms
with OutCubic easing, with 120 ms color changes. Narrow layouts proportionally
fit the base widths without overflowing. Local positional bumper glyphs replace
L1/R1 text, matching the footer's positional face-button language. Keyboard hints
remain Ctrl+PgUp/PgDown.

Dismissed records disappear from the list; the server retains bounded internal
closure state/history. Stable ListModel IDs preserve remaining rows and select
the next neighbor (or preceding last item). Removed rows slide right by 96 scaled
pixels and fade over 180 ms with OutCubic easing; neighbors close the gap over
180 ms. Exiting rows stop accepting clicks. Popup dismissal already slides and
fades over 120 ms. Directional footer hints use arrows alone for navigation,
adjustment and text entry, on controller and keyboard. Face-button diamonds
remain for actions (Select, Back, Dismiss, Clear), with no redundant arrow suffix.
Clear captures all current record IDs, including
hidden dismissed records, while preserving new arrivals. The notification
preferences page has its own descriptive heading/subtitle. The Steam reminder
setting explains its Start+Select purpose.

## Notification input inventory

These are the configured producers; live hardware/service availability must be
checked on the device. The observer reports controllers/network/battery health to
`/run/user/UID/zaman-notification-events.json` with timestamps and errors.

| Input | Current events |
| --- | --- |
| InputPlumber system bus, composite inventory | Real logical controller connected/disconnected; virtual targets excluded |
| NetworkManager system bus, wired/Wi-Fi device state | Connected, disconnected, connection failed and network changed, with Wi-Fi/profile name |
| BlueZ system bus, adapter Powered and identified connected Bluetooth gamepads with Battery1 | Bluetooth on/off; low battery crossing 20%, rearm at 30%, 15-minute cooldown |
| PipeWire through native Quickshell producer | Default output changed; muted/unmuted; no volume-step history |
| Dukkan transaction callback | Install complete, removal complete, stopped/failed install or removal |
| Sessiond managed Steam client | Optional Start+Select reminder once after an owned client is focused |
| org.freedesktop.Notifications session-bus API | Notify/replace/close from any application that sends standard desktop notifications |

The read-only system observer subscribes to InputPlumber/NetworkManager/BlueZ
signals, coalesces within a bounded 200 ms window and refreshes at most one scan at a time, with a
15-second fallback. Initial/recovery discovery is not a hotplug event. The audio
producer coalesces for 500 ms. Dukkan and the Steam reminder push terminal events;
the service does not scrape their UIs or subscribe to general Steam activity.
Transfer/import classification events are a reserved schema, not a connected
backend yet. Game achievements, download progress and emulator events have no
custom producer in this sprint. Applications can still send standard Notify calls.

## Connectivity and display corrections

The observer now compares active connection/SSID identity as well as device
state, includes the SSID (or public active-profile name) and retains the previous
network name on disconnect. Signal storms cannot reset the scan timer forever. A change arriving during a
scan requests a follow-up snapshot instead of waiting for the 15-second fallback.
Bluetooth adapter Powered changes are observed independently from batteries.
Only confirmed snapshot transitions generate events; startup/recovery rebaseline.

Display choices deduplicate exact width/height/refresh triples. Duplicate Sway
mode entries could otherwise keep Right selecting an identical first match.
Both directions now wrap through distinct rates. Fractional rates remain distinct;
preview/rollback and helper contracts are unchanged.
