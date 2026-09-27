# zaman-sessiond

Current release: **0.6.7**, including the previously unpushed 0.6.6 Transfer Games work. See [release notes](docs/release-0.6.7.md),
[development notes](docs/session-0.6.7.md), and [mistakes and fixes](mistakes.md).

`zaman-sessiond` owns game sessions for Zaman OS. It is a persistent user
service positioned between frontends, the emulator registry, InputPlumber, and
the user's systemd manager.

Frontends do not select emulator binaries, construct shell commands, manage
controller identities, or kill emulator processes. Their contract is:

```text
zamanctl launch SYSTEM_ID /absolute/path/to/ROM
```

`zamanctl launch` blocks until the session finishes. This gives Pegasus and other
frontends ordinary launch-and-return behavior while the persistent daemon
continues to own supervision.

## D-Bus contract

```text
service:   com.kawnelectro.Zaman.Session1
path:      /com/kawnelectro/Zaman/Session1
interface: com.kawnelectro.Zaman.Session1
```

Methods:

- `Launch(system_id, rom_path)` resolves and starts a registered emulator.
- `Stop()` idempotently requests termination of the active game session.
- `OpenMenu()` asks the standalone menu to take foreground control.
- `CloseMenu()` restores the saved library/game return target.
- `ToggleMenu()` serializes the corresponding open or close operation.
- `Resume()` is the backwards-compatible alias for `CloseMenu()`.
- `ExitGame()` accepts the system menu's explicit exit selection.
- `Status()` reports state, selected system/emulator/ROM, result, and error.
- `ForegroundStatus()` reports foreground, return target, and transition reason
  without changing the stable `Status()` tuple.
- `MenuStatus()` reports whether the menu is open, its generation, and the last
  transition reason.
- `MenuPresented(generation)` lets the menu acknowledge successful surface
  creation; a missing acknowledgement causes sessiond to restore the prior UI.
- `Version()` returns the interface implementation version.

Signals:

- `MenuOpened(generation, reason)` tells a shell to present its system menu.
- `MenuClosed(generation, reason)` tells the shell to dismiss that menu.
- `MenuInput(generation, event, value)` forwards normalized InputPlumber `ui_*`
  events while the menu owns controller input.

The generation monotonically identifies each menu opening. A menu client reads
`MenuStatus()` when it starts, subscribes to the signals, and ignores input from
an obsolete generation. A replacement standalone menu can implement this
client without direct system-bus or controller access.

## Full-screen menu client

`zaman-menu` is the reference standalone client for this contract. It owns the
well-known name `com.kawnelectro.Zaman.Menu1` while healthy and stays connected
to the user D-Bus without creating a window. `MenuOpened` creates a native
Wayland SDL2 desktop-fullscreen surface above the current Cage client and then
acknowledges the generation. `MenuClosed` destroys the surface so the
still-running Pegasus or game client becomes visible again. It does not open
evdev, identify controllers, or depend on Pegasus.

Phase A deliberately exposes one action: Resume. Back/B and Escape request the
same service-owned close operation. The existing `ExitGame()` API remains for
compatibility but is not part of this minimal menu surface.

Normalized `ui_up`, `ui_down`, `ui_left`, `ui_right`, `ui_accept`, `ui_back`,
and `ui_cancel` events drive the menu. Repeated press events are suppressed
until their matching release. Accept and Back activate on release so
InputPlumber observes the complete click before sessiond restores PASS mode;
Guide uses the same release-safe close behavior. Keyboard navigation remains
available for bench recovery. Sleep and Power Off are intentionally not fake
menu entries; they will be added after the polkit-governed power broker owns
those operations.

The renderer uses SDL2 and SDL2_ttf. `ZAMAN_MENU_FONT` can name an explicit
font; otherwise the client checks packaged Noto Sans and DejaVu Sans paths.
The production image should install one known font and set the environment in
`zaman-menu.service` if its path differs.

The canonical introspection contract is in
`interfaces/com.kawnelectro.Zaman.Session1.xml`.

## Registry

Registry schema v1 is loaded in ascending precedence from:

```text
/usr/share/zaman
/etc/zaman
```

`ZAMAN_REGISTRY_DIRS` can replace that search path for tests. Each root contains
`systems.d/*.toml` and `emulators.d/*.toml`.

Resolution is deterministic. The highest-priority emulator claiming a system
wins; a top-priority tie is an error. Emulator `argv` is an array, the
executable is absolute, substitutions occur per argument, and no shell is
involved. ROM paths are canonicalized and checked against the system's declared
extensions before launch.

The packaged NES/MesenCE entry expects:

```text
/usr/lib/zaman/emulators/mesence/Mesen
```

Bench-specific paths belong in `/etc/zaman/emulators.d`, never in the product
registry or frontend configuration.

## Supervision

Each game runs as the transient user unit `zaman-game.service` with:

