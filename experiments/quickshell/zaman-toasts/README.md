The renderer and notification server now have one canonical home in
`citrineio/zaman-menu`, under `notifications/`. The Sprint 4 server owns the
notification name; the renderer owns no D-Bus name. `tools/zaman-toasts` remains a
compatibility command. Stop the old trial before owner-run activation.
