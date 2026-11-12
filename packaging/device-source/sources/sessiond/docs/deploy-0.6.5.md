# Deployment record — 0.6.5 accepted

The user has installed and hardware-accepted 0.6.5. **Do not rerun the original
apply/build/install sequence merely to close the release.** Use
`zaman-0.6.5-closeout.sh` to write the accepted documents and commit/push.

Verified installed version: 0.6.5. Backup:
`/home/kadhem/zaman-pre-0.6.5.N9pDxX`.
Library/game reboot, frontend return, saves, menu layout/navigation, and shutdown
in both contexts were confirmed working without quirks. See
`session-0.6.5.md` for evidence and the test-harness corrections.

The commands below are the historical deployment and recovery reference.
Their earlier pending-state wording is superseded by the acceptance record.

---

# Deploy and verify 0.6.5

Use the existing board checkout at `~/zaman-lab/zaman-sessiond-0.6.3`.
These commands are for the user to run on the board. The assistant has not
built or installed the patch. Reboot is the only feature in this version.

## 1. Apply and build

Save `zaman-0.6.5-reboot.patch` to your home directory, then run:

```bash
bash <<'BASH'
set -euo pipefail
cd "$HOME/zaman-lab/zaman-sessiond-0.6.3"
test "$(git rev-parse HEAD)" = 0d0d156691dd4c000069c3e64842a1fa38161a7a
git diff --quiet
git diff --cached --quiet
git apply --check "$HOME/zaman-0.6.5-reboot.patch"
git apply "$HOME/zaman-0.6.5-reboot.patch"
cargo fmt
cargo fmt --check
git diff --check
cargo build --locked
cargo test --locked
cargo build --release --locked --bins
BASH
```

If a command fails, stop and share its output. Do not reapply an already-applied
patch. The two untracked navigation patches are unrelated and can remain.

## 2. Preview the actual renderer

Run after the build. This does not touch the graphical session or power state.

```bash
bash <<'BASH'
set -euo pipefail
cd "$HOME/zaman-lab/zaman-sessiond-0.6.3"
preview_dir="$HOME/zaman-0.6.5-previews"
mkdir -p "$preview_dir"
for size in 1280x800 1920x1080 2560x1440; do
    width=${size%x*}
    height=${size#*x}
    for mode in library reboot shutdown pending error; do
        state=paused
        if [ "$mode" = library ]; then state=idle; fi
        target/release/zaman-menu --preview "$mode" "$width" "$height" \
            "$preview_dir/$mode-$size.bmp" "$state"
    done
done
BASH
```

Inspect the images: labels/details fit, all four game rows and footer are
visible, pending/error text is confined to the right panel. Share the images
if any spacing is wrong. Geometry tests do not replace this render check.

## 3. Back up, smoke-test, and install

Exit the current game through the menu first. Run in the normal user account,
not a root shell. The block refuses to proceed while a game unit is active.
The existing smoke script repeats some build checks and needs InputPlumber
running; it tests synthetic sessions and never reboots the system.

```bash
bash <<'BASH'
set -euo pipefail
cd "$HOME/zaman-lab/zaman-sessiond-0.6.3"
if systemctl --user is-active --quiet zaman-game.service; then
    echo 'Exit the active game first.' >&2
    exit 1
fi
backup_dir="$HOME/zaman-backups/pre-0.6.5-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backup_dir"
cp -p /usr/libexec/zaman-sessiond "$backup_dir/zaman-sessiond"
cp -p /usr/libexec/zaman-menu "$backup_dir/zaman-menu"
cp -p /usr/local/bin/zamanctl "$backup_dir/zamanctl"
printf '%s\n' "$backup_dir" > "$HOME/zaman-backups/pre-0.6.5-latest"
printf 'Backup: %s\n' "$backup_dir"
sudo -v
systemctl --user stop zaman-menu.service zaman-sessiond.service
recover() {
    rc=$?
    trap - EXIT
    if [ "$rc" -ne 0 ]; then
        sudo install -m 0755 "$backup_dir/zaman-sessiond" /usr/libexec/zaman-sessiond
        sudo install -m 0755 "$backup_dir/zaman-menu" /usr/libexec/zaman-menu
        sudo install -m 0755 "$backup_dir/zamanctl" /usr/local/bin/zamanctl
    fi
    systemctl --user start zaman-sessiond.service zaman-menu.service || true
    exit "$rc"
}
trap recover EXIT
bash tools/smoke-test.sh
sudo install -m 0755 target/release/zaman-sessiond /usr/libexec/zaman-sessiond
sudo install -m 0755 target/release/zaman-menu /usr/libexec/zaman-menu
sudo install -m 0755 target/release/zamanctl /usr/local/bin/zamanctl
systemctl --user start zaman-sessiond.service zaman-menu.service
systemctl --user is-active --quiet zaman-sessiond.service
systemctl --user is-active --quiet zaman-menu.service
test "$(/usr/local/bin/zamanctl version)" = 0.6.5
/usr/local/bin/zamanctl status
busctl --user introspect com.kawnelectro.Zaman.Session1 \
    /com/kawnelectro/Zaman/Session1 com.kawnelectro.Zaman.Session1
trap - EXIT
BASH
```

No unit file changes are needed. The XML describes the interface; zbus derives
the runtime interface from Rust. Preserve the current user service environment.

## 4. Permission and hardware acceptance

Check permission without rebooting, as the same user:

```bash
busctl --system call org.freedesktop.login1 /org/freedesktop/login1 \
  org.freedesktop.login1.Manager CanReboot
```

