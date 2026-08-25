# zaman-sessiond

`zaman-sessiond` owns game sessions for Zaman OS. It is a persistent user
service positioned between frontends, the emulator registry, InputPlumber, and
the user's systemd manager.

Frontends do not select emulator binaries, construct shell commands, manage
controller identities, or kill emulator processes. Their contract is:

```text
zamanctl launch SYSTEM_ID /absolute/path/to/ROM
```

`zamanctl launch` blocks until the session finishes. This gives ES-DE and other
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
- `Resume()` closes an open system menu and returns controller input to the game.
- `ExitGame()` accepts the system menu's explicit exit selection.
- `Status()` reports state, selected system/emulator/ROM, result, and error.
- `MenuStatus()` reports whether the menu is open, its generation, and the last
  transition reason.
- `Version()` returns the interface implementation version.

Signals:

- `MenuOpened(generation, reason)` tells a shell to present its system menu.
- `MenuClosed(generation, reason)` tells the shell to dismiss that menu.
- `MenuInput(generation, event, value)` forwards normalized InputPlumber `ui_*`
  events while the menu owns controller input.

The generation monotonically identifies each menu opening. A shell reads
`MenuStatus()` when it starts, subscribes to the signals, and ignores input from
an obsolete generation. Pegasus, another frontend, or a standalone overlay can
implement this client without direct system-bus or controller access.

## Full-screen menu client

`zaman-menu` is the reference standalone client for this contract. It stays
connected to the user D-Bus without creating a window. `MenuOpened` creates a
native Wayland SDL2 desktop-fullscreen surface above the current Cage client;
`MenuClosed` destroys the surface so the still-running game becomes visible
again. It does not open evdev, identify controllers, or depend on ES-DE or
Pegasus.

The first visual milestone exposes the two actions already implemented safely
by the v0.3 session contract:

- Resume
- Exit Game

Normalized `ui_up`, `ui_down`, `ui_left`, `ui_right`, `ui_accept`, `ui_back`,
and `ui_cancel` events drive the menu. Repeated press events are suppressed
until their matching release. Keyboard navigation remains available for bench
recovery. Sleep and Power Off are intentionally not fake menu entries; they
will be added after the polkit-governed power broker owns those operations.

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

InputPlumber composites and normalized D-Bus targets are discovered at launch.
No username, controller model, VID/PID, event node, or composite index is part
of the sessiond contract. Guide is reserved by setting InputPlumber PASS mode;
cleanup restores NONE after D-Bus Stop, natural exit, or handled errors.

One Guide press opens the system menu immediately. sessiond explicitly places
all discovered composites in InputPlumber ALL mode, retains ownership after the
button is released, and forwards normalized menu input over the user D-Bus.
`Resume()` restores PASS and closes the menu. `ExitGame()` closes the menu and
ends the supervised transient unit. Natural game exit and errors close stale
menu state during cleanup.

No long-hold action exists. Some controllers use a Guide hold for firmware
power-off, so shutdown must be selected from the visible system menu instead of
depending on controller-specific timing.

## State model

The public service state is intentionally small:

```text
Idle -> Active -> Stopping -> Idle
                 \-> Failed
```

The internal game session retains the finer-grained
`Idle -> Starting -> Running -> MenuOpen -> Running/Stopping` transitions in
its journal. Menu state is separately observable through `MenuStatus()`.

## Build and bench validation

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
action selects Resume. On the second, Down followed by the primary accept
action selects Exit Game. It verifies the visible full-screen surface,
Guide-to-menu, retained ALL interception, Resume-to-PASS, and menu-selected
ExitGame. It uses a synthetic supervised process and does not qualify MesenCE
autosave or graceful termination.

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
- Add controller hotplug after a session has already started.
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
