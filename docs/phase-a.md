# Phase A: Global Zaman Menu

## Architecture

The Q6A image runs Cage with `/usr/libexec/zaman-shell` as its startup
application. The shell imports the Wayland environment into the user systemd
manager, starts `zaman-sessiond` and `zaman-menu`, and then execs Pegasus.
Pegasus launches games through `zamanctl launch`, and sessiond owns each game
as the transient `zaman-game.service`.

Phase A keeps that design. Cage already permits several Wayland clients while
showing the most recently mapped full-screen window. `zaman-menu` therefore
creates a window only while the menu is open; destroying it reveals the
unchanged Pegasus or MesenCE window below it. No overlay protocol or second
compositor is involved.

Sessiond now owns two independent state dimensions:

| Game state | Foreground | Return target | Meaning |
|---|---|---|---|
| `Idle` | `library` | none | Pegasus is active |
| `Idle` | `menu` | `library` | Menu opened from Pegasus |
| `Active` | `game` | none | The supervised game is active |
| `Active` | `menu` | `game` | Menu opened over the game |

The existing six-field D-Bus `Status()` method is unchanged. The new
`ForegroundStatus()` method supplies foreground data, and `zamanctl status`
appends it to the existing output.

All menu requests enter one sessiond worker queue. On open, sessiond verifies
or starts the menu service, changes InputPlumber from PASS to ALL, records the
return target, emits `MenuOpened`, and waits up to three seconds for the client
to acknowledge its full-screen surface. Failure restores the previous state
and PASS mode. Close emits `MenuClosed`, restores PASS, and clears the return
target. Loss of the menu's well-known D-Bus name follows the same recovery
path. Natural game exit while its menu is open closes the menu to the library.

The existing Q6A InputPlumber device configuration is unchanged. Its
normalized D-Bus target already produces `ui_guide`, `ui_accept`, and
`ui_back`. PASS reserves Guide while preserving ordinary composite-controller
events for Pegasus/MesenCE; ALL is used only while the menu is active. A/B and
Guide complete menu-close on button release, before sessiond restores PASS.
This prevents InputPlumber's normalized D-Bus target from retaining a pressed
button across the foreground transition.

## Build and install on the Dragon Q6A

Run these commands in a checkout of this exact tree. Do not update live
binaries while a game is active.

```bash
cd ~/zaman-lab/zaman-sessiond
test "$(zamanctl status | sed -n 's/^state=//p')" = Idle
systemctl --user stop zaman-menu.service zaman-sessiond.service

./tools/smoke-test.sh
cargo build --locked --release --bins

sudo install -D -m 0755 target/release/zaman-sessiond /usr/libexec/zaman-sessiond
sudo install -D -m 0755 target/release/zaman-menu /usr/libexec/zaman-menu
sudo install -D -m 0755 target/release/zamanctl /usr/local/bin/zamanctl
sudo install -D -m 0644 dist/systemd/user/zaman-sessiond.service /etc/systemd/user/zaman-sessiond.service
sudo install -D -m 0644 dist/systemd/user/zaman-menu.service /etc/systemd/user/zaman-menu.service
sudo install -D -m 0644 dist/dbus-1/services/com.kawnelectro.Zaman.Session1.service /usr/share/dbus-1/services/com.kawnelectro.Zaman.Session1.service
sudo install -D -m 0644 interfaces/com.kawnelectro.Zaman.Session1.xml /usr/share/dbus-1/interfaces/com.kawnelectro.Zaman.Session1.xml

systemctl --user daemon-reload
systemctl --user start zaman-sessiond.service zaman-menu.service
systemctl --user --no-pager --full status zaman-sessiond.service zaman-menu.service
zamanctl version
zamanctl status
```

Expected initial values include `0.6.2`, `state=Idle`,
`foreground=library`, and `return_target=-`.

## Preflight validation

