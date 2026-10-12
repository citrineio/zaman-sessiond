# Bluetooth and inline mute (0.6.11 source candidate)

Apply this incremental patch to the previously delivered 0.6.10 source. System now includes Bluetooth. The
Volume screen has four visible rows: Output, Volume with an inline mute button,
Apply output and Back. The slider is selected initially.

Navigation follows the visual layout. Right from the slider selects the speaker
icon beside the percentage; Left from the icon returns to the slider. A on
that icon mutes or unmutes. Up/Down goes between Output, Volume, Apply output
and Back; from either the slider or the icon, Up goes to Output and Down goes
to Apply output.

A on the slider enters adjustment, with ADJUSTING VOLUME guidance. Left/Right
then applies the existing 5% volume steps. A confirms and returns to System.
B finishes adjustment and stays on Volume so Right can select mute; changes
already applied are retained. Up/Down also finishes adjustment and moves to
the adjacent visible row. Outside adjustment, B retains the existing Back
behavior. Guide retains the session's existing menu-close behavior. The footer
and right pane explain the active mode. A fresh menu opening starts in navigation;
changing the default output or losing audio status also ends adjustment.

The icon has its own amber focus highlight. Unmuted audio shows a speaker with
sound waves; muted audio shows a red cross next to the speaker. Volume remains
visible while muted. Symbols are drawn directly with the renderer and scale
with the output resolution, requiring no additional fonts or assets.

## Bluetooth controls

System > Bluetooth offers power On/Off, Devices and Find devices. Discovery
runs for 30 seconds and can be stopped from the Bluetooth home page. Leaving
Bluetooth or closing the menu stops this menu's discovery session. Other
applications' discovery sessions are not stopped.

Devices includes previously paired and newly discovered devices. Three devices
appear per page; Left/Right or A on Page changes the page. Connected devices
have green labels and disconnected devices have red labels. The detail pane
shows the address, paired status and battery percentage when BlueZ reports it.
Status refreshes every three seconds while Bluetooth is open; a read failure
shows Unavailable instead of claiming a cached connection is current.

Select a device to Pair, Connect/Disconnect or Forget. Pairing supports matching
code confirmation, PIN entry, six-digit passkey entry and displaying a code to
type on a Bluetooth keyboard. The existing controller keyboard provides input;
select Confirm when ready. Confirmation defaults to Cancel, and B cancels the
active request. Forget opens a separate confirmation that also defaults to Cancel.
Successful pairing remembers/trusts that selected device; Connect explicitly
requests its available profiles. Bluetooth audio outputs appear in the existing
Volume output selector when PipeWire/WirePlumber exposes them.

The client uses BlueZ's system-bus interfaces and an application-scoped
KeyboardDisplay pairing agent. It accepts callbacks only from BlueZ for the
selected device and never registers as the system default agent. No new Rust
dependencies are needed. BlueZ is already installed; its running service, a
working adapter, user authorization for its D-Bus operations and the relevant
audio/input profiles must be present on the device. No service, policy or boot
configuration is changed by this patch. With multiple adapters, the first
powered adapter is used (otherwise the first adapter); adapter selection is
not included in this candidate.

Codex reviewed source and patch mechanics only. It did not compile, install,
execute tests or render previews. The owner performs those steps.

## Receive, apply and build (device owner)

Download `zaman-menu-0.6.11.patch` and send it to Zaman through Tailscale
Taildrop. Also download the 0.6.11 build-fix and navigation-fix patches. For a
source tree that still contains 0.6.10, apply all three in order before building:

```bash
sudo tailscale file get --wait "$HOME"
cd ~/zaman-sessiond-publish
git apply --check "$HOME/zaman-menu-0.6.11.patch"
git apply "$HOME/zaman-menu-0.6.11.patch"
git apply --check "$HOME/zaman-menu-0.6.11-build-fix.patch"
git apply "$HOME/zaman-menu-0.6.11-build-fix.patch"
git apply --check "$HOME/zaman-menu-0.6.11-navigation-fix.patch"
git apply "$HOME/zaman-menu-0.6.11-navigation-fix.patch"
cargo build --locked --release --bin zaman-sessiond --bin zaman-menu --bin zamanctl
cargo test --locked
```

If 0.6.11 and its build fix are already applied, apply only the navigation fix
then repeat the build and tests. Do not reapply earlier patches.

If the build and tests pass, with no game or transfer active:

```bash
./install.sh
zamanctl version
```

Expected version: `0.6.11`. Retain the installer's printed backup directory
until acceptance. Existing audio/output and network acceptance is described
in `docs/menu-feedback-0.6.10.md`; the standalone Mute row in those historical
instructions is now this inline button. Display tests remain pending as
recorded in `docs/display-settings.md`.

