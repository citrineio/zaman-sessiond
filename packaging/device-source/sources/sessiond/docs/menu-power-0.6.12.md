# Menu icons and power confirmation (0.6.12 source candidate)

This patch applies after 0.6.11 and its build and navigation corrections.
The owner confirmed audio navigation working on that baseline. Controller
mapping is also owner-confirmed fixed; this update does not change mappings.

## Behavior

The library menu has System, Transfer Games, Reboot and Shut Down. The game
menu has System, Exit Game, Reboot and Shut Down. Resume is removed from the
cells because B and Guide already dismiss the menu. Exit Game stays below
System and above the power actions; moving it to the bottom is deferred until
more users agree.

System has a gear icon, Reboot a circular arrow, and Shut Down a power icon.
They are drawn by the renderer at the output scale and use the cell's focus
colors; no symbol font or additional dependency is required.

Reboot and Shut Down open dedicated confirmation pages with a red warning
frame, large action icon and title, a consequence statement, and two large
Yes/No cells. No starts selected every time. Up/Down chooses, A/Enter selects,
and B/Escape cancels back to the originating power cell. Guide closes the
menu through the existing session daemon handling.

Only Yes sends the existing reboot/shutdown request. Game closure and save
handling remain the daemon's graceful stop path; the UI does not promise
that an unsupported emulator can preserve saves. Transfer suspension blocks
input, and removing an available power action dismisses its confirmation.
Pending requests continue to block duplicate activation. Direct CLI/D-Bus
power commands retain their existing behavior; these confirmations apply to
the menu.

## Owner deployment

Download `zaman-menu-0.6.12.patch` and send it through Tailscale Taildrop.
Receive it and check that the download is a patch:

```sh
sudo tailscale file get --wait "$HOME"
head -n 5 "$HOME/zaman-menu-0.6.12.patch"
```

The first line starts with `diff --git`. Apply to the working 0.6.11 source:

```sh
cd ~/zaman-sessiond-publish
git apply --check "$HOME/zaman-menu-0.6.12.patch" &&
git apply "$HOME/zaman-menu-0.6.12.patch" &&
cargo build --locked --release --bin zaman-sessiond --bin zaman-menu --bin zamanctl &&
cargo test --locked
```

If the original 0.6.12 test run failed at `remove test ROM` in
`registry::tests::rejects_top_priority_ties`, apply the follow-up
`zaman-menu-0.6.12-test-fix.patch` before installation. The original helper
used timestamp-only filenames; parallel tests could share a ROM and remove
it out from under each other. The correction reserves a private directory
for each fixture, cleans up only that directory and checks the actual
rejection reason. It changes test code only and keeps version 0.6.12.

```sh
git apply --check "$HOME/zaman-menu-0.6.12-test-fix.patch" &&
git apply "$HOME/zaman-menu-0.6.12-test-fix.patch" &&
cargo test --locked
```

Once the complete suite passes, the release build above succeeded, and no
game or transfer is active:

```sh
./install.sh
zamanctl version
```

Expected version: `0.6.12`. Retain the installer's backup directory if a
rollback is needed. No compilation, installation, preview or test execution
is performed by the assistant.

## Owner acceptance

- In library and game contexts, check four rows, readable icons and labels,
  System above Exit Game/Transfer Games, and no Resume cell. B and Guide
  still dismiss the root menu; Exit Game still uses its existing stop path.
- Open both power confirmations with controller A and keyboard Enter. No
  starts highlighted; a second accept on No returns without a power request.
  B/Escape returns to the originating cell, including when Yes is highlighted.
  Reopening resets focus to No. Guide dismisses the whole menu.
- Check the red frame, large title/icon, consequence text and both focus
  states at the connected monitor/TV resolutions. Review screenshots for
  clipping, label alignment and visibility at viewing distance.
- At an appropriate time, select Yes for each action from the library and
  from a supported running game. Confirm one action only and verify the
  existing graceful save/exit behavior. A failed request must leave readable
  recovery text and permit returning through No.
- Recheck slider-to-mute navigation and audio output switching. Bluetooth
  pairing/reconnection and display restoration after daemon restart/reboot
  still require acceptance; this patch does not establish those results.

Optional production-renderer fixtures (owner runs):

```sh
mkdir -p /tmp/zaman-menu-0.6.12-previews
for fixture in library game reboot shutdown reboot-confirm reboot-confirm-yes reboot-confirm-library shutdown-confirm shutdown-confirm-yes shutdown-confirm-library; do
    ./target/release/zaman-menu --preview "$fixture" 1920 1080 "/tmp/zaman-menu-0.6.12-previews/$fixture.bmp" paused || break
done
```

These fixtures only render the UI and do not execute power actions. Inspect
the output; rendering success alone does not establish visual acceptance.
