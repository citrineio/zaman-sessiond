# Zaman Menu

The controller-operated system menu and notification center for the
[Zaman retro console](https://zaman.kawnelectro.com/), developed by
[Kawn Electro](https://kawnelectro.com/).

Open the menu over your game or library to adjust settings, read notifications,
or return to your games. The renderer and input helper stay resident between
openings, so the menu can reuse its connections and controller inventory.

This is working prototype software for Zaman's Linux/Sway environment. It is
published for development and integration; it is not a general desktop shell.

## What it does

- **Home:** resume, exit a managed game, open Dukkan or Steam, and reach system actions.
- **Notifications:** view history, dismiss individual items, clear captured history,
  enable Do Not Disturb, and control the Steam shortcut reminder. Queued/running
  Dukkan jobs appear as live activity; transaction results open their relevant view.
- **Settings:** audio output and mute, network connections, Bluetooth devices,
  controllers, and display resolution/refresh with a timed Keep/Revert preview.
- **Controller navigation:** animated tabs, positional button icons, shoulder
  navigation through sections, and remembered locations within each tab.
- **Recovery:** controller loss stops held navigation and shows a warning while
  keeping the menu open. Closing restores the input/focus/display lease. Guide
  restoration uses current ownership rather than blindly restoring a saved mode.
- **Toasts:** compact passive notifications, including while the menu is open.
  The menu and standalone toast renderer share one server and one history.

Game launching, emulator shutdown/autosave and storefront supervision belong to
[zaman-sessiond](https://github.com/citrineio/zaman-sessiond). This UI requests
those actions through the session interface rather than supervising games itself.

## Controls

The on-screen legend is contextual. Face-button icons indicate physical positions
rather than assuming the letters printed on a particular controller.

| Input | Action |
| --- | --- |
| Guide | Open/close Zaman's menu when Zaman owns Guide |
| Start + Select | Existing fallback shortcut, also used when Steam owns Guide |
| D-pad / arrows | Navigate; left/right adjusts supported controls |
| South face / Enter | Select |
| East face / Escape | Back; close at a top-level section |
| Shoulder buttons / Ctrl+Page Up or Page Down | Change tabs outside blocked dialogs/editing |
| West face / X | Dismiss the selected notification or visible popups |
| North face / Y | Clear notification history, with confirmation |

Native-Guide Steam retains its own Guide behavior. No global gameplay face-button
shortcut is introduced for dismissal; open Zaman's menu first. Text entry and
confirmation screens can reserve their own controls.

## Running on Zaman

The supported environment uses Quickshell 0.3, Sway/Wayland, InputPlumber,
Python 3 with GIO, and a matching zaman-sessiond checkout with the built input and
pairing helpers. Audio uses Quickshell's PipeWire module; networking and Bluetooth
use the system services already configured by Zaman.

With those components already available:

```sh
python3 apply.py /absolute/path/to/zaman-sessiond
cd /absolute/path/to/zaman-sessiond
cargo test --locked --offline --bin zaman-menu-dev-input
cargo build --release --locked --offline --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
cd /absolute/path/to/zaman-menu
sh run.sh /absolute/path/to/zaman-sessiond --resident
```

This starts the hidden resident renderer. It does not install dependencies or
configure the complete Zaman session. The paired deployment and rollback notes
are in [the resident-menu guide](RESIDENT-MENU.md). Older instructions there and
in the history describe earlier prototypes; the current Guide ownership and
notifications notes are in [Sprint 4](docs/sprint4-notifications.md).

## Notifications and application integration

**Zaman Notifications is part of this repository.** It contains the Python server,
notification history, socket client and Quickshell presentation. It implements the
standard `org.freedesktop.Notifications` session-bus interface, plus a local API
for Zaman producers and UI clients.

Start with the [notification API](docs/notifications-api.md) for working D-Bus,
Python and command-line examples, supported hints, expiry, replacement and dismissal.
The service refuses to replace another desktop's notification server.

System event producers are in zaman-sessiond. Dukkan sends terminal install/remove
results through a durable ordered outbox; the notification server saves its receipt
before acknowledging. Lost ACKs, clear and dismissal do not create duplicate
terminal records. Progress is a live view rather than percentage-history updates.
System-event notifications remain separate from game/session lifecycle success.
See [Sprint 5 transactions and branding](docs/sprint5-transactions.md).

## Diagnostics and tests

Direct diagnostics are available without journalctl or Quickshell IPC:

```sh
python3 tools/zaman-notifications preflight
python3 tools/zaman-notifications status
tail -n 80 "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/zaman-menu-timing.log"
```

The timing file records menu acquisition, close origins and restored input modes.
The supplied notification service writes `zaman-notifications.log` in the same
runtime directory. Never commit notification history or device logs.

Source-level checks with existing tools:

```sh
for test in tests/*.cjs; do node "$test" || exit; done
python3 tests/test-notifications.py
python3 tests/test-notification-renderer.py
dbus-run-session -- env ZAMAN_PRIVATE_NOTIFICATION_TEST=1 python3 tests/notification-protocol.py
```

Run the D-Bus test only in its private session. JavaScript/model tests do not
replace Qt rendering or physical controller testing. See the
[Sprint 4 closure record](docs/sprint4-closure.md) for the acceptance boundary.

## Related projects

- [Zaman Sessiond](https://github.com/citrineio/zaman-sessiond): sessions, input and system-event producers.
- [Dukkan](https://github.com/citrineio/dukkan): Zaman's curated native-game storefront.
- [Zaman website](https://zaman.kawnelectro.com/): the console this software is built for.

Bug reports and integration questions are welcome in this repository's Issues.
Historical prototype notes are retained under [docs/history](docs/history).
No new code license is selected by this publication; existing third-party asset
and dependency licenses retain their own terms.
