# Sprint 3: Dukkan games in Pegasus

`zamanctl library sync [--dry-run]` reconciles the four authorized native
Flatpak games with the current user's installed `aarch64/stable` refs. The
daemon also runs a best-effort sync on startup. Dukkan requests sync after a
successful, postcondition-verified install or uninstall. A sync failure does
not undo a completed Flatpak transaction; rerun the CLI command to repair it.

The helper is installed at
`/usr/local/libexec/zaman/pegasus-library-sync.py`. It writes only:

- `~/.config/pegasus-frontend/metafiles/zaman-native-games.metadata.pegasus.txt`
- `~/.config/pegasus-frontend/themes/zaman/library-refresh/*.signal`
- `~/.local/share/zaman/pegasus-native-games/refs/*.game`
- `~/.local/state/zaman/pegasus-library-sync/sync.lock`

The Steam tile remains removed. The sync never touches Steam metadata, ROM
directories, Pegasus settings, or favorites. The generated marker files are
Pegasus existence checks, not executables. Each game gets an explicit fixed
`zamanctl installed-game launch <app-id>` command. `zaman-sessiond` still
authorizes only its four curated IDs, and the managed helper independently
verifies the installed user `aarch64/stable` Flatpak ref at launch.

The installed Pegasus build has no external CLI or D-Bus game-list reload
command. The active Zaman theme uses Qt 5's `FolderListModel` to watch the
single signal filename. After a changed sync, it calls Pegasus's in-process
`Internal.settings.reloadProviders()` method. Unchanged syncs leave the
signal alone. This needs Debian's `qml-module-qt-labs-folderlistmodel` package
and the theme overlay in `dist/pegasus-theme/theme.qml`. Press F5 once in the
visible Pegasus library after installing that overlay to load it. Routine
installs and uninstalls then refresh the game list automatically without
restarting Pegasus or the display service. The `Internal` method is tied to
Pegasus revision `6b322063`; recheck this integration when upgrading Pegasus.

## Q6A check

```bash
zamanctl library sync --dry-run
zamanctl library sync
sed -n '1,100p' "$HOME/.config/pegasus-frontend/metafiles/zaman-native-games.metadata.pegasus.txt"
```

With Kadhem's 14 October inventory, the metadata should contain SuperTux and
SuperTuxKart, and no Wesnoth, OpenTTD, or Steam entry. Open Native Games and
launch one game with the controller. Confirm Guide Resume/Exit, natural exit,
and return to a responsive Library. Run sync again to verify idempotence.
After a Dukkan install or uninstall, the relevant game should appear or
disappear visibly with no manual reload. A failed Flatpak listing must leave
the previous metadata unchanged.
