# Audio controls, network feedback and display debugging (0.6.10 source candidate)

This is an incremental source update to the previously delivered 0.6.9 Display
candidate. Apply it to that source tree, not directly to `be89bc6`. Codex has
reviewed the source diff and whitespace only; it has not compiled, installed,
executed tests, run the streaming script or generated renderer previews.
Those steps belong to the device owner. No new Rust dependencies are introduced. Audio discovery uses `pw-dump`
from PipeWire alongside the existing `wpctl`; neither tool is installed by this
patch. Streaming uses the already available WayVNC and Tailscale tools.

## Volume behavior

System > Volume opens five rows: Output, Volume, Mute/Unmute, Apply output and
Back. The slider is selected initially for immediate adjustment.
Left/Right on Output (or A) cycles playback devices without changing the
current output. Apply output makes the chosen device the WirePlumber default,
verifies active readback and refreshes its own volume/mute state. The right pane
shows the current output separately from the selected candidate. Volume and
mute controls apply to the current default until Apply output succeeds.

The list contains exposed playback sinks, not microphones, application streams
or inactive card profiles. This does not switch card profiles or forcibly move
streams pinned to a device. WirePlumber owns default-device persistence and
stream routing; verify an already running game and a newly launched game on the
device. Discovery uses JSON instead of parsing the decorative `wpctl status`
tree or requiring the newer `wpctl list` command. A device identity is checked
again before applying so a cached, recycled numeric ID is not used blindly.
The screen refreshes every three seconds while open, preserves a still-present
output candidate and replaces it when the device disappears.

Left/Right on the slider applies 5% steps immediately, bounded to 0–100%. A on the slider
confirms by returning to System; it never mutes. B or Back also returns to
System, retaining already applied changes. This is immediate adjustment,
not a preview/cancel transaction.

Mute/Unmute is an explicit action on its own row. Muting preserves the level;
the slider continues to show that level and the right pane says AUDIO MUTED.
Left/Right can adjust the stored level while muted, without unmuting. Unmute
uses the resulting stored level. Audio operations stay off the input loop and
block additional audio changes until readback completes. Back remains usable.
Re-entering Volume refreshes the default output's audio state and discards
any output selection that was not applied.

## Network behavior

Ethernet and Wi-Fi status labels are green for Connected and red for
Disconnected. Connection names/SSID and IPv4 address remain neutral beneath
the labels. Radio off counts as disconnected. Missing initial status or a
failed status refresh displays an amber Checking label rather than claiming a
connection state.

Available-network rows show the same colored Connected/Disconnected badges
on a dark background, including when selected. Automatic status refreshes
continue every three seconds while the network menu is open. Cached scan
rows retain signal/security data, but connection badges follow the actual
current SSID, including after disconnect. Renamed saved connection profiles
and multiple access points broadcasting the same SSID are handled separately
from profile names. Ordinary status refreshes use NetworkManager's cached
Wi-Fi list without requesting a rescan.

## Receive and apply (device owner)

Download `zaman-menu-0.6.10.patch` and send it to Zaman through Tailscale
Taildrop. On Zaman, receive and apply it to the tree where 0.6.9 was applied:

```bash
sudo tailscale file get --wait "$HOME"
cd ~/zaman-sessiond-publish
git apply --check "$HOME/zaman-menu-0.6.10.patch"
git apply "$HOME/zaman-menu-0.6.10.patch"
```

## Build, test and install (device owner)

```bash
command -v pw-dump wpctl
cargo build --locked --release --bin zaman-sessiond --bin zaman-menu --bin zamanctl
cargo test --locked
./install.sh
zamanctl version
```

Expected version: `0.6.10`. The installer accepts installed 0.6.7 through
0.6.10, backs up the existing binaries and prints the backup directory.
Retain that backup until acceptance is complete. Stop any active game or
transfer before running the installer.

## Acceptance on Zaman

