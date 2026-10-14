# Sprint 2: installed games launched from Dukkan

## Current source boundary

Dukkan has four curated identifiers, user Flatpak install/remove, ARM64 remote
availability checks, confirmation, and installed-state refresh. It has no game
launch method. Installed-game selection currently opens a removal confirmation.
Sessiond's existing Launch is ROM/emulator-oriented; StorefrontAction supervises
clients. A Flatpak game must not become a child that Close Dukkan can kill.

## First implementation slice

- Add a daemon-owned catalog/launch registry. Dukkan submits only the curated app
  ID; remote artwork/metadata cannot provide argv, flags, executable paths or env.
- Verify the exact user installation and aarch64 ref locally before launch.
  Resolve an explicit installed branch/ref, avoiding ambiguous app IDs.
- Add an additive installed-game API and CLI with request acknowledgment and
  session identity. Do not falsify ROM paths or alter the emulator Launch API.
- Start one managed game independently of zaman-store-dukkan.service; Dukkan
  remains alive in store-dukkan. Enforce a single active game across emulator,
  installed game and detected Steam game; serialize launch/stop and reject races.
- Place the game in a game workspace using verified ownership. Flatpak can create
  an app scope through its session helper; validate actual cgroups before relying
  on the launcher PID or inherited parent cgroup. Record exact Flatpak instance
  identity and use it for lifecycle cleanup if needed. Never stop all Flatpaks.
- Menu Resume and Exit Game target this game. Restore input before actions.
  Use a normal window close first; qualify a bounded forced-stop fallback.
  Do not inherit emulator save-state assumptions for native applications.
- Natural exit or confirmed Exit Game returns to responsive Library. Reopening
  Dukkan preserves its browsing state; closing Dukkan must not stop the game.
- Dukkan details show Play as the primary installed action, Remove separately
  with the existing confirmation. Busy state and launch errors prevent duplicate
  dispatch and provide a recoverable UI.

## Device qualification gate

Before finalizing lifecycle code, inspect one manually launched existing curated
user ARM64 Flatpak: exact installed ref, flatpak ps instance/PID, /proc cgroup,
Sway window pid/app_id/class, natural exit and normal window-close behavior.
This is a deliberate device qualification step, not an assumption that the
launcher owns the whole runtime. No dependency installation is required.

## Acceptance

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
