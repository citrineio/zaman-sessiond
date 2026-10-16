# Zaman notification producers

The notification server, history, Unix-socket client and Quickshell presentation
are published in [zaman-menu](https://github.com/citrineio/zaman-menu).
Its [notification API](https://github.com/citrineio/zaman-menu/blob/main/docs/notifications-api.md)
is the canonical contract for applications and producers.

This repository owns event production close to the session/system state:

| Source | Implementation / behavior |
| --- | --- |
| Controllers | `tools/zaman-notification-events.py`: logical InputPlumber composite connect/disconnect; virtual targets excluded |
| Network | Same observer: NetworkManager terminal state and connection identity, Wi-Fi SSID/profile name |
| Bluetooth | Same observer: BlueZ adapter Powered changes and identified connected gamepad low-battery crossings |
| Audio | `experiments/quickshell/AudioNotificationEvents.qml`: optional PipeWire default-output and mute observer |
| Managed Steam | `src/notification.rs` / `src/storefront.rs`: optional Start+Select reminder once per focused native-Guide client generation |

The Python observer subscribes to InputPlumber/NetworkManager/BlueZ signals,
coalesces within a bounded 200 ms window, allows one scan at a time with a
follow-up for changes during that scan, and has a 15-second fallback. Startup,
service recovery and failed snapshots rebaseline without inventing hotplug events.
It reads state and never changes interception, pairing or network configuration.

Network identity includes SSID or the public active-profile name. Same-SSID AP
roaming is not a network-change toast. BlueZ on/off is independent of battery
telemetry. Controller battery alerts use identified connected Bluetooth controllers,
20% crossing, 30% rearm and a 15-minute cooldown; unsupported telemetry is omitted.
The optional audio observer coalesces for 500 ms and does not fill history with
volume steps.

Observer health is written to `$XDG_RUNTIME_DIR/zaman-notification-events.json`,
including timestamps, availability/errors and Bluetooth health. Delivery is
best-effort: no blocking game path and no replay of old events after service downtime.
The supplied deployment uses `zaman-notification-events.service`; inspect its actual
installed source path before changing a running device.

Dukkan sends its own verified transaction terminal events. This service does not
scrape Dukkan or Steam UIs. Transfer/import classification, achievements and
emulator-specific events are not connected producers in this checkpoint.

Existing checks:

```sh
python3 tests/test-notification-events.py
cargo test --locked --offline
```

The tests do not substitute for real NetworkManager/BlueZ/PipeWire/controller
acceptance. See the [Sprint 4 closure record](sprint4-closure.md).
