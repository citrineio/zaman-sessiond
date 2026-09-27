# Transfer Games — 0.6.6 / 0.4.0 source candidate

> Historical deployment instructions from the 0.6.6 source-candidate stage. The user later reported successful laptop/phone transfers and passing transfer tests. Sessiond 0.6.6 was not separately pushed; see `docs/session-0.6.6.md` and `docs/release-0.6.7.md` for the update. The original full transfer bundle also contains the Python/QML assets and its own installer; this repository contains the sessiond portion.

This package implements one feature: the library-menu Transfer Games window. Sessiond is 0.6.6; transfer is 0.4.0. The accepted baseline is sessiond commit `7ac0f6bb8a39f0028ed3185d04f5cefa3528c2bf` and transfer 0.3.0.

Source changes and regression tests are written. Source reviews and static Python/XML/shell/patch checks are recorded under `evidence/`. **No project test, Rust format/build, dependency installation, GUI render, or hardware check was run by the assistant.** This is not an accepted release. Keep the existing checkout and backups. No commit/push until install and hardware acceptance.

## 1. Verify on your machine

Extract the package in your home directory. From the extracted directory run:

```bash
bash verify.sh "$HOME/zaman-lab/zaman-sessiond-0.6.3"
```

Use the actual existing checkout path if different. The script checks the exact accepted baseline and a clean tracked tree before applying the patch, runs `cargo fmt`, locked build/tests and release build, then the Python suite against the supplied transfer source. It does not install anything. It does not download/install a toolchain or Python dependencies on your behalf. Cargo may resolve its already-locked dependency cache in your own environment.

If a command fails, stop and share its output. After formatting, the patch may no longer reverse-apply exactly; do not reapply over a changed tree. Resume the individual build/test command instead. Source copies under `zaman-sessiond/` and `zaman-transfer/` are also supplied for inspection. The transfer patch is against the uploaded 0.3.0 archive, not a claimed Git commit.

Test-first source snapshots are in `checkpoints/`. They record tests before their behavior was implemented, including separate review-fix checkpoints. They were not executed here. Initial Rust checkpoint overlays the baseline; supplemental checkpoints apply to the intermediate stage described by their README/report. Do not overlay checkpoints on the final working checkout. The final suite is the acceptance target; missing-method failures from a baseline checkpoint alone are not evidence of correct final behavior.

## 2. Dependencies and session preflight

Install/provision dependencies yourself before proceeding:

- Existing sessiond/InputPlumber/Cage environment and Rust build requirements.
- System Python with PySide6 QtCore, QtGui, QtQml, QtQuick, QtDBus, and the native Wayland platform plugin. The installer checks these before changing installed files; it does not install them.
- `ip` for physical Ethernet/Wi-Fi address discovery; `avahi-resolve-host-name` and a working Avahi setup for friendly-name verification. Existing `avahi-publish-service` provides service advertisement. Optional `qrencode` generates the QR image; URL/PIN still work without it.
- Existing TLS/config/ROM permissions; the normal graphical user must belong to `zaman-roms`. A newly added group requires a logout/login before service access is reliable.

Use the board's supported package sources and Python ABI. No package names or availability are assumed beyond the executable/modules checked above.

Exit the running game and close any terminal transfer session. Check the normal user's service environment contains the existing Wayland display/session values; preserve the working Cage/session environment. The new unit uses `QT_QPA_PLATFORM=wayland`, does not enable itself, and does not supply a made-up display name.

Run the existing synthetic smoke harness after source tests if desired/required by your normal acceptance process, with the installed services stopped and restored on exit as in the previous deployment record. It needs InputPlumber; it is not a GUI/PC upload test. Capture output before sharing. Do not deliberately trigger power actions while a real upload/game is active.

## 3. Back up and install

From the extracted package directory, as the normal graphical user:

```bash
bash install.sh "$HOME/zaman-lab/zaman-sessiond-0.6.3"
```

This uses the release binaries you just built, backs up the existing binaries, transfer application/config/TLS/identity and any prior GUI unit, then installs transfer GUI assets and the on-demand user unit. It stops/restarts the existing menu/session daemon, checks version/status and prints introspection. It never enables the GUI at boot or changes the hostname. On an installation failure it invokes rollback. The backup path is printed and recorded in `~/zaman-pre-0.6.6-latest`; keep it private because it contains the existing TLS key.

