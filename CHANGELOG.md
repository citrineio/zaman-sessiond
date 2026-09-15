# Changelog

Versions are listed newest-first. Each entry corresponds to one development
session with a single feature goal and its own findings document under `docs/`.

## 0.6.3 — 2026-09-14

**Goal:** single in-flight lifecycle transaction.

### Added

- `src/operations.rs`: `OperationSlot` enforcing one lifecycle operation at a
  time, with an `OperationIdentity` captured at authorization and validated on
  completion. Rejected starts return `StartRejected::Busy` or
  `StartRejected::ShuttingDown`. `abort_for_quit()` provides a priority path
  for Quit. `check_deadline()` reports operations that exceeded their budget.

### Changed

- `src/daemon.rs`: every lifecycle arm in `run_worker` begins a slot operation,
  executes, and completes with the current identity. A new `slot_tick` select
  arm polls deadlines every 50 ms.
- `Cargo.toml`: version bumped to `0.6.3`.

### Not delivered

- Preemption of an in-flight operation. The slot is enforced, but a running
  operation still blocks the worker from polling Quit or the deadline tick
  until it returns. Restructuring `run_worker` to spawn each operation is
  deferred to 0.6.4.

### Findings

See `docs/session-0.6.3.md`. Summary: Mesen's internal `PauseWhenInBackground`
behavior replaces cgroup freeze for the menu-over-game case; SIGINT graceful
exit was confirmed while internally paused; the foundation spec should be
revised accordingly in a later session.

## 0.6.2 — (baseline, installed on Dragon Q6A)

See the delivered 0.6.2 archive for details. This changelog starts at 0.6.3.
