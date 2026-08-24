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
- `Status()` reports state, selected system/emulator/ROM, result, and error.
- `Version()` returns the interface implementation version.

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

The current Guide gesture implementation is deliberately non-destructive while
the system menu and power broker are unfinished:

- releasing before three seconds requests no system action and restores PASS;
- three seconds emits and logs a system-menu validation event;
- releasing a long hold restores PASS;
- the validation event does not stop the game or change system power state.

Guide holds beyond this threshold are deliberately not part of the contract.
Some controllers, including the current bench controller, power themselves off
after roughly five seconds. Shutdown therefore belongs in the system menu and
must not depend on a longer Guide hold.

This establishes and bench-qualifies gesture timing without allowing unfinished
menu or shutdown paths to affect a running game.

## State model

The public service state is intentionally small:

```text
Idle -> Active -> Stopping -> Idle
                 \-> Failed
```

The internal game session retains the finer-grained
`Idle -> Starting -> Running -> Stopping` transitions in its journal.

## Build and bench validation

```bash
./tools/smoke-test.sh
```

The smoke test builds both binaries, runs unit tests, validates the registry,
starts an isolated daemon against temporary registry data, introspects the
D-Bus API, verifies a natural exit, and verifies an explicit `zamanctl stop`
terminates the entire transient unit and restores input interception.

With a controller connected, the non-destructive Guide thresholds can be
validated interactively:

```bash
./tools/guide-validation.sh
```

The validator asks for a short press and a three-to-four-second hold. It verifies
the expected event after each gesture and verifies that the supervised test game
remains active. It uses a synthetic supervised process and does not qualify
MesenCE or any other production emulator.

## Distribution files

- `dist/systemd/user/zaman-sessiond.service`
- `dist/dbus-1/services/com.kawnelectro.Zaman.Session1.service`
- `interfaces/com.kawnelectro.Zaman.Session1.xml`
- `registry/`

Packaging is intentionally thin: install files to their declared paths. Do not
generate registry or policy data in Debian maintainer scripts. This keeps the
later Buildroot integration mechanical.

## Remaining work

- Install and validate the production user unit in the kiosk login session.
- Add controller hotplug after a session has already started.
- Connect the three-second Guide event to game freeze and the full-screen menu.
- Add shutdown to the full-screen menu and qualify the physical power-key path.
- Qualify MesenCE launch, natural exit, and explicit Stop as a production session.
- Add owned-key emulator configuration generation.
- Replace path-based frontend launches with library game IDs when the Zaman
  library service becomes authoritative.
- Add frontend-facing progress and session-ended signals without breaking the
  versioned interface.
