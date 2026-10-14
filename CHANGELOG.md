# Changelog

## Sprint 1 storefront source candidate (2026-10-12)

Managed Dukkan/Steam clients, isolated workspaces, contextual resident-menu
contract and opt-in Steam Guide delegation. Based on today’s published snapshot.
Python/JavaScript checks passed; Rust/Qt build and device acceptance are pending.
See `docs/sprint1-storefronts.md`.


The repository's first push after 0.6.5 includes both 0.6.6 and 0.6.7.

## Unreleased configuration — planned 0.7.0 validation

- Enable InputPlumber's native ManageAllDevices setting after every service
  start/restart, using its existing upstream profiles without a generic YAML
  replacement. Retry Manager readiness and verify property readback.
- Include the helper/drop-in in source packaging and full-installer rollback.
  Deployment of this integration requires no Rust rebuild. The owner confirmed
  Bluetooth menu input after enabling the native setting on InputPlumber 0.78.0;
  persistent startup, reconnect, gameplay and other-controller validation remain
  pending. Supersede the earlier generic-profile override proposal.

## [0.6.12] - 2026-10-06 (source candidate; device validation pending)

- Isolate registry test ROMs in exclusively created directories instead of timestamp-only filenames. Clean up only the owned fixture and assert the intended rejection reason, preventing parallel tests from sharing/deleting each other's ROMs.
- Remove the redundant Resume cell; B and Guide retain menu dismissal. Keep System before Exit Game and retain Exit Game before the power actions.
- Add native-scale gear, restart arrow and power icons to the System, Reboot and Shut Down cells without new fonts or dependencies.
- Open separate Reboot/Shut Down confirmation pages with a large action icon, red warning frame, explicit consequences and Yes/No cells. No is selected on every opening.
- Selecting No or pressing B/Escape restores the originating power cell. Guide retains its global menu-dismissal behavior. Only Yes requests the existing graceful power action.
- Respect transfer suspension and revoke an open confirmation when its action becomes unavailable. Keep generation checks and request deduplication in the existing action path.
- Add focused confirmation test sources and production-renderer preview fixtures. Compilation, previews, installation and device tests remain with the owner.

## [0.6.11] - 2026-10-06 (source candidate; device validation pending)

- Replace the separate Mute/Unmute cell with a speaker icon button next to the volume slider and percentage, leaving four visible audio rows.
- Give the icon a distinct focus highlight; Right from the slider selects it, A changes mute state and Left returns to the slider without changing volume. Up/Down follows the four visible rows.
- Separate audio focus from adjustment: A on the slider starts adjustment, Left/Right changes the level, A confirms back to System, and B returns to row navigation while retaining applied changes.
- Draw speaker/wave/cross symbols geometrically at the output scale, avoiding font symbol dependencies. Muted state uses a red cross; slider confirmation still returns to System.
- Add System > Bluetooth with adapter power, bounded discovery, paginated device/status lists, pairing, Connect/Disconnect and confirmed Forget.
- Use an application-scoped BlueZ agent for code confirmation, PIN/passkey entry and keyboard pairing instructions; cancel pending requests on exit and reject stale or unrelated callbacks.
- Refresh Bluetooth connection/pairing state and optional battery values; show green Connected/red Disconnected labels and preserve selection by device identity.
- Update controller guidance, focused test sources and renderer fixtures. Compilation, installation and device tests remain with the owner; see `docs/menu-bluetooth-0.6.11.md`.

## [0.6.10] - 2026-10-06 (source candidate; device validation pending)

- Open a dedicated Volume screen from System: Left/Right applies 5% steps immediately; A on the slider confirms and returns to System.
- Select playback outputs with Left/Right and explicit Apply output; discover nodes through PipeWire JSON, verify identity before applying and read back the active default.
- Include `tools/zaman-stream` for one-command, on-demand WayVNC debugging on the Tailscale address, with stop/status/logs actions and no occupied SSH terminal.
- Separate Mute/Unmute from confirmation and retain the stored volume while muted, including when adjusting it.
- Show green Connected and red Disconnected labels for Ethernet, Wi-Fi and available-network rows; retain readable colors on the selected row.
- Refresh cached Wi-Fi connection indicators using the actual SSID rather than the connection profile name or stale scan flags.
- Update focused test sources and renderer fixtures; compilation, testing and installation remain with the device owner. See `docs/menu-feedback-0.6.10.md`.
- Display acceptance continues separately: timeout rollback is user-confirmed; saved-mode restoration after daemon restart and reboot remains pending.

## [0.6.9] - 2026-10-06 (source candidate; device validation pending)

- Add System > Display with Sway output identity, resolution and fractional refresh selectors.
- Verify mode changes through IPC readback and offer a 15-second Keep/Revert preview, with rollback owned by sessiond.
- Save only confirmed choices and restore matching supported preferences on daemon startup; no automatic mode ranking or hotplug policy.
- Expose reusable display methods on Session1 for the later side menu and reload fonts when the output size changes.
- Include focused test sources and display preview fixtures; compilation, testing and installation are performed by the device owner. See `docs/display-settings.md`.

## [0.6.8] - 2026-09-27 (candidate)

