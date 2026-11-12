# Sprint 6 — accepted Steam integration

Accepted on device, 18 October 2026.

- Installed Steam games appear automatically in Pegasus with local artwork.
- Selected App IDs launch through managed native Steam Big Picture.
- Steam retains responsibility for per-game options and runtimes.
- Pegasus-origin return and native Guide / Start+Select handoff were exercised.
- Steam startup notices have an independent notification preference.
- DDNet install, appearance, launch, uninstall removal and reinstall passed.
- Reinstallation produced no duplicates; Silksong and Native Games remained intact.
- Focused Python and JavaScript checks passed.

An earlier context-change return failure was not reproduced across subsequent
tests. Its cause remains unresolved. Silksong launch-speed investigation is
separate follow-up work. Secondary unavailable-library and broader recovery
coverage are not claimed by these acceptance results.

Source locations:
- tools/steam-integration.py: exporter, watcher and launch adapter.
- tools/zaman_steam_library.py: discovery, metadata and artwork.
- tools/zaman_steam_session.py: game/session evidence.
- tools/zaman-steam: accepted native Big Picture startup wrapper.
- tools/test-steam-*.py: focused integration checks.

Runtime activation and rollback records remain in the existing device deployment
kit. Publication does not activate or rebuild components.
