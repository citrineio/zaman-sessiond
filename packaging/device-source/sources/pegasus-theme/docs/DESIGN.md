# Zaman menu styling — 0.2.2

Visual source: `zaman-quickshell-prototype.tar.gz`, archive revision 11, modified 2026-10-08 07:52 UTC. These values come from the actual `qml/shell.qml` and `MenuText.qml`, rather than an older screenshot or the previous brown palette.

| Token | Value | Use |
| --- | --- | --- |
| ink | `#080C13` | Full-screen backdrop; menu scrim hue rendered opaque here |
| panelTop | `#171D28` | Upper panel gradient |
| panel | `#12161E` | Middle panel gradient |
| panelBottom | `#17191F` | Lower panel gradient |
| gold | `#EAC28D` | Selection gradient start |
| caramel | `#CB986C` | Selection gradient end |
| ivory | `#F0E5D6` | Headings |
| text | `#ECEEF1` | Unselected labels |
| muted | `#AEB8C5` | Supporting copy |
| dim | `#97A2B1` | Unselected details |
| line | `#405D718C` | Translucent blue-gray panel border (ARGB) |
| divider | `#265C6E85` | Quiet divider (ARGB) |
| focusBorder | `#F4D2A4` | Selected control border |
| selectedText | `#211E1B` | Text on selected controls |
| selectedDetail | `#594433` | Supporting text on selected controls |
| logo | `#EEE1CF` | Supplied bird and wordmark |

The theme uses a platform Sans Serif face for headings and labels, matching the latest menu, with IBM Plex Mono for metadata and hints. The old Saira Condensed assets and license notices remain bundled but are no longer loaded by the default interface.

The menu's 24-unit panel and 13-unit row corner radii are reused. Controls use a horizontal sand-to-caramel gradient with a pale warm border. Game cards share this focus treatment, while artwork stays visible. The header, hero, detail panel, collection picker and preferences reuse the same surfaces. No blur, extra QML module, icon font or Quickshell runtime dependency is added to Pegasus.

The supplied logo source is retained unchanged in `assets/branding/zaman-logo-source.png`. The bird and wordmark PNGs use the same crop regions and ivory tint as the menu, with black converted to transparency. They are static assets to avoid repeated canvas pixel processing in Pegasus.

Layout continues to use the original 1920 × 1080 logical canvas, scaled for TV and handheld resolutions. Pegasus model and launch API calls remain from the supplied theme. Home adds a hero-action focus row between tabs and recent games. The action buttons use QtQuick TapHandler for mouse/touch activation with release-within-bounds semantics; a hover-only MouseArea provides cursor and highlight feedback. Clicking the hero artwork opens details, and the buttons have priority over the artwork hit area. Reduced-motion preferences and the original keyboard/controller flow remain supported.