- Move Volume into a System submenu in library and game contexts; preserve its 0.6.7 control behavior.
- Show Ethernet, Wi-Fi and IPv4 status; scan and select visible networks, toggle or disconnect Wi-Fi, and join WPA Personal networks with controller password entry.
- Cycle password keyboard modes with X or the onscreen Mode key.
- Show masked entry feedback and Connect readiness while typing the Wi-Fi password.
- Show the password field above the onscreen keys so typing feedback is visible beside the keyboard.
- Add temporary Show/Hide for password verification and reconnect saved Wi-Fi profiles without re-entry.
- Keep network scans responsive and refresh connection status automatically every three seconds while the network menu is open.
- Restore device autoconnect when turning Wi-Fi on after an explicit disconnect.
- Keep network work off the menu input loop and retain the existing D-Bus session contract.
- Require NetworkManager's `nmcli` for network controls; see `docs/deploy-0.6.8.md` for device acceptance.

## [0.6.7] - 2026-09-26

- Show five spacious menu rows and display the selected action's description in the right pane.
- Add a 0–100% volume slider in the library and game menus; A mutes and restores the previous volume without a separate Mute row.
- Read and control the PipeWire default output through `wpctl` in the logged-in user session.
- Handle audio commands off the menu event loop, refresh audio state on each opening, and keep session actions responsive.
- Preserve Transfer Games supervision and input suspension from 0.6.6.
- Requires `wpctl` from WirePlumber in the menu service PATH.
- Document the six-row text collision and the initial compile error in `mistakes.md`.
- Defer a brief audible cue while adjusting volume; specification in `docs/session-0.6.7.md`.
- The user reported the revised menu functional and visually correct on the Q6A.

## [0.6.6] — Transfer Games menu integration (previously unpushed)

- Add Transfer Games to the four-row library menu; game menu remains unchanged.
- Supervise the standalone transfer window, preserve the underlying menu, and hand normalized input exclusively to the ready GUI.
- Validate generation/owner, reject conflicting launch/power/menu actions, and restore the originating menu after confirmed cleanup.
- Add `zamanctl transfer`, transfer D-Bus members, and the on-demand GUI user service.
- Paired transfer component 0.4.0. No new Rust dependencies.
- User-reported laptop browser and phone QR transfers passed; the transfer test suite passed. The GUI cancellation exit defect was corrected during board validation.
- This version was not separately pushed or tagged; the 0.6.7 push carries these changes.

## [0.6.5] - 2026-09-16

- Add Reboot to the library and active-game menus, the D-Bus API, and
  `zamanctl reboot`.
- Reuse the existing graceful SIGINT game-stop path before successful session
  completion requests reboot; failed session completion does not reboot.
- Record logind power-action failures in status and the journal.
- Fit the fourth game-menu row and move pending/error text to the right panel.
- Declare the existing MenuContextChanged signal for runtime introspection.
- Correct smoke-test version matching to read Cargo.toml and capture complete
  CLI replies to avoid broken-pipe panics and detect CLI failures.
- Installed and hardware-accepted on the Q6A: library reboot, in-game reboot
  with save preservation, frontend return, menu layout/navigation, and shutdown
  in both contexts. User confirmed all requested checks work with no quirks.

## [0.6.4] - 2026-09-16

- Restore the amber two-column system menu and right-hand context panel.
- Fix DPI blur using a 1920x1080 design space and fonts rendered at physical
  output resolution; cache fonts per menu surface.
- Add Shut Down to library and game menus.
- Add the menu context D-Bus surface and generation-guarded action routing,
  preserving activate-on-release and the MenuPresented handshake.
- Restore graceful daemon-driven game exit with SIGINT for the transient unit,
  allowing the emulator to preserve saves.
- Fix navigation stalls by moving periodic controller discovery into a bounded
  background task; apply results using the current controller interception mode.
- Remove temporary navigation and render tracing.

Versions newest-first. Each entry corresponds to one development session
with one feature goal and a findings document under `docs/`.

## 0.6.3 — 2026-09-14

**Goal:** single in-flight lifecycle transaction.

### Added

- `src/operations.rs`: `OperationSlot` enforcing one lifecycle operation at
  a time. `OperationIdentity` captured at authorization is validated on
  completion. `StartRejected::Busy` / `ShuttingDown` for rejected starts.
  `abort_for_quit()` for the priority path. `check_deadline()` for expired
  operations.

### Changed

- `src/daemon.rs`: every lifecycle arm in `run_worker` uses `begin_or_reject`
  and completes with the captured identity. New 50 ms `slot_tick` select arm
  polls deadlines.
- `src/contract.rs`: version constant bumped to `0.6.3`.
- `Cargo.toml`: version bumped to `0.6.3`.

### Fixed during install verification

- Completion path re-read the current menu generation instead of using the
  identity captured at begin, causing every successful menu-open to be
  rejected as `IdentityChanged` until the deadline tick fired. Now captures
  once and reuses.

### Not delivered

- Preemption of an in-flight operation. The slot is enforced, but a running
  operation still blocks the worker from polling Quit or the deadline tick
  until it returns. Restructuring `run_worker` to spawn operations is
  deferred to 0.6.4.

### Findings

See `docs/session-0.6.3.md`. Summary: Mesen's `PauseWhenInBackground` behavior
replaces cgroup freeze for menu-over-game; SIGINT graceful exit was confirmed
while internally paused; the foundation spec should be revised accordingly in
a later session.

## 0.6.2 — baseline

Installed on Dragon Q6A. See the delivered 0.6.2 archive.
