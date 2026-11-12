# Sprint 1: managed storefront candidate

Base: `snapshot/quickshell-20261012` (404c021), paired with the
`sprint1/storefronts` branches of `citrineio/zaman-menu` and `citrineio/dukkan`.
The recovery acceptance round passed on Kadhem's Q6A on 2026-10-14.
Device builds and activation were performed by the owner. Local Python/JS
checks alone do not establish Rust/Qt compilation or device acceptance.
See `sprint1-acceptance-20261014.md` for the exact tested scope and deferred work.

## Session contract

- Pegasus stays tiled and borderless in `library`.
- Each managed storefront runs as its own transient user service:
  `zaman-store-dukkan.service`, `zaman-store-steam.service`.
- Descendant windows are matched by PID/cgroup, including Xwayland; they are
  placed in `store-dukkan`/`store-steam`, with Dukkan fullscreen. Steam retains its own window/fullscreen policy.
- The resident layer-shell menu does not disable storefront fullscreen.
- Home hides the storefront workspace while preserving its process. Reopening
  Storefronts resumes an existing managed client instead of launching a duplicate.
- Closing a storefront is explicit and confirmed. SIGTERM, an 8-second stop
  budget and systemd cgroup cleanup apply. Failed/uncertain stops retain the
  reservation until a fresh stop succeeds. Startup recovers orphaned store units.
- Legacy `Status` remains the emulator game session; it never represents Steam.
  The additive `StorefrontStatus` reports id, generation, phase, Guide owner,
  detected Steam game and error. `StorefrontAction(action,id,generation)` supports
  launch/home/close. Home/close require the current generation (decimal string).
- `ForegroundStatus` reports library/storefront/game. A detected Steam game is
  `game` with its client identified separately by `StorefrontStatus`.
- Game windows are detected by an owned `steam_app_<nonzero id>` WM class or a
  nonzero `SteamAppId` in the owned window process environment. This must be
  qualified against real native and Proton games; games escaping the service
  cgroup or lacking these identifiers are not currently classified.
- Zaman never invokes emulator `ExitGame` on Steam. While a detected Steam game
  runs, the menu offers Resume, confirmed Close Steam, and system settings.
  Exit only the game inside Steam. Close Steam shuts down the managed client
  and its game together. Cancel leaves both running. Return to
  Library remains blocked during gameplay. Closing only the game inside Steam
  allows the client window to become primary again.
- This does not yet supervise games launched from Dukkan/Flatpak as independent
  sessions, persist installs beyond Dukkan UI exit, or add notifications.

## Source build (owner runs; nothing installed by the assistant)

Fetch into your existing clones without overwriting local edits. Prefer separate
worktrees for this candidate. Record the installed version and current branch.
Build sessiond and both development helpers together:

```bash
cargo test --locked --offline
cargo build --release --locked --offline
```

Use the sessiond helper as canonical. `zaman-menu/apply.py` recognizes both
published helper baselines and retains the snapshot's parallel identity reads;
it is unnecessary when using this sessiond candidate directly. If you use it,
run it before the build and rebuild the helpers.

Dukkan uses the existing CMake workflow from its README. Its only change is a
stable Wayland desktop/app identity. Use the existing Qt/SDL dependencies.
Do not upgrade or install packages as part of this candidate automatically.

## Configure executable paths

Close any manually launched Dukkan/Steam before the first managed launch. An
already-running external Steam client cannot be adopted into a new cgroup.
Check the actual executables:

```bash
command -v steam
ls -l "$HOME/projects/dukkan/build/dukkan"
mkdir -p "$HOME/.config/zaman/storefronts.d"
```

Copy the two `.toml.example` files from `registry/storefronts.d/` to that
configuration directory as `dukkan.toml` and `steam.toml`, only if there is no
existing config. Edit their absolute argv paths to match your device. Dukkan's
example uses `/home/kadhem/projects/dukkan/build/dukkan`; Steam's uses
`/usr/bin/steam -gamepadui`. They are examples, not discovered device paths.
`argv` is passed directly to systemd, without shell parsing. Optional `env` and
`workdir` use the same validation as emulator commands.