The owner supplied screenshots showing the menu surfaces, network status
colors, Bluetooth adapter On/status and the inline mute icon. Extensive
Bluetooth, audio and persistence testing remains pending. The horizontal
navigation change below still needs owner compilation and device validation.

## Audio acceptance

1. Open System > Volume. Confirm four visible rows and a speaker button beside
   the slider/percentage with no separate Mute cell. Check the layout at the
   current display resolution and later at the alternate test resolution.
2. Press A on the slider to enter adjustment, adjust with Left/Right and
   confirm with A. Verify confirmation returns to System without muting.
   Check 0%/100% and the percentage spacing beside the icon. Reopen and
   press Right without entering adjustment: it must focus mute without
   changing the volume. Left returns to the slider without changing it.
3. Press Right to focus the speaker button and press A. Confirm the
   red cross, AUDIO MUTED status and preserved percentage. Press A again to
   unmute. Left from the button must return focus without changing volume.
4. Check Up/Down navigation through the four visible rows: Output, Volume,
   Apply output and Back. From either slider or mute, Down must go to Apply
   output and Up to Output. While adjusting, B must retain the level and
   stay on Volume, with Right then reaching mute. Up/Down must also leave
   adjustment and move rows. Confirm a new menu opening starts in navigation.
   Verify output selection and applying another output still updates that
   output's volume/mute state. Check the highlighted icon and volume row are
   visually distinguishable, including while muted.
5. Repeat the interaction from the game menu and confirm Resume/Guide behavior.

## Bluetooth acceptance (device owner)

1. Open System > Bluetooth from library and game contexts. Verify the adapter
   identity and On/Off status. Toggle power and check that device status follows.
2. Put a spare controller, keyboard or headphones in pairing mode. Choose Find
   devices, inspect device names and addresses, and verify the list updates.
   Check pagination if more than three devices appear. Confirm discovery stops
   after 30 seconds and on leaving Bluetooth/closing the menu.
3. Select an unpaired device and Pair. Exercise the prompt supported by that
   device: compare codes on both sides, enter a PIN/passkey or type the displayed
   code on the keyboard. Confirm only a matching device/code. Check leading
   zeroes in a six-digit code. Confirm Cancel is selected initially.
4. Cancel an active request with B, return to System or close/reopen the menu
   while pairing/discovery is active. Verify an old request cannot bring up a
   prompt in a new menu or change the currently selected device. Try a rejected
   code or device timeout and confirm the menu remains usable.
5. Check Paired status, Connect/Disconnect and green/red connection labels.
   Verify a paired controller is usable through the existing input setup and
   connected headphones appear under Volume > Output when the audio stack
   exposes them. Selecting an output still requires Apply output.
6. Confirm Forget defaults to Cancel, then deliberately forget the spare device
   and check its paired status clears. Check that a vanished selected device
   returns to Devices/Back instead of selecting a different device silently.
7. Reboot and check remembered pairing and connection status after reopening
   the menu. Actual automatic reconnection follows BlueZ/device/profile behavior;
   this patch does not add an automatic connection policy.
8. If Bluetooth fails, inspect `systemctl status bluetooth --no-pager` and
   `journalctl --user -u zaman-menu.service -n 80 --no-pager`. This candidate
   reports failures; it does not install or change authorization policies.

Optional production-renderer fixtures, run only by the owner:

```bash
target/release/zaman-menu --preview volume 1920 1080 /tmp/zaman-volume.bmp idle
target/release/zaman-menu --preview volume-edit 1920 1080 /tmp/zaman-volume-edit.bmp idle
target/release/zaman-menu --preview muted 1920 1080 /tmp/zaman-muted.bmp idle
target/release/zaman-menu --preview mute-icon 1920 1080 /tmp/zaman-mute-icon.bmp idle
target/release/zaman-menu --preview system 1920 1080 /tmp/zaman-system.bmp idle
target/release/zaman-menu --preview bluetooth 1920 1080 /tmp/zaman-bluetooth.bmp idle
target/release/zaman-menu --preview bluetooth-devices 1920 1080 /tmp/zaman-bluetooth-devices.bmp idle
target/release/zaman-menu --preview bluetooth-pair 1920 1080 /tmp/zaman-bluetooth-pair.bmp idle
target/release/zaman-menu --preview bluetooth-pin 1920 1080 /tmp/zaman-bluetooth-pin.bmp idle
```

For remote checks, start `./tools/zaman-stream`. Stop with
`./tools/zaman-stream stop` when finished.
