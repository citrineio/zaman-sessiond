# Surge Engine

The user confirmed `app/org.opensurge2d.OpenSurge/aarch64/stable` on 15 October
2026: version 0.6.1.3-1, commit
`9ac92e7e6d886f85acc6c8c1634ec524055a3cea8676fd67eaf2784591dc654a`.
The catalog follows the stable branch; this is not a version pin.

Surge Engine is now one entry in `catalog/games.json`. All authorization and
Pegasus export consumers use the shared catalog. Follow the current
[build, activation and editing commands](shared-catalog.md). The old
`activate-surge-catalog.sh` delegates to that complete deployment workflow.

Package availability was confirmed by the device output. Actual Play/Guide/Exit
and in-game controller acceptance remain device checks. No controller profile
or game-specific settings are included in the catalog update.