1. From both the library and game menu, open System > Volume. Check all five
   rows, the slider, selected/current output names and right-pane instructions.
   Cycle available outputs; verify the current default does not change until
   Apply output. Apply another device, verify its own volume/mute state and
   audible output for the current game and a newly launched game. Check a
   device unplugged during selection and external/default changes while open.
2. Apply several Left/Right steps and press A on the slider. Confirm it returns
   to System with the chosen level and does not mute. Reopen Volume and verify
   readback. Check the 0% and 100% bounds.
3. Select Mute. Confirm AUDIO MUTED and the unchanged stored percentage. Adjust
   while muted, then select Unmute; verify the chosen level and audible output.
   Check A on the slider while muted returns without unmuting. Check B/Back.
4. In Network, verify connected Ethernet/Wi-Fi labels are green and disconnected
   labels are red. Check the SSID/profile name and address lines fit, and the
   selected row's description does not overlap status text.
5. Open Available Networks. Check the connected SSID's green badge and other
   rows' red badges, including on the highlighted amber row. Disconnect Wi-Fi
   and reopen the list: no old green badge should remain. Connect again and
   wait for the automatic status refresh.
6. If available, check a renamed saved profile, an SSID containing a colon or
   backslash, and a network with multiple access points. Turning Wi-Fi off
   should clear the connected indicator. Confirm Resume/Guide, game return and
   Transfer Games still behave as before.

Optional renderer fixtures, run only by the device owner:

```bash
target/release/zaman-menu --preview volume 1920 1080 /tmp/zaman-volume.bmp idle
target/release/zaman-menu --preview muted 1920 1080 /tmp/zaman-muted.bmp idle
target/release/zaman-menu --preview audio-output 1920 1080 /tmp/zaman-audio-output.bmp idle
target/release/zaman-menu --preview network 1920 1080 /tmp/zaman-network.bmp idle
target/release/zaman-menu --preview networks 1920 1080 /tmp/zaman-networks.bmp idle
```

## One-command display debugging

After applying the patch, no Rust build or installation is needed for this script:

```bash
./tools/zaman-stream
```

Run as `kadhem` on Zaman, including over SSH. It reads the imported graphical
session environment, obtains the Tailscale IPv4 address, starts a transient
user service and confirms its listener. Default capture is HDMI-A-1, 15 FPS,
port 5900. It prints the address to use in your VNC client and returns SSH to
the prompt. It preserves remote keyboard/mouse input and disables client-driven
display resizing. It does not change resolution or the boot compositor.

```bash
./tools/zaman-stream stop
./tools/zaman-stream status
./tools/zaman-stream logs
```

Select another connector with `./tools/zaman-stream start DP-1`. Optional
`ZAMAN_STREAM_FPS` and `ZAMAN_STREAM_PORT` environment variables override the
capture limit and listening port. A second start reports an already running
stream; stop before restarting with different options. The service is on-demand
only, not enabled at boot. A previously started manual WayVNC occupying the
port must be stopped first; this script stops only its own service.

The script binds to the Tailscale IPv4 address and uses the existing WayVNC
configuration. No dependency installation, firewall edits or compositor
configuration changes are performed. If the imported session socket or chosen
output is unavailable, it reports that failure instead of silently guessing a
session. Logs are retained in the user journal. Stop the stream after debugging.

Owner acceptance: start through SSH, connect from the existing phone VNC client,
open the menu, confirm the shell remains free, then stop and check the client
loses the stream. Check the missing-session/output and occupied-port messages
when convenient. Codex has not executed the script.

## Display acceptance remains open

The owner confirmed the 15-second timeout rollback remotely. Saved-mode
restoration after daemon restart and reboot still needs acceptance, along
with physical checks after returning to the device. This update carries the
existing Display feature without changing its policy or compositor launcher.
The current Sway A/B override is temporary and reboot returns to the existing
Cage default unless a persistent trial is configured separately. See
`docs/display-settings.md` for the remaining Display procedure.
