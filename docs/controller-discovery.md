# Native controller management

## Confirmed behavior and scope

The owner connected an 8BitDo Ultimate 2C Wireless over Bluetooth on 2026-10-06.
BlueZ reported Paired/Bonded/Trusted/Connected, and Linux exposed a gamepad with
bus 0005, vendor 2dc8, product 301b, `ID_INPUT_JOYSTICK=1`, `ID_BUS=bluetooth`.
The existing model-specific receiver rule matched product 310a instead.
InputPlumber saw the source but created no composite/targets.

On InputPlumber 0.78.0, the owner enabled the native `ManageAllDevices` property
and confirmed menu input works. The permanent integration reapplies this native
setting after every InputPlumber service start, using its installed upstream
profiles and existing device-specific exceptions. It supersedes the earlier
`zaman-generic-controller-policy.patch`; that generic YAML override is not shipped
by this integration. No Rust version is bumped before the planned 0.7.0 validation.

## Startup integration

`50-zaman-management.conf` adds an ExecStartPost to inputplumber.service.
`/usr/libexec/zaman-inputplumber-policy` sets `ManageAllDevices=true` and reads
back `b true`. InputPlumber announces its bus name before the Manager interface
has necessarily registered, so the helper retries readiness with bounded D-Bus
timeouts. Every start/restart reruns the helper; no second controller monitor,
new background daemon or device database is introduced.
The optional true/false argument is reserved for restoring a saved runtime
setting during rollback; the service always uses the default true.

Failure after the bounded attempts fails service startup and records the last
error in the journal. Inspect that error; do not claim input readiness merely
because the Bluetooth link is connected. The full installer restores previous
policy files/binaries on installation failure. The policy-only rollback is below.

Native management enables eligible upstream specialized profiles as well as the
generic fallback. Their virtual controller outputs can differ; button mapping,
extra capabilities and emulator compatibility still need qualification. Retain
the existing receiver exception that blocks its auxiliary keyboard/mouse sources.

## Pinned InputPlumber 0.78.0 findings

- Manager initializes `manage_all_devices` to false on each daemon start.
- Setting it to true triggers rediscovery of existing devices and enables
  configurations even without their own `options.auto_manage` flag.
- The shipped `65-generic_gamepad.yaml` matches `ID_INPUT_JOYSTICK=1` event nodes,
  sets maximum_sources=1 and defaults to xb360/mouse/keyboard targets. It preserves
  an exclusion for Framework keyboard modules misclassified as joysticks.
- Profiles sort by filename across local/vendor directories; local files win
  identical-filename ties. Existing eligible earlier profiles take precedence.
- The manager filters virtual output nodes but admits Bluetooth HID nodes, which
  can legitimately reside under `/devices/virtual/misc/uhid`.
- Each composite gets a D-Bus target automatically; sessiond already discovers
  composite/target additions and InputPlumber service-owner changes dynamically.

Pinned sources reviewed:

- <https://github.com/ShadowBlip/InputPlumber/blob/v0.78.0/src/dbus/interface/manager.rs>
- <https://github.com/ShadowBlip/InputPlumber/blob/v0.78.0/src/input/manager.rs>
- <https://github.com/ShadowBlip/InputPlumber/blob/v0.78.0/src/config/path.rs>
- <https://github.com/ShadowBlip/InputPlumber/blob/v0.78.0/rootfs/usr/share/inputplumber/devices/65-generic_gamepad.yaml>

## Receive and apply

```sh
sudo tailscale file get --wait "$HOME"
cd ~/zaman-sessiond-publish
git apply --check "$HOME/zaman-inputplumber-native-management.patch" &&
git apply "$HOME/zaman-inputplumber-native-management.patch"
```

This patch applies to the 0.6.12 source candidate without the previous generic
policy patch, and is independent of the Quickshell icon update. If the superseded
generic policy patch was applied to source, reverse it first with
`git apply --reverse --check` and `git apply --reverse` against that exact patch.
Do not reverse it if it was never applied.

## Policy-only installation: owner runs

Exit games/transfers and use the library with SSH available. No Rust rebuild or
full binary installation is needed. Restarting InputPlumber temporarily removes
virtual controllers; sessiond should rediscover them after startup.

Back up any existing versions of this helper/drop-in. If either earlier test
YAML was installed, move it into the backup so acceptance exercises stock
profiles. Preserve the existing 55-zaman receiver rule and vendor files.

