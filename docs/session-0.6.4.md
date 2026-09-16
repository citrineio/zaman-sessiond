# Session 0.6.4 — menu restoration and graceful exit

Date: 2026-09-16. Base reported by the hardware handoff: `c31180c`,
working tree `~/zaman-lab/zaman-sessiond-0.6.3`. This document records the 0.6.4 release work.

## Scope and existing hardware results

The handoff reports the amber, two-column menu restored from the separate
0.7.0-rc.1 source, a 1920x1080 design space with native-resolution font
rasterization, cached fonts, and Shut Down in both menu contexts. The context
API and generation-guarded RequestMenuAction routing are present. Phase A
activate-on-release, MenuPresented, and MENU_SERVICE registration are retained.

The handoff also reports that transient game units now use KillSignal=SIGINT
and that daemon-driven game exit is fast and preserves saves. Menu appearance
and three preview resolutions were checked on hardware in the prior session.
Shutdown was subsequently confirmed in both contexts. Earlier checks above are handoff results; subsequent acceptance is recorded below.

Preemption is out of scope. No transitions.rs port, freeze/thaw, operation
budget changes, or additional game-state setters are included.

## Navigation latency: measured cause

The instrumented hardware capture shows periodic input refresh taking
623.55–638.73 ms even with `replace=false`. The worker awaited the complete
refresh within its select arm, leaving input events queued until it returned.

All times below use the embedded SystemTime values, not journal receipt times:

| Event | Listener timestamp | Worker timestamp | Queue delay |
| --- | --- | --- | --- |
| Down release | 1789526225.349907573 | 1789526225.945113276 | 595.206 ms |
| Up press | 1789526227.821690966 | 1789526227.949399322 | 127.708 ms |

The Up press rendered by 1789526227.954073674, 4.674 ms after worker receipt.
The capture localizes the delay to the worker waiting for refresh; it does not
isolate which discovery D-Bus request accounts for the refresh duration.

## Fix

- Run discovery in a separate Tokio task sharing the existing bus connection.
- Keep at most one discovery, including completed results awaiting consumption.
  Periodic ticks and launch requests coalesce into that task.
- Keep the existing one-second refresh interval. Add no polling loop or thread.
- Apply completed inventories in the worker using the current menu mode and
  current listener health, rather than state captured before discovery.
- Preserve inventory ordering, change detection, listener recovery, and error
  deduplication. Unchanged inventories require no interception write or monitor
  replacement.
- Keep initial discovery before entering the worker loop. Launch requests now
  request background discovery instead of awaiting another complete scan.
- Retain serialized interception writes when inventory actually changes or
  listeners need recovery. Those writes can still await D-Bus; this change
  removes the measured periodic discovery stall, not all lifecycle waits.
- Remove temporary input timestamps, refresh traces, and render timing output.

The discovery task only reads inventory. It cannot change controller mode.
Its JoinSet drops with the worker, cancelling an outstanding read on exit.
Menu layout, action routing, and graceful exit behavior are unchanged.

## Validation and acceptance

- The operator ran `cargo test --locked` on Q6A: all 53 tests passed
  (43 daemon tests and 10 menu tests; no failures).
- `git diff --check` produced no errors in the supplied output.
- `cargo fmt --check` initially reported formatting differences in api.rs and
  zaman-menu.rs. The closeout script applies formatting and requires a clean
  formatting check before committing.
- The operator confirmed navigation lag resolved after installing the fix.
- The operator confirmed Shut Down from both the library and an active game.
- Graceful game exit and preserved saves were verified in the earlier handoff;
  a separate post-navigation-fix save verification was not explicitly reported.
- Controller reconnect and repeated menu-transition stress checks were not
  explicitly reported. They are not recorded as passed.
- The compiler reported unused `InFlight.started` and `OperationSlot::elapsed`.
  The out-of-scope operation machinery remains untouched.

The assistant performed source review and patch-application checks only. No
Rust environment, compilation, installation, or Rust test execution was
performed in the assistant workspace.

## Follow-up

Replace periodic inventory discovery with verified device/topology and service
notifications. Subscribe before the initial scan, coalesce change notifications,
and use backoff only while recovering unavailable services/subscriptions.
The current 0.6.4 fix still performs periodic discovery in the background.
Preemption remains deferred.
