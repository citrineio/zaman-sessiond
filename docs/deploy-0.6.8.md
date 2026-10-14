# Zaman Sessiond 0.6.8 source candidate

## Scope

The main menu has a System row where Volume used to be. System contains the
existing Volume slider and a Network screen. Network shows the active Ethernet
and Wi-Fi connections and the first IPv4 address. Wi-Fi can be toggled,
disconnected, scanned, and joined from a paginated list. Open networks join
directly. WPA Personal networks open a controller keyboard grid: use the D-pad
to select a key and A to enter it. Its bottom row offers Space, Erase, Mode
(lowercase / uppercase / symbols), Show/Hide, and Connect. X also cycles the
keyboard mode while entering a password. The password starts masked and can
be revealed temporarily by selecting Show; visibility resets on exit. It is
never logged or passed on the command line. The indicator and character count
update in the labeled field above the keyboard as keys are entered or erased;
the pane shows when Connect is ready.
Previously saved secure Wi-Fi profiles reconnect without asking for a password.
If a saved profile fails, selecting that network again offers password entry.
Network status updates every three seconds while its screens are open. Scans
and refreshes run in the background; Back and navigation remain responsive.
Disconnect stops automatic connections on the Wi-Fi device until a network is
selected manually or Wi-Fi is switched off and back on. Switching Wi-Fi on
restores automatic connections for that device.
Back steps up one menu level; B/Guide on the main menu resumes as before.

The menu uses the existing Session1 D-Bus actions for game and power controls.
No sessiond action, signal, or tuple signature changes. All network operations
run on a blocking worker. NetworkManager's `nmcli` must manage the intended
Wi-Fi interface, and the kiosk user must be authorized by NetworkManager to
change its state and activate connections. Enterprise (802.1X), WEP, hidden
SSIDs, and captive portal sign-in are outside this version's Wi-Fi UI.

## Build and deploy on the Q6A

Verify `nmcli device status` shows the Wi-Fi device as managed. Keep a local
keyboard or an Ethernet connection during Wi-Fi acceptance. From this checkout
on the kiosk user account:

```bash
cargo test --locked
cargo build --locked --release --bin zaman-sessiond --bin zaman-menu --bin zamanctl
./install.sh
zamanctl version
```

`install.sh` accepts an installed 0.6.7 or 0.6.8, checks that game and transfer
services are idle, backs up the three binaries, installs 0.6.8, restarts the
menu/session services, and rolls back if installation fails. Retain the printed
backup directory until device acceptance succeeds. It does not modify network
profiles or replace the separate Transfer Games GUI.

## Acceptance

1. Open in the game library and in a running game. Verify System > Volume still
   changes and mutes audio, and B returns through System to the main menu.
2. Verify the wired and wireless connection names and IP address on Network.
   Change the connection outside the menu and verify its status updates after
   the next three-second poll without selecting Refresh. Refresh manually, scan, inspect a
   second page and use Back while the scan is still running.
3. Join an open AP, then a WPA Personal AP with the controller. Exercise the
   keyboard's lowercase, uppercase, symbols, Space, Erase, Show and Hide keys.
   Use X to cycle modes without moving the selection. Check that visibility
   resets after leaving the password page, that the masked indicator
   increments and decrements, and that Connect becomes ready at eight
   characters. Check that Hide conceals the password again and errors do not
   contain it. Reconnect a
   previously joined secure AP without re-entering its password. Confirm a
   failed saved profile offers re-entry, then test Wi-Fi Off/On and Disconnect.
4. Verify Resume, Exit Game, Transfer Games, Reboot and Shut Down paths according
   to the same safe test procedure used for 0.6.7; do not actually power down
   during a game save. Confirm menu navigation remains responsive during scan.
5. Test at 720p/800p/1080p/1440p with representative long SSIDs. Review
   labels, right pane, slider, footer and selection markers on the Q6A display.

The build and renderer previews are host checks. NetworkManager authorization,
Wi-Fi association, and live controller behavior require the Q6A acceptance run.
