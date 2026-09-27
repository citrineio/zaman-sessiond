# 0.6.7 volume control

Source base: the supplied `zaman-transfer-games-0.6.6.tar.gz` package, including the transfer menu and supervision. This source tree includes no compiled binaries.

The menu shows a single Volume row in both contexts. Up/Down selects a row. Left/Right on Volume changes the default PipeWire sink by 5 percentage points within 0–100%; A on Volume toggles mute and restores the prior volume when pressed again. While muted, the slider displays 0% and the right pane explains how to restore audio. Left/Right are ignored while muted, preserving the prior level. The selected action's description appears in the right pane. `wpctl get-volume @DEFAULT_AUDIO_SINK@` refreshes the actual sink state on opening and after each action. The menu runs `wpctl` away from the input loop and accepts only one audio operation at a time. No polling is added. Transfer Games remains a selectable library action, and transfer suspension blocks menu input.

Prerequisite on the device: `wpctl` in the user service PATH, with access to the same PipeWire and WirePlumber user session as the game. Check `command -v wpctl` and `wpctl get-volume @DEFAULT_AUDIO_SINK@` as the kiosk user.

Build on the board or a matching ARM64 development environment with Rust >=1.85 and the repository's SDL development dependencies:

```bash
cargo test --locked --bin zaman-menu
cargo build --locked --release --bin zaman-menu --bin zaman-sessiond --bin zamanctl
```

Run `./install.sh` from this source directory as the logged-in kiosk user after building. It checks that games and transfers are stopped and the installed version is 0.6.6 or 0.6.7, saves all three installed binaries under `~/.local/state/zaman-sessiond/backups/`, installs the release binaries, restarts the two user services, and verifies version 0.6.7. On failure after stopping services, it attempts to restore the backups and restart the original services. It prints the backup location; retain it until hardware acceptance is complete.

The menu revision after the first 0.6.7 build changes only `src/bin/zaman-menu.rs`. If 0.6.7 is already installed, copy that file into the existing 0.6.7 source checkout and rebuild only `zaman-menu`. Back up `/usr/libexec/zaman-menu`, install the new `target/release/zaman-menu`, then restart only `zaman-menu.service`. No sessiond or CLI binary change is required for this revision.

On device, open the menu from the library and a game. Verify initial volume/mute state, boundary values, a single A toggle per button release, actual audio changes, return to game, Transfer Games opening and return, and all lifecycle actions. If `wpctl` fails, the menu reports an action error and journal records the underlying failure; other menu actions remain available.

For a noninteractive rendering check, use `target/release/zaman-menu --preview volume 1920 1080 /tmp/zaman-volume.bmp paused` and repeat with `muted` in place of `volume`. The preview uses a sample 65% level and does not change real audio.

Static diff and layout checks were run in the preparation workspace, which lacked a Rust toolchain. The user compiled the code on the Q6A, then reported that the resulting menu was functional and visually correct.

## Deferred: volume audition cue

When the selected Volume slider changes, play a brief, recognizable sound through the current output so the user can judge loudness immediately. Produce one cue for a completed adjustment rather than stacking sounds from rapid D-pad presses. The cue must follow the effective output level, remain silent while muted, and must not change the stored volume, mute state, or game/session state. Avoid a continuous tone or a loop; the user should be able to make several adjustments without a buildup of sound. The specific sound asset, rate limiting, and routing through PipeWire require a later design and device test. This feature is **not implemented in 0.6.7**.
