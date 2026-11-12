# Zaman Notifications API

Zaman Notifications is the notification server and presentation layer shipped in
**zaman-menu**. System-event producers live in **zaman-sessiond**; Dukkan is a
producer. There is one session-bus owner and one bounded history, shared by the
standalone toast renderer and the resident menu.

Use the standard D-Bus interface from ordinary applications. Use the local socket
for Zaman's typed producers and UI clients. The socket protocol is an experimental
integration interface without version negotiation; coordinate changes with this
repository. Persistent history format version 1 is an internal file format.

## Standard desktop notifications

| Property | Value |
| --- | --- |
| Bus | User session bus |
| Service / interface | `org.freedesktop.Notifications` |
| Object | `/org/freedesktop/Notifications` |
| Server information | `Zaman`, `Kawn Electro`, implementation `1.0`, spec string `1.2` |
| Advertised capabilities | `body` |

Implemented methods and signals:

| Name | Signature / result |
| --- | --- |
| `GetCapabilities()` | `as` |
| `GetServerInformation()` | Four strings: name, vendor, implementation version, spec version |
| `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)` | Inputs `s u s s s as a{sv} i`; result `u` notification ID |
| `CloseNotification(id)` | Input `u`; no result payload |
| `NotificationClosed(id, reason)` | Signal `u u` |

The server accepts the standard argument list but does not display supplied
application icons/images, execute actions, interpret body markup, open URLs or
play sounds. Do not rely on capabilities it does not advertise. IDs are nonzero
32-bit integers valid only for the current server generation.

### Send from a shell

With the server already running and the existing `gdbus` tool available:

```sh
gdbus call --session \
  --dest org.freedesktop.Notifications \
  --object-path /org/freedesktop/Notifications \
  --method org.freedesktop.Notifications.Notify \
  'My game' 0 '' 'Download complete' 'Ready to play' '[]' \
  "{'urgency': <byte 1>, 'x-zaman-kind': <'success'>, 'x-zaman-state': <'ready'>}" \
  3000
```

The response contains the protocol ID, for example `(uint32 1,)`. A shell command
opens a new D-Bus connection each time. Replacement and client closure are scoped
to the original sender; keep one application connection for those operations.

### Send, replace and close from Python

Uses the existing Python GIO bindings; the example installs nothing:

```python
from gi.repository import Gio, GLib

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)

def call(method, parameters):
    return bus.call_sync(
        'org.freedesktop.Notifications', '/org/freedesktop/Notifications',
        'org.freedesktop.Notifications', method, parameters, None,
        Gio.DBusCallFlags.NONE, 1500, None).unpack()

def notify(title, body, replaces=0):
    hints = {'x-zaman-kind': GLib.Variant('s', 'install')}
    args = ('My game', replaces, '', title, body, [], hints, 3000)
    return call('Notify', GLib.Variant('(susssasa{sv}i)', args))[0]

notification_id = notify('Installing', 'Preparing game files')
notification_id = notify('Installed', 'Ready to play', notification_id)
# Later, if the message is no longer relevant:
call('CloseNotification', GLib.Variant('(u)', (notification_id,)))
```

Replacing a live ID owned by the same sender updates that record, preserves its
ID and restarts its timer, even for identical text. An unknown ID or another
sender's ID creates a new notification instead. Close requests affect only that
sender's live notifications. Unknown/already-closed or other-sender close requests
are no-ops in this implementation, not an existence check.

### Hints and lifetime

| Hint | Type | Behavior |
| --- | --- | --- |
| `urgency` | byte | `2` requires acknowledgement; other values use routine behavior |
| `x-zaman-kind` | string | Icon/category from the list below; unknown values become `system` |
| `x-zaman-state` | string | Semantic state used for duration/accent |
| `x-zaman-target` | string | Only reserved `transfer-pending` is retained; all other values become empty |

Categories: `network`, `bluetooth`, `audio`, `input`, `battery`, `install`,
`success`, `warning`, `error`, `system`, `transfer`.

Use `connected` for successful controller/network/Bluetooth connections and
`enabled` for Bluetooth on: these receive muted green. Bluetooth `disabled` is
neutral. `error`, `disconnected`, and critical urgency receive the alert accent.
Titles are bounded to 160 UTF-8 bytes, body text to 1024, source to 64 and state
to 40. Control characters are sanitized; arbitrary styling is not supported.

`expire_timeout` is milliseconds: `-1` selects the default, `0` persists, positive
values are bounded to 24 hours. Defaults are 3 seconds routine and 5 seconds for
warning/error/battery or disconnected messages. Critical urgency always persists
until acknowledged, even if a positive timeout was supplied. Identical replacement
calls restart the duration. Queued routine records can expire before presentation.

Closed reason values: `1` expired, `2` dismissed, `3` client-closed, `4` removed
by retention/resource policy. A record dismissed after it already expired does
not emit a second close signal. Protocol IDs are invalid after closure/restart;
the history record UUID is a different identifier.

## Local producers and CLI

Run commands from this repository, or use the deployed client path from
`ZAMAN_NOTIFICATION_CLIENT`. The Zaman deployment also exposes
`~/.local/libexec/zaman-notifications`.

```sh
python3 tools/zaman-notifications preflight
python3 tools/zaman-notifications status
python3 tools/zaman-notifications event --json \
  '{"source":"My game","key":"level-loaded:unique-occurrence","title":"Ready","body":"Game loaded","category":"system","state":"ready"}'
python3 tools/zaman-notifications dismiss-visible
python3 tools/zaman-notifications preferences --json '{"dnd":true}'
```

