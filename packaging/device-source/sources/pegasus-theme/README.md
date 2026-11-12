# Zaman for Pegasus — 0.2.2

A controller-first QML home and game library restyled from the latest Zaman Quickshell menu (archive revision 11, 8 October 2026). Blue-charcoal panels, warm ivory headings, muted blue-gray labels, rounded surfaces and a sand-to-caramel selection gradient now carry through Home, Library, Favorites, Preferences, game details, search and system selection. The header uses the same supplied bird and ZAMAN wordmark.

UI labels use the platform Sans Serif font, matching the current menu; metadata and control hints use bundled IBM Plex Mono. Existing Saira Condensed assets are retained for future customization. Search, favorites, saved preferences and game launching follow the original 0.1.0 implementation. Home navigation now includes the hero actions: Up from recent games focuses Play/Game details, Left/Right selects an action, Accept activates it, and Down returns to the same recent game.

This is a standalone implementation. GAMEOS was reviewed as a reference for the featured-game / library navigation approach; it is not required, and none of its QML or assets are included.

## Install on Zaman

Extract the archive, open a terminal in `zaman-pegasus`, and run:

```sh
sh install.sh
```

In Pegasus, open **Settings → Theme → Zaman**. Press **F5** while Pegasus is focused to reload after editing a theme file.

The installer uses `$XDG_CONFIG_HOME/pegasus-frontend/themes/zaman`, or `~/.config/pegasus-frontend/themes/zaman` when that variable is unset. It backs up an existing `zaman` theme before replacing it and prints the backup path. It does not change the selected theme automatically.

For a portable installation or another configuration location:

```sh
sh install.sh --themes-dir /your/pegasus/config/themes
```

To revert, select your previous theme in Pegasus. The original theme and any saved backup remain available.

## Screens

| Screen | Behavior |
| --- | --- |
| Home | Featured selected game and up to 20 recently played games, newest first. A new library shows alphabetically sorted games instead. |
| Library | All games, collection picker, title/recent sort, title search, four- or five-column grid. |
| Favorites | The same library controls restricted to games marked as favorites. |
| Game details | Artwork, description, metadata, play time, launch, and favorite toggle. Long descriptions scroll. |
| Search | Controller-operated on-screen keyboard; also accepts unbound printable keyboard keys. Search applies when you choose Done. |
| Preferences | Reduced/standard navigation motion, grid density, featured artwork on/off. |

The Home Play action launches the game normally. In-game **Resume** remains a function of `zaman-menu` / `zaman-sessiond`.

## Controls

The theme uses Pegasus's configured action mappings, so controller remapping stays in Pegasus. The footer names actions instead of assuming a controller face-button layout.

| Action | Result |
| --- | --- |
| D-pad / arrows | Navigate. Up from the first game row opens filters; Up again reaches tabs. |
| Accept | Play the selected game, activate a control, or type an on-screen key. |
| Escape | Close a theme dialog; otherwise open Pegasus’s native menu. |
| Controller Cancel / Back | Close a dialog; otherwise move to tabs, then Home, then open Pegasus’s native menu. |
| Details | Open the selected game's detail screen. |
| Filters | Open search. |
| Next page / Previous page | Cycle top-level tabs. |
| Page up / Page down | Move two rows in the library. |
| Guide | Owned by the existing Zaman system-menu path. The theme does not intercept it. |
| Mouse / touch | Select a game; double-click for details. On Home, click or tap Play/Game details; clicking the featured artwork opens details. Buttons show hover/press feedback. |

On Home, press Up from the recent-games shelf to reach **Play game / Game details**, Left/Right to choose, and Accept to activate. Up again reaches tabs; Down returns to the selected recent game.

To filter systems with a controller: Library → Down → Accept → choose a collection → Accept. To mark a favorite: select game → Details → Right → Accept.

## Runtime and integration

- Targets Pegasus's documented theme API and Qt Quick on **Qt 5.15 or later**.
- Uses QtQuick only; no QtMultimedia, QtGraphicalEffects, custom native plugin, or GAMEOS dependency.
- IBM Plex Mono is local and licensed under SIL OFL; retained Saira Condensed font notices are also in `assets/fonts`. The interface Sans Serif face comes from the platform.
- Reads game metadata and artwork from Pegasus. No demo games or ROMs are installed.
- Calls `game.launch()`. Your existing Pegasus metadata must already route launches through the intended `zamanctl` path. The theme does not rewrite commands or invent a new session interface.
- Stores view preferences and the selected game's file identity in `api.memory`, then restores the selection after Pegasus reloads the theme.
- Does not install or modify `zaman-sessiond`, `zaman-menu`, controller configuration, service files, or ROM metadata.

See [integration notes](docs/INTEGRATION.md), [design tokens](docs/DESIGN.md), and [validation results](docs/VALIDATION.md).

## Preview and development

The `preview` directory runs the actual theme against mock Pegasus data. Every pictured title and its vector artwork is a fictional test fixture. Your installed theme displays your actual games.

```sh
python3 -m pip install PyQt5
python3 preview/run.py --interactive
python3 preview/run.py --test --output preview/home.png
python3 preview/run.py --page library --output preview/library.png
```

Preview keyboard: arrows, Enter = Accept, Escape = Back, I = Details, F = Search, Q/E = previous/next tab. These preview bindings do not replace Pegasus's own mappings.

The source is organized as `theme.qml` (Pegasus entry point), `components/Shell.qml` (views and navigation), `components/Brand.qml` (tokens/fonts), `components/GameCard.qml`, `components/ActionButton.qml`, and `lib/Library.js` (model helpers).

## References

- Latest `zaman-quickshell-prototype.tar.gz`, revision 11, updated 2026-10-08: `qml/shell.qml`, `MenuText.qml`, and the supplied logo asset.
- Supplied Zaman Pegasus 0.1.0 archive, retained as the behavior and navigation baseline.
- [Pegasus theme structure](https://pegasus-frontend.org/docs/themes/overview/)
- [Pegasus theme API](https://pegasus-frontend.org/docs/themes/api/)
- [GAMEOS](https://github.com/PlayingKarrde/gameOS)
- [Saira Condensed source and license](https://github.com/google/fonts/tree/main/ofl/sairacondensed)
- [IBM Plex Mono source and license](https://github.com/google/fonts/tree/main/ofl/ibmplexmono)

Code is supplied for Kawn Electro's use and further development. No public release or distribution license for Kawn's code or branding is selected by this package; third-party fonts retain their included licenses.
