# Display settings source candidate (0.6.9)

Based on `be89bc6` on `test/zaman-0.6.8`. Compilation, test execution,
installation and hardware acceptance are left to the device owner. This source
candidate has not been built or tested by Codex.

## Validation progress (2026-10-06)

The device owner confirmed the 15-second timeout rollback remotely. Saved-mode
restoration after sessiond restart and reboot remains unconfirmed, and further
physical display checks remain pending. The current source candidate is 0.6.11;
use `docs/menu-bluetooth-0.6.11.md` for its build/install commands. The original
0.6.9 procedure below is retained as release history. The Sway trial remains a
runtime A/B override; this patch does not make it the boot default.

## Behavior

System now contains Volume, Network, Display and Back. Display shows the active
connector, monitor identity and current mode. Left/Right (or A) cycles the
display, resolution or refresh row. Modes come from Sway's advertised list;
refresh is stored in millihertz and displayed with three decimals. Choosing a
resolution retains the selected refresh when possible, otherwise offers the
closest advertised rate for that resolution. Selecting a row only edits the
candidate; Apply is the first operation that changes the output.

Apply requests the chosen mode and reads it back from Sway. A verified preview
offers Keep changes and Revert, with Revert selected by default. The backend
owns the 15-second deadline; it rolls back on timeout, menu closure, menu owner
loss or normal daemon shutdown. B reverts an active preview before leaving
Display. Keep verifies the active mode again and atomically saves it. Failed
saving leaves the preview available for Revert or timeout. Stale generations
and preview tokens cannot confirm another preview.

The watchdog runs in sessiond, so a menu crash does not remove it. Killing
sessiond with SIGKILL or losing power during an unconfirmed preview is outside
this in-process guarantee; the preview is never written as a confirmed choice.
If Sway becomes unreachable, rollback reports an error and retries while the
daemon lives. A disconnected or replaced display cancels the preview without
sending its old mode to another monitor. Displays without useful EDID serials
can only be distinguished by connector, make and model.

Confirmed choices live in `$XDG_CONFIG_HOME/zaman/display.json`, falling back
to `~/.config/zaman/display.json`. On sessiond startup only matching display
identities with still-advertised saved modes are restored. This is preference
restoration, with no resolution ranking, automatic refresh choice, hotplug
layout policy, output disabling or changes to scale/position. Sway config
reloads can override runtime modes; the next daemon start restores confirmed
preferences. Cage remains usable; this page reports that Sway is required.

`SWAYSOCK` must already be imported into the systemd user manager by the Sway
session launcher, as in the current A/B setup. No compositor launcher or boot
default changes are included. The saved preference is not a Sway config file.

## Build and install (run by the device owner)

The new `serde_json` dependency and its registry lockfile entries are included.
Build and test this candidate on the device before installation:

```bash
cargo build --locked --release --bin zaman-sessiond --bin zaman-menu --bin zamanctl
cargo test --locked
./install.sh
zamanctl version
```

Use the complete installer rather than `install-zaman-menu.sh`: the new menu
needs the matching backend. `install.sh` accepts installed 0.6.7, 0.6.8 or
0.6.9, backs up binaries and installs 0.6.9. Keep its printed backup directory
until acceptance succeeds. It does not remove saved display preferences during
binary rollback. Remove or move `display.json` to stop restoring a choice.

## Acceptance on Zaman

1. Confirm Display shows HDMI-A-1 and 2560 x 1440 at 59.951 Hz on the current
   Acer. Compare its selectable rates against `swaymsg -t get_outputs`.
2. Select 1920 x 1080 at an advertised rate. Apply, wait without pressing
   anything, and verify the original mode returns after 15 seconds.
3. Apply again, choose Revert (or B), and verify the original mode returns.
4. Apply again and stop only `zaman-menu.service` during the preview. Verify
   sessiond restores the original mode, then start the menu service again.
5. Apply and Keep. Confirm the preference file exists, the mode is active, and
   restarting sessiond restores that confirmed choice. Restore your desired
   mode afterward using the same controls.
6. Check fonts and layout at both resolutions, controller navigation, returning
   to Pegasus, game pause/resume and the existing Volume/Network screens.
7. If feasible, check monitor unplug/replug, a stale confirmation after timeout
   and an unwritable preference destination. No unconfirmed mode should be
   saved, and a replacement monitor must not receive the previous one's mode.

The production renderer can generate fixtures without Sway for visual checks:

```bash
target/release/zaman-menu --preview display 2560 1440 /tmp/zaman-display.bmp idle
target/release/zaman-menu --preview display-preview 1920 1080 /tmp/zaman-display-preview.bmp idle
```

## Reusable backend surface

Session1 adds `DisplayStatus`, `PreviewDisplay(generation, connector, width,
height, refresh_millihertz)`, `ConfirmDisplay(generation, token)` and
`RevertDisplay(generation, token)`. Each returns a JSON `DisplayState` snapshot:
available, outputs (identity, current mode, advertised modes), optional preview
(token, output, mode, seconds_left), and error. Existing menu tuples and actions
are unchanged. The menu polls while Display is open. The later side menu can
consume the same surface; it does not need to own IPC, persistence or rollback.

Sway IPC reference: <https://manpages.debian.org/trixie/sway/sway-ipc.7.en.html>.
Output mode reference: <https://manpages.debian.org/trixie/sway/sway-output.5.en.html>.
