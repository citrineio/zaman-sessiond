# Sprint 1 recovery acceptance — 2026-10-14

Owner-reported device results, Dragon Q6A; these are not container runtime tests.

| Check | Result |
| --- | --- |
| Steam Windowed ON, normal Silksong options, fresh sessions and reboot | Pass |
| Start + Select opens Zaman menu in Steam | Pass |
| Close Steam -> Cancel leaves Silksong running | Pass |
| Confirm Close Steam closes client/game -> responsive Library | Pass |
| Emulator launch, Guide, normal exit -> Pegasus | Pass |
| Dukkan opens/closes normally through menu | Pass |
| Controller reconnect in game and menu | Pass |

This accepts the tested recovery round, not the entire original native/Proton,
resolution, abnormal-exit and race matrix. Fallout 4 and Another Crab's Treasure
never launch, including the prior Cage setup; compatibility investigation is
explicitly deferred. No working Windows/Proton-game acceptance is claimed.

Keep Steam Big Picture Windowed ON, the existing native ARM64 Steam wrapper and
native Guide choice. Update 3's Pegasus-pause helper remains rolled back. Steam
workspace routing, Close Steam generation validation, menu input restoration
before shutdown, and the Start+Select storefront guard form the working baseline.

The Start+Select reminder popup is recorded in potential-features.md and is not
implemented. Next work: independent installed-game sessions launched from Dukkan.
