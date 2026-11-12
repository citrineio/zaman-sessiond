# Integration with Zaman

## Theme boundary

`theme.qml` passes Pegasus's `api` into `Shell.qml`. The theme reads `api.allGames`, `api.collections`, `api.keys`, `api.memory` and `api.device`. It launches the selected game by calling its documented `launch()` method and writes only the documented `favorite` property and its own `zaman.v1` memory entry.

The theme does not need access to the ROM database, D-Bus, systemd, or a shell-command execution plugin. Pegasus remains the source of frontend metadata; Zaman's established library and launch pipeline remain authoritative.

## Existing menu

The inspected `zaman-menu` implementation is an SDL2 client, separate from Pegasus. It uses opaque ink/obsidian panels, a caramel edge, ivory labels, and a gold selected row. This theme applies the same visual vocabulary to its QML screens.

That source exposes Resume and Exit Game. Earlier menu planning also mentioned Sleep and Power Off, but this package does not introduce those actions or change their implementation status. Guide ownership, menu opening policy, normalized `ui_*` input, and emulator lifecycle remain in the existing Zaman services.

## Board acceptance

After installing, verify these with the running Zaman stack:

1. Select Zaman in Pegasus; check font loading and artwork from actual metadata.
2. Navigate Home, Library, collections, favorites and search with the controller.
3. Launch a known-good game through the same metadata entry used before this theme.
4. Open the existing Guide menu, Resume, reopen it, then Exit Game.
5. Confirm Pegasus returns to the same selected game and that navigation works.
6. Check the intended output resolution and font readability at TV/handheld distance.

These checks require the board and real services. The supplied preview tests cover QML rendering and frontend behavior; they do not claim device, emulator, compositor or D-Bus qualification.

## Artwork and metadata

- Hero: background → screenshot → banner → title screen.
- Cards: banner → Steam grid → screenshot → box front → poster → tile.
- Details: box front → poster → tile → banner → screenshot.
- Empty or failed images reveal the theme's text/shape fallback.
- Existing multi-file titles continue to use Pegasus's built-in file chooser.
- Search is a case-insensitive title substring filter, combined with the current collection and favorites page. It does not search descriptions or files.
- Favorites update the writable Pegasus property. Recent play information comes from Pegasus's own statistics.
- Theme preferences and navigation state are saved under `api.memory["zaman.v1"]` through its get/set API.

## Scope limits

No scraping, save-state resume, network setup, transfer UI, emulator configuration, suspend control, telemetry, or external downloads are added. The current UI is English; Arabic typography and RTL layout need a dedicated localization pass.
