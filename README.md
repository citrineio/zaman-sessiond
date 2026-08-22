# zaman-sessiond

`zaman-sessiond` is the game-session supervisor for Zaman OS. The current
v0.1 bench milestone validates the supervisor core on Armbian before the same
contracts are packaged for the production image.

## Current architecture

- `command`: validates an executable plus argument vector. It does not invoke
  a shell, so ROM paths containing spaces remain one argument.
- `inputplumber`: discovers every InputPlumber composite and normalized D-Bus
  target through ObjectManager. It contains no controller VID/PID, device
  index, username, or controller model.
- `systemd`: owns one transient user unit, `zaman-game.service`, created with
  `StartTransientUnit`. The unit uses `KillMode=control-group` and is never
  replaced if another session already owns its name.
- `session`: coordinates the state machine and guarantees normal-path cleanup
  for Guide, SIGINT, SIGTERM, natural game exit, and returned errors.

The CLI command interface is deliberately a validation surface:

```sh
./target/debug/zaman-sessiond -- /absolute/emulator "ROM path"
```

It is not the eventual public launch API. The next milestone exposes a
registry-backed Zaman D-Bus API so callers select a qualified system and ROM
instead of providing arbitrary executables.

## State flow

```text
Idle -> Starting -> Running -> Stopping -> Idle
                                      \-> Failed
```

At process startup, sessiond first restores InputPlumber interception to
`NONE`. It then enters `PASS`, launches the game, and restores `NONE` after the
session completes. A hard SIGKILL cannot run asynchronous cleanup, so the
startup reset is the crash-recovery mechanism.

## Validation

Run the complete bench smoke test:

```sh
./tools/smoke-test.sh
```

The script formats, checks, tests, and builds the crate, rejects a relative
executable, launches `/usr/bin/sleep infinity` as a transient game, sends
SIGINT to sessiond, and verifies that the game unit and input interception are
cleanly released.