`smoke-test.sh`, used in the installation sequence above, formats and checks the
tree, runs the unit suite, validates D-Bus introspection, and exercises natural
and requested termination with synthetic games. It requires the installed
sessiond and menu services to be stopped so its isolated daemon can own the
D-Bus name.

```bash
cd ~/zaman-lab/zaman-sessiond
./tools/smoke-test.sh
```

To follow transition logs in another SSH session on an image with a readable
per-user journal:

```bash
journalctl --user -u zaman-sessiond -u zaman-menu -b -f
```

The Q6A image may return `No journal files were found` for `--user` queries.
The records are still available in the system journal under
`_SYSTEMD_USER_UNIT`; use:

```bash
sudo journalctl \
  _SYSTEMD_USER_UNIT=zaman-sessiond.service \
  _SYSTEMD_USER_UNIT=zaman-menu.service \
  -b -f -o short-monotonic
```

For a bounded fallback after reproducing a problem:

```bash
systemctl --user --no-pager --full status \
  zaman-sessiond.service zaman-menu.service -n 100
```

## Test A: Pegasus

Boot normally and leave Pegasus at a recognizable navigation position. Do not
press Guide until the manual CLI tests in A through C pass.

```bash
zamanctl status
PEGASUS_PID=$(pgrep -xo pegasus-fe)
test -n "$PEGASUS_PID"
echo "Pegasus PID: $PEGASUS_PID"
```

Repeat the following block ten times. After `menu open`, use the controller's
A button on Resume, visually confirm the exact Pegasus position returns, then
press Enter in the SSH terminal so the PID assertion runs.

```bash
for pass in $(seq 1 10); do
    echo "Pegasus pass $pass"
    zamanctl menu open
    zamanctl status
    read -r -p "Press controller A, verify Pegasus position, then press Enter here: " _
    test "$(pgrep -xo pegasus-fe)" = "$PEGASUS_PID"
    test "$(zamanctl status | sed -n 's/^foreground=//p')" = library
done
```

## Test B: MesenCE

Launch an NES ROM through Pegasus's normal metadata entry. Then capture the
emulator PID and session status:

```bash
MESEN_PID=$(pgrep -fo '^/usr/lib/zaman/emulators/mesence/Mesen( |$)')
test -n "$MESEN_PID"
echo "MesenCE PID: $MESEN_PID"
zamanctl status
```

Expected status includes `state=Active`, `foreground=game`, `system=nes`,
`emulator=mesence`, and the selected ROM. Repeat ten times, checking the live
game state after every return:

```bash
for pass in $(seq 1 10); do
    echo "MesenCE pass $pass"
    zamanctl menu open
    zamanctl status
    read -r -p "Press controller A, verify unchanged game state, then press Enter here: " _
    test "$(pgrep -fo '^/usr/lib/zaman/emulators/mesence/Mesen( |$)')" = "$MESEN_PID"
    test "$(zamanctl status | sed -n 's/^state=//p')" = Active
    test "$(zamanctl status | sed -n 's/^foreground=//p')" = game
done
```

## Test C: Toggle

Run once from Pegasus and once with MesenCE active:

```bash
zamanctl menu toggle
zamanctl status
zamanctl menu toggle
zamanctl status
```

The first status must show `foreground=menu` and the correct `return_target`;
the second must show the restored foreground and `return_target=-`. Repeat the
pair ten times in each application if any bounce or focus problem is observed.

## Test D: Controller

Only after A through C pass, verify the normalized system action without SSH
interaction at the console:

The optional interactive validator uses a synthetic supervised process and can
check the normalized Guide/A/B path before the real console pass. Stop the
installed services first; the script starts isolated copies and cleans them up.

```bash
systemctl --user stop zaman-menu.service zaman-sessiond.service
cd ~/zaman-lab/zaman-sessiond
ZAMAN_SKIP_BUILD=1 ./tools/guide-validation.sh
systemctl --user start zaman-sessiond.service zaman-menu.service
```

