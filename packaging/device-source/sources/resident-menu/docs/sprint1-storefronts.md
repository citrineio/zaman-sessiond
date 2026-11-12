# Sprint 1 menu candidate

Use with `citrineio/zaman-sessiond` branch `sprint1/storefronts`, based on the
published `snapshot/quickshell-20261012`, and the matching Dukkan branch.
See the daemon's `docs/sprint1-storefronts.md` for setup, acceptance and rollback.

Adds equal Dukkan/Steam rows under Storefronts on Home. The Guide menu adapts
Resume, Return to library and Close storefront to the managed application.
Closing a storefront requires confirmation. Detected Steam games do not expose
Zaman's emulator Exit Game; exit inside Steam to preserve its client.

A storefront action first hides the layer and waits for the resident helper's
idle acknowledgement, proving input/focus restoration. Only then does the
one-shot session bridge pin a unique sessiond bus owner and request the action.
Duplicate requests and late readiness cannot overlap the handoff. Generations
remain decimal strings so Qt/JavaScript cannot truncate them. Failure reopens the
menu with an error. Helper failure cancels the queued action.

The helper accepts managed storefront context and includes its id/generation in
ready. Both repos ship the same helper, including the published sessiond snapshot's
concurrent identity reads. Rebuild it; a QML-only update is insufficient. No
production install or package installation was performed by the assistant.

Validation: existing and new Python bridge tests, actual QML-function JS tests,
shell syntax and bundle checks. QML rendering, Rust compilation, controller
hardware and Steam overlay acceptance remain with the device owner.

## Accepted recovery update (2026-10-14)

Kadhem verified Close Steam -> Cancel preserves the game, confirmation closes
Steam and Silksong and returns to responsive Library, emulator Guide/normal
exit/Pegasus return, Dukkan menu open/close, and controller reconnect in game
and menu. Start+Select works during Steam sessions. Close Steam is available
during a detected managed Steam game; it explicitly closes both game and client.
Keep the shortcut's device launcher/project adapter and storefront guard.
