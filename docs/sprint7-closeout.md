# Sprint 7 — accepted baseline, 19 October 2026

The owner accepted Sprint 7 after device testing. Feature development is closed;
the next period is dedicated to rigorous testing and fixes for reproduced bugs.
This is an engineering baseline, not a claim of completed production validation.

## Delivered

- Background reception from stock LocalSend clients into a durable SQLite inbox.
  Reception survives closing the menu. Registry evidence determines unambiguous
  destinations; ambiguous games wait for controller-operated classification.
- Staged validation and import preserve existing games and keep pending work
  independent of notification history. Gameplay is not interrupted by questions.
- Receiving progress bars, clear phase changes and controller navigation. X
  dismisses finished records and Y clears history, consistent with Notifications.
- Automatic recommended artwork and information lookup after a destination is
  known. Lookup failures do not block importing; the user can retry manually.
  Lookup uses the existing Skyscraper 3.21.0 installation and configuration.
- Artwork & Game Info offers a collection list including All, a prominent
  recommended action, optional configuration, activity, results and GUI answers
  to interactive questions. Selection is stable across status polling.
- Receiving and artwork progress are distinct. Artwork lookup does not remain
  on a misleading completed receiving bar. Completion copy directs the user to
  close the menu to update the library.
- ROM refresh is separate from Steam and Native Games refresh ownership.
- Gold focus matches the main menu. Collection uses the compact Options layout;
  large primary actions highlight only when focused. Artwork has an image icon;
  Notifications keeps its bell.
- Menu dismissal reverses the entry slide. Normal closing retains input ownership
  until the layer hides, then restores it through the existing helper. Interrupted
  entry, duplicate close requests, unmapping and stalled animation have recovery
  paths. Emergency helper cleanup remains immediate.

The publication inventory records whether the optional row-removal animation
layer is active. When active, it retains retiring cards during the slide/fade and
keeps history navigation consistent across Transfer, Artwork and Notifications.

## Validation evidence

The owner confirmed a successful transfer above 4 GiB, destination selection and
import, successful manual artwork lookup, and an end-to-end GBA import with artwork
and details. Earlier Resident Evil 4 failure involved a mismatch between advertised
and actual file length; it did not establish a general 4 GiB limit.

The owner supplied 61 passing Python tests, menu navigation tests and both Qt scale
tests for the transfer-stage baseline, then accepted the subsequent UI updates.
The final consistency update passed 13 local JavaScript suites and nine updater
tests. The owner's final response was "Works" and approval to close the sprint.
Do not infer a detailed final Qt test transcript from that response.

## Testing focus

- Transfers while gaming, offline artwork lookup, sender interruption, low storage,
  duplicate names, cancellation, restart/reboot recovery and companion-file cases.
- Receiving, artwork, import and library visibility as separate milestones.
- History dismissal/cleanup without loss of pending imports or game data.
- Guide and Start+Select, input handoff, repeat open/close, controller reconnect,
  multiple controllers, Steam/Dukkan return and emulator exit/autosave.
- Longer gaming sessions with concurrent receiving and deferred artwork work.

Keep existing activation/rollback records and package directories on the device.
Publication does not switch working branches or alter the running installation.

## Deferred

Controller file browser; screenshots/recording and gallery; update utility; PC
game streaming. Native emulator-menu access remains a separate backlog item.
None is introduced by this publication.