1. In Pegasus, press Mode/Home (`BTN_MODE`); verify the Zaman Menu appears.
2. Press B; verify the exact Pegasus navigation position returns.
3. Launch an NES game normally and record a recognizable game state.
4. Press Mode/Home; verify the Zaman Menu appears.
5. Press A on Resume; verify the same MesenCE process and game state return.
6. Repeat both paths ten times.

Then explicitly validate the release-safe Guide toggle:

1. Press Guide once to open the menu.
2. Press Guide once to close it.
3. Press Guide once; the menu must reopen on this first click.
4. Close with B, then repeat the sequence ten times from Pegasus and MesenCE.

Capture visual evidence with `grim`:

```bash
mkdir -p ~/zaman-test-shots
grim ~/zaman-test-shots/foreground-before.png
zamanctl menu open
grim ~/zaman-test-shots/zaman-menu.png
zamanctl menu close
grim ~/zaman-test-shots/foreground-after.png
```

Serve the screenshots to another device on the same network:

```bash
cd ~/zaman-test-shots
hostname -I
python3 -m http.server 8000 --bind 0.0.0.0
```

Browse to `http://ZAMAN_IP_ADDRESS:8000/` and stop the server with Ctrl+C.

## Test E: Controller late discovery and reconnect

Run this after A through D. It specifically guards the 0.6.1 startup-inventory
regression. Power the controller off, then restart InputPlumber and the Zaman
services so sessiond initially sees no normalized target:

```bash
systemctl --user stop zaman-menu.service zaman-sessiond.service
sudo systemctl restart inputplumber.service
systemctl --user start zaman-sessiond.service zaman-menu.service

SESSIOND_PID=$(systemctl --user show -p MainPID --value zaman-sessiond.service)
echo "sessiond PID before controller: $SESSIOND_PID"
```

Power the controller on without restarting any service. Within approximately
one second, this command must show a D-Bus target:

```bash
until busctl --system tree org.shadowblip.InputPlumber 2>/dev/null |
    grep -q '/devices/target/dbus'; do
    sleep 0.2
done

test "$(systemctl --user show -p MainPID --value zaman-sessiond.service)" = "$SESSIOND_PID"

sudo journalctl \
  _SYSTEMD_USER_UNIT=zaman-sessiond.service \
  -n 60 --no-pager -o short-monotonic
```

The journal must show `InputPlumber system input available`, the discovered
target, and `Subscribed to ...dbus0`. A single Guide press must now open the
menu from Pegasus. Repeat once with a game active, verifying its PID and state
survive the controller disconnect/reconnect.

Useful final evidence:

```bash
zamanctl status
pgrep -a -x pegasus-fe
pgrep -af '^/usr/lib/zaman/emulators/mesence/Mesen( |$)'
sudo journalctl \
  _SYSTEMD_USER_UNIT=zaman-sessiond.service \
  _SYSTEMD_USER_UNIT=zaman-menu.service \
  -b --no-pager
```

## Known limitations

- Phase A does not pause emulation; MesenCE keeps running while hidden.
- InputPlumber inventory is refreshed once per second. A controller or
  normalized D-Bus target that appears after sessiond starts is adopted without
  restarting sessiond, the menu, Pegasus, or the active game. A disconnected
  target can therefore take up to one second to become usable after reconnect.
- A sessiond restart retains the existing conservative behavior: it stops an
  orphaned game and safely returns to Pegasus rather than reconstructing game
  ownership.
- Game exit while the menu is open is handled directly. Pegasus exit remains
  owned by the existing Cage/display supervisor, which restarts the graphical
  session; sessiond does not scrape or supervise Pegasus's PID.
- Hardware focus, PID preservation, and controller acceptance cannot be
  certified by the synthetic automated tests; the Q6A procedure above is the
  release gate.
- User-unit journal access was unavailable in the supplied board snapshot.

## Phase B suggestion

After Q6A acceptance, add one service-owned action at a time—starting with a
polkit-governed power operation—without changing the foreground handoff. Keep
the menu a thin D-Bus client and keep emulator-specific lifecycle behavior out
of the renderer.
