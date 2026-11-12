# Quickshell toast trial

This is an on-demand experiment alongside the working 0.6.12 menu and the
planned 0.7.0 validation. It adds no Rust dependencies, does not replace the
menu, and does not change Sway boot configuration or enable any service.
The owner performs package installation, execution and acceptance testing.

## What is included

- A standard desktop notification server using Quickshell's
  `Quickshell.Services.Notifications` module.
- Up to three toasts at the upper right of a selected Sway output, using
  Zaman's existing fonts and colors. Slide/fade entry and exit take 180 ms;
  remaining cards move smoothly when a card disappears.
- An overlay layer above fullscreen windows, an empty mouse input region,
  no keyboard focus, and no reserved desktop space. Verify on the Dragon
  that toasts do not interrupt gameplay or trigger pause on focus loss.
- Plain-text titles/bodies, notification replacements, client-requested
  closure, and client timeouts.
  Default normal notifications last six seconds. Explicit zero timeouts
  and critical notifications with default timeout remain until closed or
  displaced. A fourth notification expires the oldest to bound the stack.
- No history, notification actions, inline replies, images, blur or sound
  in this trial. The NET/AUD/PAD badges and green/red/amber accent are driven
  by optional `x-zaman-kind` and `x-zaman-state` hints.
- A helper to start/stop the trial from SSH without occupying the terminal,
  and to send three explicitly marked demo notifications. These are simulated
  network/audio/controller events; automatic system-event monitoring comes
  after renderer and focus behavior are accepted.

## Dependencies: owner installs

Debian lists an ARM64 Quickshell package in **trixie-backports** (0.3.0 at
the time of preparation). Use that package and its matching Qt dependencies.
The QML targets Quickshell 0.3.0 APIs. First inspect your available packages:

```sh
apt-cache policy quickshell libnotify-bin qt6-wayland
```

If trixie-backports is not configured, add the official Debian suite:

```sh
printf '%s\n' \
  'Types: deb' \
  'URIs: https://deb.debian.org/debian' \
  'Suites: trixie-backports' \
  'Components: main' \
  'Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg' | \
  sudo tee /etc/apt/sources.list.d/zaman-trixie-backports.sources >/dev/null
sudo apt update
```

Install the renderer and the runtime modules used by this experiment:

```sh
sudo apt install -t trixie-backports quickshell
sudo apt install libnotify-bin qt6-wayland qml6-module-qtqml \
  qml6-module-qtqml-models qml6-module-qtqml-workerscript
quickshell --version
```

If apt cannot resolve this, stop and retain its output for review. Do not
switch the base system to Debian testing/unstable for this trial.

## Receive and apply

Download `zaman-quickshell-toast-trial.patch`, send through Tailscale Taildrop,
then receive and apply it to the existing source checkout:

```sh
sudo tailscale file get --wait "$HOME"
cd ~/zaman-sessiond-publish
git apply --check "$HOME/zaman-quickshell-toast-trial.patch" &&
git apply "$HOME/zaman-quickshell-toast-trial.patch"
```

No Rust rebuild or `install.sh` run is needed. The experiment reads its QML
and existing fonts directly from the checkout.

## Start, demo and stop

```sh
./tools/zaman-toasts start HDMI-A-1
./tools/zaman-toasts demo
```

The helper uses the graphical environment imported into the user systemd
manager, validates the selected Sway output and starts a transient
`zaman-toast-trial.service`. Type=dbus waits for notification bus ownership
before reporting startup. If another notification daemon already owns that
bus name, it reports the conflict; it does not stop or replace that daemon.

```sh
./tools/zaman-toasts status
./tools/zaman-toasts logs
./tools/zaman-toasts stop
```

Stopping closes only the trial. The service is not enabled for boot. Start
it again explicitly after reboot; if Sway is not restored, the helper reports
that rather than starting an incompatible graphical session.

An individual notification can be sent with the standard client:

```sh
notify-send -a Zaman -t 6000 \
  -h string:x-zaman-kind:network -h string:x-zaman-state:connected \
  'Network ready' 'DEMO: Toast rendering check.'
```

## Owner acceptance

1. Send demo notifications in Pegasus, the existing menu and a running game.
   Confirm readable text, contrast, correct placement, stacking and animation.
2. During gameplay, send a notification over SSH. The game must keep running,
   retain keyboard/controller input and not pause because of focus loss.
   Mouse clicks on a toast must pass through to the window beneath it.
3. Check 1080p/1440p and any intended TV scale. Disconnect/reconnect the chosen
   output; the stack should hide while that output is absent and return there
   when it is available. It should not silently migrate to another output.
4. Send more than three notifications; verify the oldest disappears and the
   stack remains bounded. Confirm normal expiry, replacement and sender close.
5. Test replacement without a second card (owner command below). The latest
   title/body should replace the earlier content and restart its timeout:

   ```sh
   ZAMAN_TOAST_ID=$(notify-send -p -a Zaman -t 10000 'First message' 'Replacement test')
   notify-send -r "$ZAMAN_TOAST_ID" -a Zaman -t 6000 'Updated message' 'This should replace the first card.'
   ```

6. Stop and restart the helper. Confirm the existing menu, compositor and game
   supervision still work, and inspect the journal for QML/runtime errors.

The source has been reviewed and the patch mechanically checked. It has not
been installed, compiled, rendered or executed by the assistant. Device
acceptance is pending; this experiment is not yet part of the 0.7.0 release.

## Version-specific source findings

The v0.3.0 C++ implementation stores the D-Bus `expireTimeout` directly in
milliseconds, even though its generated type page describes seconds. The
trial uses milliseconds, matching `notify-send -t 6000` to six seconds. This
was verified in the pinned upstream `notification.cpp`, not by runtime tests.

Replacements update the existing object without re-emitting the server's
notification signal. The trial listens to property changes to update the
card and reset its timeout. An entirely identical replacement emits no
property change, so its existing timeout continues in this initial trial.

## References used for the trial

- Debian ARM64 package: <https://packages.debian.org/trixie-backports/arm64/quickshell>
- Notification server: <https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Notifications/NotificationServer/>
- Notification lifetime: <https://quickshell.org/docs/v0.3.0/types/Quickshell.Services.Notifications/Notification/>
- Panels: <https://quickshell.org/docs/v0.3.0/types/Quickshell/PanelWindow/>
- Layer selection: <https://quickshell.org/docs/v0.2.0/types/Quickshell.Wayland/WlrLayer/>
- Input region: <https://quickshell.org/docs/v0.3.0/types/Quickshell/QsWindow/>
- Pinned lifetime implementation: <https://github.com/quickshell-mirror/quickshell/blob/v0.3.0/src/services/notifications/notification.cpp>
- Pinned replacement implementation: <https://github.com/quickshell-mirror/quickshell/blob/v0.3.0/src/services/notifications/server.cpp>

## Sprint 4 ownership

This trial is superseded by `zaman-menu/notifications/`. Its renderer and server
are no longer duplicated here. The compatibility `tools/zaman-toasts` targets
`zaman-notifications.service`. Historical trial observations above describe the
old six-second, three-card prototype, not Sprint 4. The new GIO protocol owner
handles every Notify call (including identical replacements), while Quickshell
only renders snapshots. Real controller/network/battery producers live in
`tools/zaman-notification-events.py`; audio events use the optional read-only
`experiments/quickshell/AudioNotificationEvents.qml` producer loaded by the
renderer. Unsupported/missing telemetry produces no invented battery values.