```sh
cd ~/zaman-sessiond-publish
sudo -v
ZAMAN_INPUT_BACKUP=$(mktemp -d "$HOME/zaman-input-backup.XXXXXXXX")
sudo busctl --system get-property org.shadowblip.InputPlumber \
    /org/shadowblip/InputPlumber/Manager org.shadowblip.InputManager ManageAllDevices \
    > "$ZAMAN_INPUT_BACKUP/input-management"
for ZAMAN_INPUT_PATH in \
    /usr/libexec/zaman-inputplumber-policy \
    /etc/systemd/system/inputplumber.service.d/50-zaman-management.conf; do
    if sudo test -e "$ZAMAN_INPUT_PATH" || sudo test -L "$ZAMAN_INPUT_PATH"; then
        sudo cp -a "$ZAMAN_INPUT_PATH" "$ZAMAN_INPUT_BACKUP/"
    fi
done
for ZAMAN_INPUT_NAME in \
    56-zaman-8bitdo-ultimate-2c-bluetooth.yaml \
    65-generic_gamepad.yaml; do
    if sudo test -e "/etc/inputplumber/devices.d/$ZAMAN_INPUT_NAME"; then
        sudo mv "/etc/inputplumber/devices.d/$ZAMAN_INPUT_NAME" "$ZAMAN_INPUT_BACKUP/"
    fi
done
printf 'Input policy backup: %s\n' "$ZAMAN_INPUT_BACKUP"
sudo install -D -m 0755 dist/libexec/zaman-inputplumber-policy /usr/libexec/zaman-inputplumber-policy &&
sudo install -D -m 0644 dist/systemd/system/inputplumber.service.d/50-zaman-management.conf \
    /etc/systemd/system/inputplumber.service.d/50-zaman-management.conf &&
sudo systemctl daemon-reload &&
sudo systemctl restart inputplumber.service
```

Move the two known experimental YAML files only if they came from this session's
test proposals. A pre-existing independently maintained generic override needs
review before removal. The commands above assume those files were test artifacts.

For a full deployment, `install.sh` includes helper/drop-in installation and
backup/rollback, reloads systemd and restarts InputPlumber while sessiond/menu are
stopped. It does not remove existing device configurations. For OS images, copy
the tracked files to their declared paths; do not execute live service commands
during image assembly. No extra service enablement is required by the drop-in.

## Owner acceptance

Basic Bluetooth menu input with the native setting is user-confirmed. The helper
and drop-in have not been executed, installed or device-tested by the assistant.

```sh
sudo busctl --system get-property org.shadowblip.InputPlumber \
    /org/shadowblip/InputPlumber/Manager org.shadowblip.InputManager ManageAllDevices
busctl --system tree org.shadowblip.InputPlumber
sudo journalctl -u inputplumber.service --since '2 minutes ago' --no-pager
```

1. After installation, verify `b true`, successful policy journal output, and a
   composite plus D-Bus target for the Bluetooth controller using stock profiles.
2. Restart InputPlumber from the library and repeat. Do not manually enable the
   property; this proves service-restart persistence and sessiond rediscovery.
3. Reboot, reconnect the controller and verify the property/input path again.
   Controller automatic reconnection is separate from startup policy persistence.
4. Validate Guide, D-pad/stick navigation, A/B, gameplay, pause on menu focus,
   interception and input restoration. Toast gameplay testing remains separate.
5. Power off/on and reconnect without restarting sessiond. Switch to the receiver
   and confirm its specific profile and blocked auxiliary sources still work.
6. Test another controller and two simultaneous controllers; verify independent
   composites/input, correct mappings and no virtual-output recapture.
7. Inspect the journal for QML, policy and input-listener errors.

Bluetooth Connected remains a link status. A correlated Controller ready status
is a follow-up UI feature and is not claimed by this startup integration.

## Policy-only rollback

From the library, with the printed backup directory assigned to
`ZAMAN_INPUT_BACKUP` (assign it again after reconnecting SSH):

```sh
for ZAMAN_INPUT_PATH in \
    /usr/libexec/zaman-inputplumber-policy \
    /etc/systemd/system/inputplumber.service.d/50-zaman-management.conf; do
    ZAMAN_INPUT_NAME=${ZAMAN_INPUT_PATH##*/}
    if sudo test -e "$ZAMAN_INPUT_BACKUP/$ZAMAN_INPUT_NAME" || \
       sudo test -L "$ZAMAN_INPUT_BACKUP/$ZAMAN_INPUT_NAME"; then
        sudo cp -a "$ZAMAN_INPUT_BACKUP/$ZAMAN_INPUT_NAME" "$ZAMAN_INPUT_PATH"
    else
        sudo rm -f "$ZAMAN_INPUT_PATH"
    fi
done
for ZAMAN_INPUT_NAME in \
    56-zaman-8bitdo-ultimate-2c-bluetooth.yaml \
    65-generic_gamepad.yaml; do
    if sudo test -e "$ZAMAN_INPUT_BACKUP/$ZAMAN_INPUT_NAME"; then
        sudo cp -a "$ZAMAN_INPUT_BACKUP/$ZAMAN_INPUT_NAME" /etc/inputplumber/devices.d/
    fi
done
sudo systemctl daemon-reload
sudo systemctl restart inputplumber.service
ZAMAN_INPUT_MANAGEMENT=$(cat "$ZAMAN_INPUT_BACKUP/input-management")
case "$ZAMAN_INPUT_MANAGEMENT" in
    'b true') ZAMAN_INPUT_MANAGEMENT=true ;;
    'b false') ZAMAN_INPUT_MANAGEMENT=false ;;
    *) printf 'Unexpected saved management value; inspect backup.\n' >&2 ;;
esac
if [ "$ZAMAN_INPUT_MANAGEMENT" = true ] || [ "$ZAMAN_INPUT_MANAGEMENT" = false ]; then
    sudo /bin/sh dist/libexec/zaman-inputplumber-policy "$ZAMAN_INPUT_MANAGEMENT"
fi
```

Rollback restores the saved runtime setting. Without a startup integration,
stock 0.78.0 returns to false at the next restart; custom auto_manage rules remain
independently active.
