# Zaman CM2 Debian packages

This directory builds real `.deb` packages and publishes them as GitHub release assets.
Target: Debian 12 / Bookworm, ARM64. Native GitHub ARM64 runners build inside Debian 12.

## Install on the CM2

```sh
curl -fsSL https://raw.githubusercontent.com/citrineio/zaman-sessiond/main/packaging/cm2/install.py -o /tmp/install-zaman.py
python3 /tmp/install-zaman.py
```

The installer downloads the newest COMPLETE `cm2-bookworm-*` release, including prereleases,
verifies SHA256 checksums, checks the package names, and installs all components together.
Use `--tag cm2-bookworm-VERSION` to select an exact release, or `--download-only DIRECTORY`
to save packages without installing. A failed build never publishes a partial package set.

From a local text console on the board, run `zaman-start` without sudo. Installing does not
enable autologin, start the graphical services, change boot files, or replace vendor GPU drivers.
F12 opens/closes the resident menu. Super+Enter opens a terminal; Super+Shift+E exits the session.
Pegasus application, InputPlumber, emulator binaries, and game/BIOS files are separate.
Without Pegasus the session opens a bench terminal; without InputPlumber use the keyboard.

## Publish the current Q6A installation directly

Recommended: run on the NucBox, where your GitHub CLI is already authenticated,
and let it collect the Q6A source automatically over SSH:

```sh
curl -fsSL https://raw.githubusercontent.com/citrineio/zaman-sessiond/main/packaging/cm2/publish-device.py -o /tmp/publish-zaman.py
python3 /tmp/publish-zaman.py --source-host kadhem@zaman
```

It transfers its exporter to the Q6A over SSH, collects the archive automatically,
and uses the NucBox's existing GitHub credentials. No GitHub CLI/login is needed on
the Q6A and no archive is passed through chat. Normal SSH authentication is required.

On the source Q6A, as the existing Zaman user, with `git` and authenticated `gh` available:

```sh
curl -fsSL https://raw.githubusercontent.com/citrineio/zaman-sessiond/main/packaging/cm2/publish-device.py -o /tmp/publish-zaman.py
python3 /tmp/publish-zaman.py
```

It discovers the live local source directories and current deployed theme, exports them,
pushes them to a new `device-build/TIMESTAMP-ID` branch in this repository, and waits for
GitHub Actions to build and publish installable packages. No files are sent through ChatGPT.
No force-push, source checkout edits, service restarts, or installation occurs on the Q6A.
Git history, machine inventories, service environment dumps, credentials directories, ROMs,
BIOS and saves are not published. Custom source files are published as requested; source
code should not contain credentials. See the exporter for exact filename exclusions.

Discovery selects each component from its own systemd user service: MainPID first,
then that unit's effective ExecStart/WorkingDirectory. A menu helper running from
a different checkout is exported as a separate Rust project and built from that
project's Cargo.toml/Cargo.lock. The presence of two active Zaman source trees
does not force the daemon and helper to be selected from the same checkout.

Optional installed configuration, policy directories and binary diagnostics are
read without elevated privileges. Unreadable optional paths are recorded in the
snapshot's skipped list and do not stop publication. Read failures in component
source trees, helper projects or explicit `--extra` paths make the export
incomplete and block publication, including failures while listing a directory.

If discovery is ambiguous, pass the actual paths directly:

```sh
python3 /tmp/publish-zaman.py \
  --sessiond-dir "$HOME/projects/zaman-sprint4/zaman-sessiond" \
  --menu-dir "$HOME/zaman-quickshell-prototype" \
  --theme-dir "$HOME/.config/pegasus-frontend/themes/zaman"
```

`--extra /path/to/helper` adds a required installed helper tree to the source snapshot.
`--no-wait` returns after pushing. Build/source branches are retained for reproducibility.

The checked-in main build uses pinned published menu/theme commits from `sources.json`.
It is a published-source baseline, not a claim that unpublished Q6A changes are present.
Only running the device publisher makes those local changes available to GitHub.

## Packages and stable paths

| Package | Architecture | Content |
| --- | --- | --- |
| zaman-sessiond | arm64 | daemon, CLI, input/pairing helpers, registry, catalog, D-Bus definitions |
| zaman-menu | all | resident QML, notifications, shortcut and menu actions |
| zaman-helpers | all | shared Python/shell helpers and transfer receiver/client |
| zaman-pegasus-theme | all | theme source/assets; user activation copies to a writable theme tree |
| zaman-session | all | Sway session, units, setup and launch tools |
| zaman-stack | all | matched component metapackage |
| zaman-quickshell-runtime | arm64 | private Qt + Quickshell runtime under /opt/zaman/runtime |

Menu sources live in `/usr/share/zaman/menu`, executable helpers in `/usr/libexec/zaman`,
CLI/session tools in `/usr/bin`, default registry/catalog in `/usr/share/zaman`,
user units in `/usr/lib/systemd/user`, and session config in `/etc/zaman`.
Per-user mutable state uses XDG directories. The package-owned theme template is under
`/usr/share/pegasus-frontend/themes/zaman`; activation copies it into the user's theme directory
so existing refresh-token helpers can write there. A custom preexisting theme is preserved.
The package does not silently overwrite it; move it aside before using the packaged session.

`/etc/zaman/session.env` can override unit environment settings. Units are attached to
`zaman-session.target`, started after Sway imports its live Wayland/Sway environment and
stopped when the Zaman session exits. Installation never enables them at `default.target`.

Runtime system-library dependencies are inferred from the built ELF files. Rust binaries
are rebuilt for Bookworm, not copied from the Q6A's newer userspace. Qt and Quickshell are
versioned together; vendor libEGL/libGLES/libgbm are not bundled into the private runtime.
Qt/Quickshell license notices and exact upstream source tags accompany the runtime package.

Installed device helper code is preserved under `/usr/libexec/zaman/device` with its original
directory hierarchy as deployment evidence. Helpers with absolute custom paths may require
additional adaptation; their presence alone does not establish that every optional feature
is integrated. No Q6A kernel/display/firmware or machine policy files are applied to the CM2.

## Build locally in a Debian 12 ARM64 container

```sh
export ZAMAN_PACKAGE_VERSION=0.1.1
bash packaging/cm2/build-components.sh
bash packaging/cm2/build-runtime.sh
```

The scripts install build dependencies and therefore run as root INSIDE the builder,
not on the live console. Runtime and component jobs may run independently. Output goes
to `packaging/out`. Component source resolution honors a live-device snapshot on the branch.

The workflow publishes only when both jobs succeed. Each release has immutable versioned
filenames, `SHA256SUMS`, component provenance and installation instructions. Hardware tests
must still qualify Sway/GPU rendering, audio, controller hotplug, menu/Resume and emulator return.
