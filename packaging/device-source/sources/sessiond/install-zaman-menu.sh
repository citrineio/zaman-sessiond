#!/usr/bin/env bash
set -euo pipefail

# Run from the zaman-sessiond-0.6.7 source directory after replacing src/bin/zaman-menu.rs.
test -f Cargo.toml && test -f src/bin/zaman-menu.rs
cargo test --locked --bin zaman-menu
cargo build --locked --release --bin zaman-menu

sudo -v
backup="/usr/libexec/zaman-menu.backup-$(date +%Y%m%d-%H%M%S)"
sudo cp -a /usr/libexec/zaman-menu "$backup"
systemctl --user stop zaman-menu.service
sudo install -m 0755 target/release/zaman-menu /usr/libexec/zaman-menu
systemctl --user start zaman-menu.service
systemctl --user is-active --quiet zaman-menu.service
printf 'Menu installed. Previous binary: %s\n' "$backup"
