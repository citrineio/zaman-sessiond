# Changelog

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
