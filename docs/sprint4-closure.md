# Sprint 4 closure — 16 October 2026

Sprint 4 delivered a resident tabbed menu, notification center/toast service,
application and system-event producers, and follow-up navigation/input corrections
for the Zaman retro console. The owner considers the sprint ready for closure.
This is a source checkpoint, not a claim of broad hardware certification.

## Included in the checkpoint

- Home / Notifications / Settings tabs, animated sizing, shoulder navigation and
  remembered section/selection state.
- Contextual positional action icons and arrows for directional navigation.
- Shared notification protocol owner/history, passive toasts and in-menu rendering.
- Stable-ID notification dismissal, slide/fade animation, DND and reminder controls.
- Named network changes, Bluetooth on/off, controller connection and battery events,
  optional audio events, Dukkan terminal results and the managed Steam reminder.
- Distinct display refresh choices wrapping in both directions.
- Guide return correction: fresh Guide ownership determines return interception,
  retaining controller/service identity checks and native Steam delegation.
- Public notification API and current project READMEs.

## Evidence and remaining checks

| Area | Recorded boundary |
| --- | --- |
| Activation | Owner reported successful Sprint 4 activation after rollback/readiness fixes |
| Guide follow-up | Owner ran the offline Rust helper tests/build and activated the correction; no recurrence reported so far |
| Menu review | Owner accepted Zaman menu on 16 October 2026; actual-source JavaScript and Python regression checks also ran during preparation |
| Input transitions | Close-origin tests cover 15 sections; physical combinations remain device checks |
| Notification protocol | Private D-Bus integration test supplied; results must be recorded per environment |
| Theme feedback | Artwork/focus/legend corrections supplied separately; on-screen acceptance has not yet been recorded for this release |
| Native Steam / rapid reconnect | Retained regression checks; no new exhaustive post-fix stress run is claimed |

A device observation that a bug has not recurred is useful evidence, not proof
that every reconnect, transport or menu section has been stress-tested. Future
regressions remain follow-up issues rather than silently expanding this checkpoint.

## Source ownership

- `zaman-menu`: menu, input-helper source copy, notification server/client/history/UI.
- `zaman-sessiond`: session lifecycle, canonical helper, system/audio notification
  producers, managed Steam reminder and Pegasus native-library exporter.
- `dukkan`: storefront and its verified terminal-result notification producer.
- Theme: separate repository/source package; no game supervision in the theme.

## Operational lessons

Keep the notification entry at the repository root so imported components remain
inside the Quickshell configuration boundary. Check renderer readiness through
socket heartbeats and direct logs. Guide status, native ownership and actual
interception mode are separate diagnostic facts; navigation working does not prove
Guide interception is enabled. Preserve a real controller's lease identity, and
do not restore a stale mode 0 when fresh ownership requires Zaman PASS.

No binaries, private notification history, device logs, save data or ROMs are
part of the public checkpoint. Source publication does not update a running device.
