# Zaman-sessiond standalone Debian 13 ARM64 package

Builds the **committed** session daemon, `zamanctl`, controller helpers, default registry/catalog, D-Bus activation, and user systemd service into an ARM64 Debian package for the Q6A's Debian 13/Armbian environment. Unlike `packaging/cm2/`, this workflow does not build Qt/Quickshell or bundle the entire console stack. It is not a Buildroot production package.

The package is generated automatically when the files listed in `.github/workflows/sessiond-deb.yml` change on `main`, and can be run manually through workflow dispatch. After a successful build, download the package from [GitHub Releases](https://github.com/citrineio/zaman-sessiond/releases) (`sessiond-trixie-*`) or the workflow artifact. Check SHA256SUMS.

## Native local build

On Debian 13 ARM64 with Rust >= 1.85, Cargo, dpkg-dev, build-essential, pkg-config, libsdl2-dev and libsdl2-ttf-dev:

```sh
cargo test --locked --lib
cargo build --release --locked --bin zaman-sessiond --bin zamanctl --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
python3 packaging/sessiond/package.py
(cd packaging/out/sessiond && sha256sum -c SHA256SUMS)
```

## Deployment

**Warning:** the Q6A may have changes that have not been committed to GitHub. This package is made from the GitHub repository only. Compare it with the running source before upgrading.

The package does **not** start, restart, stop or enable user services on installation. Back up the current manually installed binary and inspect user-service overrides first:

```sh
systemctl --user cat zaman-sessiond.service
mkdir -p "$HOME/zaman-backup"
sudo cp -a /usr/libexec/zaman-sessiond "$HOME/zaman-backup/zaman-sessiond.pre-deb"
```

Install the downloaded `.deb` when no game is running:

```sh
sudo apt install ./zaman-sessiond_VERSION_arm64.deb
systemctl --user stop zaman-sessiond.service
systemctl --user daemon-reload
systemctl --user start zaman-sessiond.service
systemctl --user status zaman-sessiond.service --no-pager
zamanctl version
zamanctl status
```

The package retains `/usr/libexec/zaman-sessiond` for compatibility and provides a second alias `/usr/libexec/zaman/zaman-sessiond` for newer stack integrations. Existing higher-precedence unit overrides may still run binaries elsewhere; inspect `systemctl --user cat`. Never run duplicate daemons with the same D-Bus name. Roll back by restoring the previous package or saved binary and restarting the user service.

This package does not modify GPU drivers, kernel, boot configuration, ROMs, BIOS files, saves, InputPlumber configuration or the frontend. It provides the library-sync and native-game helper environment paths in its service unit.
