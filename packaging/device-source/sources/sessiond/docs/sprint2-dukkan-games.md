# Sprint 2: installed games launched from Dukkan

## Source candidate (14 October 2026)

This tree was reconstructed from the accepted Sprint 1 local checkpoint bundle.
It could not be compiled in this workspace; Kadhem compiled and activated the
candidate on the Q6A. Keep the accepted InputPlumber, Steam windowed Big Picture,
emulator and menu setup.

`src/installed.rs` owns a separate curated installed-game status and transient
`zaman-installed-game.service`. Dukkan submits one of four fixed app IDs through
`InstalledGameLaunch`. The helper at
`/usr/local/libexec/zaman/flatpak-game-session.py` (or the daemon's
`ZAMAN_INSTALLED_GAME_HELPER` override) starts a user aarch64/stable Flatpak
with `--die-with-parent`, commits only a positive PID, start time and exact app
scope, and records owned windows. Rust registers that scope with Sway. The
launch CLI waits for a verified window before reporting ready. Native status
is separate from the legacy emulator `Status` tuple; generation is a decimal
string to preserve all u64 values in the menu bridge.

The helper and Dukkan transaction relay share a runtime lock, so an uninstall
cannot begin while a managed game holds the lock. Dukkan also checks native
status before uninstall. `InstalledGameClose` targets one generation; the menu
bridge pins the same unique D-Bus owner and polls that generation. Startup and
shutdown recovery stop the exact transient unit and recorded Flatpak instance.
An uncertain cleanup remains `CleanupFailed` and blocks replacement until an
explicit close retry proves cleanup.

Dukkan's progress relay uses an ordinary Flatpak CLI in a PTY with
`FLATPAK_TTY_PROGRESS=1`; it reports only observed current-step percentages,
and stays indeterminate when none are reported. Transaction success requires
the CLI to exit 0 and an exact installed-ref postcondition. Uninstall retains
saved application data.

The Q6A build, activation and owner acceptance passed. Preserve the
device-specific shortcut adapter and avoid copying whole repository trees over it.

## Acceptance

### Status reported by Kadhem on 14 October 2026

- The Sprint 2 source candidate compiled and its build/tests passed on the Q6A.
- Activation passed with the installed-game daemon idle and without launching a
  game or starting a Flatpak transaction.
- SuperTuxKart Play entered `Running`; after a natural exit, status returned to
  `Exited`, foreground `library`, reason `installed-game-ended`. A repeat launch
  also returned to Library.
- Kadhem reported the Dukkan layout fix tested and passing, then confirmed the
  search and resident keyboard-view update works.
- Kadhem subsequently reported the Install/Uninstall/progress check, Guide
  Resume/Exit check, and Steam/emulator/Dukkan regression pass as passing.
- No launch failure or cleanup problem was observed in normal use. This is not
  a device fault-injection result for the failure/cleanup guards.
- During a controlled daemon restart with SuperTuxKart running (generation 4),
  installed-game status returned to generation 0 / Idle; the installed-game
  unit was inactive, foreground was Library, and Kadhem confirmed controller
  input worked there. The supplied `flatpak ps` output listed no instance.
- Sprint 2 device acceptance is complete for the tested curated-game path.
  Individual failed-launch and client-close races were covered by source guards
  and synthetic tests but were not separately fault-injected on the Q6A.

### Closeout evidence

1. Completed by owner report: confirmed Install/Uninstall and progress, Guide
   Resume/Exit, and Steam/emulator/Dukkan regression routes.
2. Covered in source tests: invalid catalog ID, wrong architecture, exact-instance
   cleanup, stale close request, active transaction lock, and replacement blocked
   after uncertain cleanup. The owner reported the candidate tests passed.
3. Completed on Q6A: controlled daemon restart during a running Dukkan game,
   followed by Idle, inactive game unit, Library foreground and responsive
   controller. No orphaned Flatpak instance appeared in the supplied output.

Reconcile the three Sprint 2 source trees with the exact activated artifacts and
publish the accepted changes. Keep
device-specific service overrides and shortcut adapters out of generic patches.

1. Install an available curated ARM64 game with confirmation; refresh -> installed.
2. Select Play -> one foreground game; no tiling with Pegasus or Dukkan.
3. Guide -> Resume preserves input; reconnect works while menu/game is active.
4. Natural quit -> Library; reopen Dukkan -> browsing state retained.
5. Exit Game confirmation/cancel and graceful/forced cleanup affect only that game.
6. Missing app/architecture, failed launch, double Play and client close are safe.
7. Restart daemon -> no orphan game or stuck controller interception.
8. Retest accepted Steam/emulator/Dukkan recovery baseline.

Catalog expansion, background transaction service, Pegasus catalog export and
notification popup are separate work after this launch/lifecycle slice.
