# Emulator native menu (Sprint 7 hotfix)

The console's resident Quickshell menu exposes **Emulator Menu** for supervised
emulators with a validated `[emulator.menu]` registry section. It closes the
overlay, waits for controller release and input ownership restoration, checks the
supervised game window, and then sends its registered key using InputPlumber.

## Registry schema

For ARMSX2 standalone (`/etc/zaman/emulators.d/armsx2-standalone.toml`):

```toml
[emulator.menu]
method = "keyboard"
key = "KEY_ESC"
require_focus = true
hold_ms = 120
```

For RetroArch-based emulators:

```toml
[emulator.menu]
method = "keyboard"
key = "KEY_F1"
require_focus = true
```

Use `method = "unsupported"` for an emulator without a verified native menu
binding. `hold_ms` defaults to 0 and is validated to be at most 500 ms.
The `EmulatorMenuAvailable` D-Bus capability reports emulator support; it does
not guarantee the InputPlumber keyboard target is currently available.

The 19 verified local configurations are maintained in `/etc/zaman/emulators.d`;
**do not copy host-specific execution paths or BIOS settings from one device to
another.** Preserve each device's existing config and add only `[emulator.menu]`.

| Emulator registry ID | Menu key | Hold (ms) |
|---|---|---:|
| `armsx2-standalone` | `KEY_ESC` | 120 |
| `dolphin-libretro` | `KEY_F1` | 0 |
| `ppsspp-libretro` | `KEY_F1` | 0 |
| `retroarch-armsx2` | `KEY_F1` | 0 |
| `retroarch-beetle-pce-fast` | `KEY_F1` | 0 |
| `retroarch-beetle-wswan` | `KEY_F1` | 0 |
| `retroarch-clownmdemu` | `KEY_F1` | 0 |
| `retroarch-flycast` | `KEY_F1` | 0 |
| `retroarch-gambatte` | `KEY_F1` | 0 |
| `retroarch-gearsystem` | `KEY_F1` | 0 |
| `retroarch-mame` | `KEY_F1` | 0 |
| `retroarch-melondsds` | `KEY_F1` | 0 |
| `retroarch-mesen-s` | `KEY_F1` | 0 |
| `retroarch-mesen` | `KEY_F1` | 0 |
| `retroarch-mgba` | `KEY_F1` | 0 |
| `retroarch-mupen64plus-next` | `KEY_F1` | 0 |
| `retroarch-pcsx-rearmed` | `KEY_F1` | 0 |
| `retroarch-stella` | `KEY_F1` | 0 |
| `retroarch-swanstation` | `KEY_F1` | 0 |

## Validation and operating notes

* RetroArch `KEY_F1` was tested with the controller-driven menu.
* ARMSX2 `KEY_ESC` required a 120 ms key hold. Immediate press/release was
  acknowledged by InputPlumber but did not reliably open its native menu.
* The InputPlumber `keyboard0` virtual device can disappear when the controller
  is switched off. `UnknownObject` is not an emulator shortcut mismatch.
* The live Start+Select listener is independent of the resident menu; see
  `zaman-menu/shortcut/` after its original source is reviewed and captured.
* This source change **does not** implement production-grade least-privilege
  keyboard injection. The prototype Polkit rule is method-scoped but authorizes
  arbitrary keys to processes in the allowed group; it needs replacing before
  shipping a consumer OS image.

## Repository publishing

The Sprint 7 hotfix is based on `citrineio/zaman-sessiond` commit
`531413e0f7f4c4e93f83ac93c6da74d581afaf9c` and `citrineio/zaman-menu` commit
`d94ba6fe376f2b6b0bb1053a2257f3dd87dd1948`. The publication tool pushes only on explicit request.