Keep `native_guide = false` initially. After Steam overlay acceptance, set it
true in `steam.toml` and close/relaunch the managed Steam client. This passes the
controller through with InputPlumber NONE while Steam is active, so Steam receives
Guide. Manual Zaman menu opening still acquires ALL; closing restores the saved
mode. F12/your existing shortcut remains a Zaman escape route. Start+Select is
verified on this Q6A with the shortcut guard admitting `storefront`. Preserve
its launcher/project environment adapter. To revert delegation, set false and
close/relaunch Steam.

Deploy only after your build passes, using your existing sessiond installer and
resident menu setup. Ensure `ZAMAN_MENU_QML` points to the matching menu checkout
(or its default `$HOME/zaman-quickshell-prototype/qml/shell.qml`). The new helper
requires the additive StorefrontStatus API; update both repositories together.
The installer refuses to replace binaries while either store unit is active.

## Device acceptance

Run each case from Pegasus using only a controller, at 1080p and 1440p:

1. Guide > Storefronts > Dukkan. Verify `store-dukkan`, borderless fullscreen.
   Open/close Guide ten times. Check the Dukkan window stays fullscreen and
   returns to its prior focus. Pegasus must never tile beside it.
2. Home returns to Pegasus; reopening Dukkan retains its browsing state. Close
   storefront requests confirmation and returns to Pegasus after cleanup.
3. Repeat for Steam. Start a native game and a Proton game. Read
   `zamanctl storefront-status`; verify `game=true`, no emulator Exit Game action, confirmed Close Steam available, and that exiting inside Steam keeps Steam running/fullscreen.
4. Enable native Guide only after the overlay works. Check short presses,
   repeat presses/holds, Steam overlay close, F12 Zaman menu, and return to library.
5. While Guide is open in each storefront, unplug/reconnect the controller,
   restart InputPlumber, and disconnect while holding a direction. The page
   stays open, held navigation stops, and input recovers without a new menu.
6. Kill the storefront's main process, test an invalid executable config, and
   test a client exiting during startup/Guide presentation. Expect Home recovery,
   no orphan interception, a journal error for failures, and usable relaunch.
7. Return Home with a storefront running and launch an existing emulator game.
   Check normal Guide/exit/autosave and foreground restoration remain intact.
8. Rapidly select a launch/close action and press Guide. The UI must wait for
   helper-idle, dispatch once, reject stale generations and avoid focus races.

Diagnostics:

```bash
zamanctl status
zamanctl storefront-status
swaymsg -t get_tree
systemctl --user status zaman-store-dukkan.service zaman-store-steam.service
journalctl --user -u zaman-sessiond.service -n 100 --no-pager
```

CLI launch: `zamanctl storefront launch dukkan` or `... steam`.
Return/close: `zamanctl storefront home` or `... close` (menu must be hidden).
An accepted request is not proof the UI is ready; observe StorefrontStatus.

## Rollback

Close any games and managed storefronts before replacing binaries. If normal
close is unavailable, stop the two managed user units explicitly, then stop
sessiond and the resident renderer. Restore the installer's saved binaries and
the previous menu checkout/service path; rebuild the previous helper if needed.
Keep or remove only the new storefront config files you created. The daemon API
and helper must be rolled back together. Restart the previous services and verify
Pegasus, Guide, controller recovery, and emulator graceful exit.

## Tested Steam display setting

Keep Steam Settings > Display > Big Picture Mode > **Windowed ON**. This
resolved the Silksong rendering failure over repeated fresh managed sessions
and reboot on this device. Keep normal Silksong launch options. The failed
update-3 Pegasus-pause helper is rolled back and excluded from the checkpoint.
The low-level rendering cause remains unproven.
