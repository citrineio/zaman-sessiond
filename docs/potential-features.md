# Potential features

These are proposals, not implemented features or installed changes.

## Steam controller shortcut reminder

Requested by Kadhem on 2026-10-13 after validating managed Steam and the
Start + Select route to Zaman's fallback menu.

- On a managed Steam foreground launch, show a short Zaman notification:
  **"Zaman menu: press Start + Select."**
- Explain that this shortcut opens Zaman's system menu while Guide belongs
  to Steam. Use controller button glyphs alongside the text where available.
- Show once per Steam session, after the client is visible. Returning from
  Guide or reopening the same background client should not repeat it.
- Use the existing notification/toast design. It must not take keyboard or
  controller focus, intercept inputs, pause the game, or require dismissal.
- Allow the reminder to be disabled. Keep it available in a future Controls
  reference/help page after the user disables the startup reminder.
- A notification is appropriate here; this is not a modal dialog.

Acceptance: cold Steam launch shows the hint once; Steam Guide and game
controls still work; Start + Select opens Zaman's menu; subsequent menu cycles
do not repeat the hint; a new Steam session may show it again unless disabled.