- `Type=exec`
- `KillMode=control-group`
- bounded TERM-to-KILL shutdown
- automatic collection after exit
- explicit `ExecStart` argv
- inherited kiosk display/session environment plus registry overrides

InputPlumber composites and normalized D-Bus targets are discovered when the
daemon starts and refreshed while it runs. Device additions/removals rebuild
only the input listeners; they do not affect the game or foreground state.
No username, controller model, VID/PID, event node, or composite index is part
of the sessiond contract. Guide is reserved by setting InputPlumber PASS mode;
PASS remains active across library and game state so Guide is globally
available. Daemon shutdown restores NONE.

PASS mode reserves Guide as the normalized Zaman system action while leaving
the composite controller available to Pegasus or the game. A Guide press calls
the same serialized toggle path as `zamanctl menu toggle`. While the menu is
open, sessiond places discovered composites in ALL mode and forwards normalized
menu input over user D-Bus. Closing restores PASS. Natural game exit while its
menu is open closes the menu and returns to the library.

No long-hold action exists. Some controllers use a Guide hold for firmware
power-off; Phase A assigns no hold gesture or power action.

## State model

The public game state remains intentionally small and compatible:

```text
Idle -> Active -> Stopping -> Idle
                 \-> Failed
```

Foreground state is independent:

```text
library <-> menu
game    <-> menu
```

`return_target` exists only while foreground is `menu`. It is observable with
`zamanctl status`; opening the menu never ends or pauses the game session.

On daemon restart, the existing conservative recovery policy stops an orphaned
`zaman-game.service` and returns to the library. On menu-client loss, sessiond
restores the saved foreground and InputPlumber PASS mode. Cage itself remains
the foreground mechanism: mapping the menu makes it current, and destroying
that window reveals the preceding live client.

## Build and bench validation

The Q6A installation and ten-cycle Pegasus/MesenCE acceptance procedure is in
[`docs/phase-a.md`](docs/phase-a.md).

```bash
./tools/smoke-test.sh
```

The smoke test builds both binaries, runs unit tests, validates the registry,
starts an isolated daemon against temporary registry data, introspects the
D-Bus API, verifies a natural exit, and verifies an explicit `zamanctl stop`
terminates the entire transient unit and restores input interception.

With a controller connected, the menu lifecycle can be validated interactively:

```bash
./tools/guide-validation.sh
```

The graphical session must already have imported `WAYLAND_DISPLAY` and
`XDG_RUNTIME_DIR` into the user systemd manager. After `smoke-test.sh` has just
built and tested the same tree, avoid repeating that work with
`ZAMAN_SKIP_BUILD=1 ./tools/guide-validation.sh`.

The validator starts the real `zaman-menu` binary and asks for two ordinary
Guide presses. On the first menu, the controller's normalized primary accept
action selects Resume. On the second, Back/B selects the same action. It
verifies the visible full-screen surface, Guide-to-menu, retained ALL
interception, CloseMenu-to-PASS, and preservation of the supervised game unit.
It uses a synthetic process and does not qualify real Cage focus restoration or
MesenCE state preservation; those remain board acceptance tests.

## Distribution files

- `dist/systemd/user/zaman-sessiond.service`
- `dist/systemd/user/zaman-menu.service`
- `dist/dbus-1/services/com.kawnelectro.Zaman.Session1.service`
- `interfaces/com.kawnelectro.Zaman.Session1.xml`
- `registry/`

Packaging is intentionally thin: install files to their declared paths. Do not
generate registry or policy data in Debian maintainer scripts. This keeps the
later Buildroot integration mechanical.

## Remaining work

- Install and validate the production user units in the kiosk login session.
- Add suspend and shutdown through a polkit-governed power broker.
- Add emulator lifecycle adapters before treating `ExitGame()` as production-safe;
  the current bounded systemd stop path is validated only with synthetic games.
- Qualify MesenCE launch, autosave, natural exit, and graceful exit as a
  production session.
- Add owned-key emulator configuration generation.
- Replace path-based frontend launches with library game IDs when the Zaman
  library service becomes authoritative.
- Add frontend-facing progress and session-ended signals without breaking the
  versioned interface.

## Transfer Games (0.6.6 source candidate)

The library menu has Resume, Transfer Games, Reboot and Shut Down. Transfer runs in a separate fullscreen Python/QML window, while the same menu remains mapped below it. Sessiond owns readiness and input handoff. `zamanctl transfer` (or `zaman-transfer --gui`) launches from the library; it is refused during games or another foreground operation. The CLI acknowledges the reserved launch, not rendered readiness.

Install the paired transfer 0.4.0 application and `dist/systemd/user/zaman-transfer-gui.service`. The unit is on-demand only; do not enable it. See `docs/deploy-0.6.6.md` for verification, backup, installation and rollback. No automatic Pegasus reload or background-transfer mode is included yet.