The existing transfer installer preserves game bytes, TLS material and configured settings. As before, it normalizes ROM group permissions and the stored device identity. ROM files are not included in the application backup and rollback does not remove uploaded games. If you need a game-library backup, make it separately before testing overwrites.

The GUI verifies the configured hostname resolves through Avahi to a current physical LAN address. A failed check shows an IP fallback; no custom `1234.zaman` DNS entry is created. With multiple LAN interfaces, verify the displayed addresses are reachable from your PC. No physical LAN address means an actionable error and Start Again/Return. HTTPS certificate warnings and client-isolated Wi-Fi remain possible.

## 4. Hardware acceptance

Record results rather than assuming success from a running service:

1. Library menu shows Resume, Transfer Games, Reboot, Shut Down with existing fonts/geometry. Open Transfer Games by controller release. The fullscreen QML window is readable at native resolution.
2. Hidden menu/Pegasus receive no navigation or activation. Holding/releasing a button across opening/closing does not activate another action. Keyboard press/release behaves consistently. Close and reopen repeatedly; the same menu selection returns. A terminal `zaman-transfer --gui` from the library returns to the library.
3. Scan the QR, claim from a phone, and upload a known NES file. PIN/QR disappear on claim; connected, transfer activity and completed filename appear. Confirm bytes/hash and completion. There is active-operation status, not a byte-progress meter.
4. **From a PC without SSH, scanning, a hosts-file edit or a terminal:** type the displayed HTTPS URL, handle existing certificate trust behavior, enter the displayed PIN, and upload a file. Test both friendly name and fallback where available. Successful resolution on the console alone does not establish client compatibility.
5. Expiry, remote stop and inactivity leave the ended page with Start Again/Return. Start Again makes a fresh invitation and old credentials no longer authorize. Last completed-game feedback remains visible until retry.
6. Back/Guide idle ends and returns. During an upload: Stay continues; Finish Current Upload and Exit rejects new browser-queue work while admitted requests finish; Cancel Transfer and Exit wakes a stalled upload promptly and removes partial files. Completed files remain; a canceled replacement keeps original bytes until publication wins.
7. An already-running CLI listener causes a clear GUI error without killing/adopting it. Missing QR helper still permits PC URL/PIN; missing Qt/startup failure returns to usable menu. Verify the real service readiness timeout, GUI crash, daemon stop/crash and stale-event paths in a controlled session. A bus/systemd cleanup failure retains the busy lease and may require restarting the session; it must not launch a second GUI over an unconfirmed process.
8. During transfer, direct CLI/D-Bus game, menu and power requests are refused. Once it closes, existing game/resume/exit/reboot/shutdown behavior remains correct. Tests of real power actions must be deliberate. Restore a normal library session afterward.
9. Record when the uploaded NES appears using the installed Pegasus reload workflow. **Automatic reload and background transfer while gaming belong to the next release.** Closing this app stops its transfer session.

Capture diagnostics without posting QR/PIN/browser authorization secrets:

```bash
zamanctl version
zamanctl status
systemctl --user status zaman-sessiond.service zaman-menu.service zaman-transfer-gui.service --no-pager
journalctl --user -u zaman-sessiond.service -u zaman-menu.service -u zaman-transfer-gui.service -n 100 --no-pager
```

The GUI never puts invitation secrets in process arguments. QR payload goes through stdin. Forced kill/power loss cannot promise synchronous partial-file cleanup; the GUI unit allows five seconds for ordinary SIGTERM cleanup before systemd escalation. Idle address discovery runs only on open/retry, not continuously; reconnect then Start Again if the network changes.

## 5. Roll back

Exit any running game and terminal transfer first. From the extracted package:

```bash
bash rollback.sh
```

Or supply the printed backup directory explicitly. This restores the known application/config/unit snapshot and restarts the previous menu/session daemon. It leaves game files and source patches available. Do not run rollback with an unrelated backup tarball.

## 6. Closeout

After all build/test/install/hardware evidence passes, record the actual results in `zaman-sessiond/docs/session-0.6.6.md`, update the session conventions/changelogs, review the final diff and only then commit/push from your real repositories. No fake archive commit or accepted release is supplied here. The Pegasus theme archive needs no changes.

Next integrated feature: allow transfers to continue in the background while gaming, with coalesced automatic Pegasus library refresh.
