# Zaman Sessiond 0.6.7 — Transfer Games and volume control

## Summary

This Git push advances the published repository from 0.6.5 to 0.6.7. It includes **both** the previously unpushed 0.6.6 Transfer Games integration and the 0.6.7 volume menu. The changes can be committed together while preserving the existing Git history; no separate 0.6.6 release or tag is claimed.

## 0.6.6 changes included in this push

- Add **Transfer Games** to the library menu and `zamanctl transfer` to launch the standalone transfer GUI. The game menu and existing CLI transfer path remain available.
- Supervise the transfer window through sessiond with generation-bound launch, readiness, ownership, timeout, cleanup, and return to the originating menu.
- Transfer controller/keyboard input to the GUI while it owns the foreground. Block hidden-menu, Pegasus, game, and power-action input leakage and reject conflicting session requests during a transfer.
- Provide on-device invitation and status screens with QR code or PC address/PIN access to the HTTPS browser transfer flow; handle cancellation and incomplete-file cleanup. Pair sessiond 0.6.6 with Zaman Transfer 0.4.0.
- Keep Transfer Games as a foreground workflow. Background transfers, device-side byte progress, and automatic Pegasus library refresh remain deferred.

## 0.6.7 changes

- A Volume row appears in both the game and library menus. Left/Right changes the default PipeWire output in 5% steps across 0–100%.
- Pressing A on Volume toggles mute. The muted slider displays 0%; unmuting restores the previously set output level. Left/Right does not change that saved level while muted.
- The selected action's description appears in the right pane. The menu has five rows in each context, with no separate Mute row.
- Volume reads and writes use `wpctl` outside the menu input loop. The menu refreshes the audio state on opening and after each audio action without adding polling.
- The source includes `mistakes.md`, recording the earlier text overlap and compile error, and a deferred specification for a short audible volume test cue in `docs/session-0.6.7.md`.

## Compatibility and validation

- Based on the packaged 0.6.6 source, itself based on the accepted 0.6.5 commit `7ac0f6bb8a39f0028ed3185d04f5cefa3528c2bf`. The remote repository had not yet received 0.6.6.
- The user reported successful transfers from a laptop browser and phone QR flow and reported that the Zaman Transfer suite passed. A GUI cancellation exit issue was corrected during 0.6.6 validation. The full acceptance list in `docs/deploy-0.6.6.md` was not independently witnessed by the assistant.
- Requires `wpctl` and access to the kiosk user's PipeWire/WirePlumber session.
- The user built and tested the revised menu on the Q6A and reported it functional and visually correct. This report covers the menu; it is not a claim that every device and emulator path was retested.
- The audible volume test cue is documented for a future release and is not implemented here.

## Installation

For an existing 0.6.7 installation, rebuild and replace only `src/bin/zaman-menu.rs` / `/usr/libexec/zaman-menu`, using `install-zaman-menu.sh` or the commands in `docs/session-0.6.7.md`. For a 0.6.6 installation, build all three Rust binaries and use `install.sh` after stopping games and transfers. Retain the backup until validation is complete. The separate Zaman Transfer 0.4.0 Python/QML component and its configuration are supplied in the original `zaman-transfer-games-0.6.6.tar.gz` bundle; this sessiond repository does not package those assets.
