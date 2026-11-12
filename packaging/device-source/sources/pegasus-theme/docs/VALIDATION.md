# Validation — 2026-10-08

This document describes the restyled 0.2.2 package. The old 0.1.0 validation claims have been replaced with checks on the updated files.

## Escape fix

Escape now closes a theme dialog first; outside dialogs it propagates to Pegasus’s native menu. Controller Cancel propagates when already at the Home tab. The preview host checks propagation from both Home and Library, and confirms dialog dismissal stays within the theme. Actual native menu display still requires Pegasus.

## Hero input checks

Controller navigation now reaches the hero actions from the recent-game shelf. Tests confirm that Left/Right chooses Play or Game details, Accept activates the selected action, and Down returns to the same selected recent game.

Actual Qt mouse events exercise both hero buttons and artwork hit areas at 1920×1080, 1280×720 and 1280×800 on Qt 5.15.19 and Qt 6.12.0. Qt 6 also exercises touchscreen press/release events for both buttons at those sizes. Tests check the exact selected-game launch, details opening, no unintended details dialog during Play, release within button bounds after pointer movement, and the duplicate-launch guard. These tests invoke the input pipeline, not the button signals directly.

The supplied 0.2.1 buttons responded to mouse events in the local mock; the missing controller focus path was reproduced. The old active `zaman-chatgpt` theme and live VNC session were not available for reproducing the reported pointer failure. Check whether pointer input works elsewhere on the device if the new version still ignores clicks.

The Qt 6 preview expression helper now unwraps the evaluation-result tuple on both Qt versions, so assertions check their actual Boolean result.

## Automated checks

The included preview harness loads the actual theme against a mock Pegasus API and sends Qt keyboard events through the window. The interaction suite passes under Qt 5.15.19 and Qt 6.12.0 using the offscreen software scene graph.

It covers directional navigation, tabs, details/back/selection retention, favorites (including the last favorite), correct game launch callback and duplicate guard, launch identity restoration, collection filtering, controller-operated search, no-result recovery, empty/single/unplayed libraries, missing artwork and ROM data, long descriptions, preferences, 10,000 synthetic games, and live model-count changes. Final runs report no QML errors or binding loops.

The installer passes shell syntax checks and a fresh-install/backup check in an isolated temporary config directory. The new logo assets and QML components are included in the installed theme.

## Visual checks

Home, Library, Favorites, Preferences, game details, collection selection, and search are rendered from the actual QML. Home, Preferences and game details are checked at 1920 × 1080; Library at 1920 × 1080 and 1280 × 720; controller Search at 1280 × 800. Missing-artwork and long-text previews are also inspected. Screenshots use fictional preview games and are not captures from the Zaman board.

## Reproduce

```sh
python3 preview/run.py --test --output preview/home.png
python3 preview/run.py --qt6 --test
```

The first command requires PyQt5; the second requires PySide6. These are developer preview dependencies, not new dependencies of the installed Pegasus theme.

## Device acceptance still required

The real Pegasus binary, physical controller, game artwork, `zamanctl` metadata launches, Guide/menu focus handoff, emulator exit and return-to-library behavior require validation on Zaman. No deployment to the board was performed.