`yes` indicates permission for that caller. `challenge` requires authorization;
`no` or `na` needs investigation. A shell/SSH caller can differ from the daemon's
session context, so the real menu test remains authoritative. Share a refusal
and inspect installed policy rather than adding a broad power permission rule.
The signature and permission query are documented in the upstream
[login1 interface](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.login1.xml).

Perform these deliberately, one at a time; each reboot ends your SSH connection:

1. Open the library menu, choose Reboot, confirm frontend returns after boot.
2. Launch a game, change a known save, open the game menu and choose Reboot.
   Relaunch after boot and verify the save survived.
3. From the library, run `zamanctl reboot` and confirm the CLI path works.
4. Confirm Shut Down still works from both library and active game.
5. Confirm ordinary resume/exit and D-pad responsiveness remain correct.

If it stays running, collect:

```bash
zamanctl status
journalctl --user -u zaman-sessiond.service -u zaman-menu.service -n 100 --no-pager
```

Reboot requests are asynchronous. A successful CLI return acknowledges queueing;
status/journal report later failures. Unit tests do not issue real power actions.

## 5. Rollback if needed

Exit any active game first. Restore the recorded backup:

```bash
bash <<'BASH'
set -euo pipefail
if systemctl --user is-active --quiet zaman-game.service; then
    echo 'Exit the active game first.' >&2
    exit 1
fi
backup_dir=$(cat "$HOME/zaman-backups/pre-0.6.5-latest")
test -x "$backup_dir/zaman-sessiond"
test -x "$backup_dir/zaman-menu"
test -x "$backup_dir/zamanctl"
sudo -v
systemctl --user stop zaman-menu.service zaman-sessiond.service
sudo install -m 0755 "$backup_dir/zaman-sessiond" /usr/libexec/zaman-sessiond
sudo install -m 0755 "$backup_dir/zaman-menu" /usr/libexec/zaman-menu
sudo install -m 0755 "$backup_dir/zamanctl" /usr/local/bin/zamanctl
systemctl --user start zaman-sessiond.service zaman-menu.service
zamanctl version
zamanctl status
BASH
```

This restores binaries, leaving source changes available for diagnosis.

## 6. Closeout only after acceptance

Record actual results in `docs/session-0.6.5.md` and update the status in
`docs/session-conventions.md` and `CHANGELOG.md`. Then:

```bash
cargo fmt --check
git diff --check
git add Cargo.toml Cargo.lock src/contract.rs src/api.rs src/daemon.rs \
  src/session.rs src/operations.rs src/menu.rs src/bin/zaman-menu.rs \
  src/bin/zamanctl.rs interfaces/com.kawnelectro.Zaman.Session1.xml \
  tools/smoke-test.sh docs/session-0.6.5.md docs/deploy-0.6.5.md \
  docs/session-conventions.md CHANGELOG.md
git diff --cached --stat
git commit -m "session: add graceful reboot from menu and CLI (0.6.5)"
git push origin main
git rev-parse HEAD
git ls-remote --heads origin main
```

The downloaded patches remain untracked. No commit or push has been performed
by the assistant. Final commit and installed acceptance belong to this session.

## If the smoke test closes the terminal

Run the complete block below in a fresh terminal/SSH session. Do not source the
script or paste just the body of the earlier heredoc. This catches a nonzero
child exit, keeps its output, and restores the installed services on exit.
It does not install binaries. Exit the active game first.

For users who already applied the original 0.6.5 patch, first apply
`zaman-0.6.5-smoke-version-fix.patch` from the checkout. The corrected full
0.6.5 patch and source archive already include that correction.

```bash
if bash >"$HOME/zaman-0.6.5-smoke.log" 2>&1 <<'BASH'
    set -eu
    cd "$HOME/zaman-lab/zaman-sessiond-0.6.3"
    if systemctl --user is-active --quiet zaman-game.service; then
        echo 'Exit the active game first.' >&2
        exit 1
    fi
    trap 'systemctl --user start zaman-sessiond.service zaman-menu.service || true' EXIT
    systemctl --user stop zaman-menu.service zaman-sessiond.service
    bash tools/smoke-test.sh
BASH
then
    printf 'Smoke test passed.\n'
else
    printf 'Smoke test failed (exit %s).\n' "$?"
fi
tail -n 100 "$HOME/zaman-0.6.5-smoke.log"
```

A saved log is still available after an actual SSH disconnect. Share its last
100 lines. Passing this standalone rerun does not install 0.6.5; continue the
installation section only after checks pass.

## MenuContextChanged missing at stage 4/8

The initial Rust interface emitted MenuContextChanged but did not declare it
for introspection. Apply `zaman-0.6.5-context-signal-fix.patch` to the existing
0.6.5 checkout, then run cargo fmt and the isolated smoke wrapper above.
The current full source archive and full reboot patch include the correction.
Do not remove MenuContextChanged from the smoke assertions.

After the smoke test passes, run `cargo build --release --locked --bins`
before installation. Otherwise the install block would use release binaries
built before this correction. No binaries have been installed by the assistant.

## After the reported SMOKE_PASS

The user has now reported the release build and all eight smoke stages passing,
with broken-pipe panics from grep -q consuming CLI output and no connected
controller composites. The smoke-output correction affects the harness only;
it does not require another release build. A local shell fixture checked full
output consumption and failure handling. Controller acceptance remains pending.

Save `zaman-0.6.5-smoke-output-fix.patch` and `zaman-0.6.5-install.sh` in the
board user's home directory and run `bash "$HOME/zaman-0.6.5-install.sh"`.
The installer applies (or recognizes) this harness correction, checks for an
active game, backs up the installed binaries, installs the existing release
binaries, and verifies version, signal introspection, and status. It restores
backups on an installation/check failure. It does not build, run smoke again,
reboot, commit, or push. Then perform section 4 hardware acceptance.
