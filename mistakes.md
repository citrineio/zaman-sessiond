# Mistakes and fixes

## 2026-09-26 — Menu text overlapped after adding volume controls

**Observed on the Q6A:** The six-row game menu drew action labels on top of their descriptions. The Volume row was functional, but the menu was difficult to read.

**Cause:** Adding separate Volume and Mute rows changed `RowLayout` to 88-pixel rows while rendering labels and descriptions with the previous 57- and 27-point fonts. The description began 51 pixels below the top of each row, inside the label's rendered area. A position-only check confirmed that rows fit above the footer but did not measure text extents or show the actual SDL output.

**Correction:** Removed the dedicated Mute row. Selecting Volume and pressing A now toggles mute, preserving the underlying volume. The five-row layout uses 108-pixel rows, places descriptions in the right pane according to selection, and displays 0% while muted. The user tested the revised menu on the Q6A and reported that it was functional and visually correct.

**Prevention:** When changing menu row count or typography, inspect SDL renderer previews for both game and library contexts at the target output sizes. Check label bounds, description placement, slider/percentage clearance, and footer clearance; a synthetic image or row-boundary assertion alone is insufficient. Include a real device screenshot before calling the UI accepted.

## 2026-09-26 — Uncompiled menu change contained an out-of-scope variable

The first 0.6.7 source package used `error.is_none()` inside `MenuSurface::input`, where `error` was not defined. The condition belonged to the rendering branch after `error` was computed. The user found this at `cargo test --locked --bin zaman-menu`; the source was corrected. Run the menu binary's compile/test command before distributing a candidate whenever a Rust toolchain is available.
