# Zaman Sessiond 0.6.5 — Accepted engineering handoff

Date: 2026-09-16. Feature goal: reboot support.
Status: installed and hardware-accepted. Commit/push remains to be executed
or confirmed using the closeout script; no final commit hash is invented here.

## Baseline and installed state

- Baseline: `0d0d156691dd4c000069c3e64842a1fa38161a7a` (0.6.4).
- Board checkout: `~/zaman-lab/zaman-sessiond-0.6.3`; directory name is unchanged.
- Installed daemon-reported version: **0.6.5**.
- Installation output: `INSTALL_PASS`; Reboot method and MenuContextChanged
  signal verified. Status Idle, foreground library, no result/error.
- Backup: `/home/kadhem/zaman-pre-0.6.5.N9pDxX`.
- Installed binaries: `/usr/libexec/zaman-sessiond`, `/usr/libexec/zaman-menu`,
  `/usr/local/bin/zamanctl`.

## Hardware acceptance

After installation, the user was asked to verify library reboot and frontend
return; active-game reboot with a recognizable save surviving relaunch; menu
spacing/navigation; and shutdown in library and game contexts. The user replied:

> Confirmed all work no quirks.

Those requested checks are accepted. No additional acceptance gate is being
introduced. A separate CLI-triggered hardware reboot was not explicitly
reported; the CLI is implemented and the Reboot API is introspection-verified.
Standalone renderer previews at every proposed resolution were not supplied;
the user accepted the actual installed menu layout and navigation.

## Implementation

- Reboot is present in both open menu contexts, the D-Bus interface, and CLI.
  Menu requests retain the generation/allowed-action guard and controller
  activate-on-release behavior.
- Library order: Resume, Reboot, Shut Down.
- Game order: Resume, Exit Game, Reboot, Shut Down.
- Worker classifies reboot with OperationKind::Reboot and reuses SHUTDOWN_BUDGET.
  Idle reboot calls login1; game reboot sends SessionControl::Reboot.
- SessionControl maps to RebootRequested, but that outcome reaches the worker
  successfully only after the existing systemd stop path succeeds. SIGINT,
  timeout/fallback policy, and existing cleanup ordering are unchanged.
- Successful RebootRequested selects Reboot; successful ShutdownRequested
  selects PowerOff. Session errors, task failures, normal exit, and ordinary
  stop do not select a power action. Logind failures enter status and journal.
- Three-row geometry remains 369/156/27 (top/height/gap); four-row geometry
  is 324/126/18 in the 1920x1080 design space. Last game row ends at y=882,
  before the footer at 936. Pending/error text uses the right column at y=666.
- Preserve the amber theme, cached/native-resolution fonts, MenuPresented,
  menu service ownership, focus-based pausing, and background discovery fix.
- Version updated in Cargo.toml, the root Cargo.lock package, contract::VERSION.
  No new dependency, polling loop, thread, or lifecycle-preemption mechanism.

## Acceptance-session corrections

1. Smoke script inherited a hardcoded 0.6.2 expectation/banner. It now reads
   Cargo.toml and reports expected/actual versions.
2. MenuContextChanged was emitted by name but lacked a Rust signal declaration.
   Added that declaration with generation:u64. The smoke check remains intact.
3. Live CLI output piped through grep -q could panic when stdout closed early.
   Five checks now capture the entire CLI reply, reject nonzero CLI exits,
   and then match. Introspection matching also drains its input.
4. Interactive shell parsing failed when heredoc control lines were pasted
   separately. Subsequent diagnostics used a quoted child shell and log file;
   installation/closeout are standalone script files.

All corrections belong to the same still-open 0.6.5 session. No extra feature
or version was introduced.

## Verification evidence and limits

- User supplied successful release build output.
- User supplied PASS and SMOKE_PASS: runtime D-Bus contract, natural synthetic
  game exit, explicit stop, session cleanup. That run contained the CLI pipe
  panics described above; do not describe it as a panic-free run.
- Smoke observed zero controller composites and degraded normalized input.
  The later hardware acceptance confirms menu navigation on the installed
  system; the earlier synthetic run itself did not validate controller input.
- Harness correction verified locally with a large-output shell fixture,
  absent-match case, and nonzero CLI exit. A separate corrected-harness board
  rerun was not reported and is not required to repeat accepted hardware work.
- Installer applied the harness correction, installed the existing release
  binaries, and verified version/status and required D-Bus members.
- Assistant checked patch application/reversal and byte equality against the
  uploaded source, shell syntax, version consistency, and XML parsing.
- Rust tests were written before implementation, but the assistant did not run
  red/green Rust tests, install a toolchain, compile, or access the board.
  User-side formatting/build/test work was retained. Exact 0.6.5 test counts
  were not pasted; do not substitute 0.6.4's 53-test count.
- Existing unused InFlight.started / OperationSlot::elapsed warnings remain
  outside this feature. No new runtime failure is established.

## Preserved lifecycle boundaries

D-Bus/CLI success acknowledges queueing. Later errors appear in status/journal;
the menu closes on dispatch rather than gaining a new persistent error view.
If natural game exit wins the existing select race, no reboot outcome is
produced; retry from the library. Failed control delivery never falls back to
an immediate reboot. Its logged error can later be replaced in the status
snapshot by session completion. These existing lifecycle semantics remain.

Successful systemd stop is not an independent emulator save acknowledgement;
the installed emulator's save behavior is now hardware-accepted. This does not
qualify every future emulator. The login1 call uses the existing interactive
boolean authorization behavior, not forced reboot. No polkit rule was added.

## Closeout

Run `zaman-0.6.5-closeout.sh` on the board. It updates this session record,
conventions, deployment reference, and changelog; checks formatting/whitespace;
stages explicit release paths; commits; pushes main; and compares local/remote
hashes. No builds, hardware power actions, source-code patches, force push,
or downloaded patch files are included in closeout.

The commit is the release identity. Record the final printed local/remote hash
in the conversation after execution. Installed and accepted is established;
committed/pushed must not be claimed until the command result confirms it.

## Next feature and working constraints

Next: **Zaman-transfer UI integration**, before additional emulated systems.
Placement remains open: menu, frontend, or a separate application invoked by
either. Do not treat a placement or UI framework as already approved.
Then additional supported emulator systems, native ARM64 games, and diagnosis
of the observed 22-second power-button-to-frontend boot.

One feature and one version per session. Retain the user's source-first split:
no assistant-side toolchain setup, dependency installation, compilation, or
resource-intensive polling. Do not port old phase-b transition machinery or
reopen lifecycle preemption as part of the next feature without new scope.