An `event` requires a string `key`; the other producer fields are `source`,
`title`, `body`, `category`, `state` and `target`. No field is executable.
Deduplication uses `(source, key)`: 5 seconds for ordinary keys and 24 hours for
`transaction:` / `steam:` prefixes, within the running server's bounded cache.
Use a new occurrence key for a genuinely new event, and a stable key for duplicate
callbacks from the same occurrence. The Steam reminder preference suppresses
`steam:` keys. Deduplication is not a durable transaction or delivery guarantee.

CLI `dismiss --id UUID` takes a **history record UUID**, not a D-Bus integer ID.
Unknown commands/invalid requests fail with a JSON error or nonzero CLI exit.
Keep producer delivery bounded and best-effort; a missing notification service
must not turn a successful install or game session into a failure.

## Unix socket and UI clients

Socket: `$XDG_RUNTIME_DIR/zaman-notifications/control.sock`, default
`/run/user/<uid>/zaman-notifications/control.sock`. The directory is mode 0700,
the socket 0600, and peers must have the server's UID. Same-UID clients are
trusted local integrators, not independently authenticated applications. Do not
use the socket as a remote/untrusted ingress. At most eight watched peers are
tracked; each receives the latest snapshot rather than an unbounded backlog.

Requests are UTF-8 JSON objects, one per newline. Keep commands below 8192 bytes;
clients accept snapshots up to 256 KiB. One-shot requests receive a full snapshot
or `{"type":"error","error":"..."}` and disconnect. `{"action":"watch"}`
keeps the connection open, sends an initial snapshot and streams replacements;
commands may be sent on the same connection. The CLI's `watch` mode bridges stdin
commands and stdout snapshots for QML.

| `action` | Fields / effect |
| --- | --- |
| `snapshot` | Read current state |
| `event` | Producer fields described above |
| `dismiss` | `id`: history UUID |
| `dismiss-visible` | Dismiss the current popup IDs |
| `clear` | `ids`: captured list of history UUIDs; later arrivals are preserved |
| `preferences` | Boolean `dnd`, `steamReminder`; omitted/invalid fields stay unchanged |
| `menu` | Boolean `active`; watched peer presents the in-menu toast stack |
| `view` | Boolean `active`; watched peer is viewing history; entering marks records read |
| `renderer-status` | Watched peer only: `output`, boolean `outputPresent`; updates readiness heartbeat |

Use watched connections for `menu`/`view` so disconnection releases their state.
`renderer-status` reports `updatedAt` without rewriting history for every heartbeat.
UI actions return/stream snapshots, not an action invocation API for applications.

A snapshot contains `type:"notifications"`, generation string, revision integer,
`records`, `visible`, unread count, `menuOpen`, `preferences`, error string and
an optional renderer status object. Records expose UUID `id`, integer `protocolId`,
`generation`, timestamp, source/title/body/category/state/target, urgency,
read/dismissed/live flags and popup state. Internal fields beginning `_` are hidden.

`records` may retain dismissed internal history; the menu filters dismissed rows.
Do not interpret popup expiry as deletion from history. Use UUIDs to preserve
selection through replacements and animations. A server restart gives a new
generation; never reuse previous protocol IDs as live IDs.

## Presentation, storage and connected producers

Two popups are visible at once; the routine queue is limited to 20. Critical
alerts can preempt a routine popup. At 100 simultaneously unacknowledged criticals,
a new notification is rejected instead of evicting an existing critical. DND
suppresses routine popups while retaining history; critical alerts remain visible.

History defaults to `~/.local/state/zaman/notifications.json`, using
`$XDG_STATE_HOME` when set. It is bounded to 100 records / seven days / 256 KiB,
with active critical protection, and written atomically with private permissions.
Old critical records are archived on restart and are not replayed as live alerts.

The standalone renderer targets `ZAMAN_TOAST_OUTPUT` (default `HDMI-A-1`), hides
when that output is absent, and never takes keyboard focus or a mouse hit region.
While the menu is open, its embedded toast stack presents the same popup state.
`ZAMAN_AUDIO_EVENT_QML` may load the optional sessiond-owned audio observer.

| Producer | Events |
| --- | --- |
| Sessiond read-only InputPlumber observer | Controller connected/disconnected |
| Sessiond NetworkManager observer | Connect/disconnect/failure/network identity change, including SSID/profile name |
| Sessiond BlueZ observer | Adapter on/off and identified connected controller battery threshold |
| Optional Quickshell PipeWire observer | Default output and mute/unmute; no volume-step history |
| Dukkan | Verified install/remove terminal result or failure/stoppage |
| Managed native-Guide Steam | Optional Start+Select reminder once per focused client generation |
| Desktop applications | Standard `Notify` calls |

The `transfer-pending` target is reserved information, not a connected import
queue or clickable command. Achievements, emulator events, download-progress
notifications and general Steam-activity monitoring have no custom producer here.

## Running, testing and source references

With the existing graphical/session environment, `python3 tools/zaman-notifications
serve` starts the server and renderer. `serve --no-renderer` is for isolated
protocol tests. The supplied user service template assumes the Zaman deployment
path; inspect/adapt it rather than replacing another desktop notification daemon.
The server requests its D-Bus name without queueing or replacing another owner.

```sh
python3 tests/test-notifications.py
python3 tests/test-notification-renderer.py
dbus-run-session -- env ZAMAN_PRIVATE_NOTIFICATION_TEST=1 python3 tests/notification-protocol.py
```

Definitions: [server](../notifications/server.py), [model](../notifications/model.py),
[transport](../notifications/transport.py), [CLI](../tools/zaman-notifications), and
[standard desktop protocol](https://specifications.freedesktop.org/notification/latest/protocol.html).
Zaman-specific behavior above describes this implementation and its current limits.
