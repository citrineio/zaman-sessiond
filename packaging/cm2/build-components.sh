#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl git build-essential \
  pkg-config libsdl2-dev libsdl2-ttf-dev dpkg-dev python3 python3-gi binutils \
  libglib2.0-bin dbus
if ! command -v cargo >/dev/null; then
  curl --fail --silent --show-error https://sh.rustup.rs -o /tmp/zaman-rustup.sh
  sh /tmp/zaman-rustup.sh -y --profile minimal --default-toolchain stable
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
python3 packaging/cm2/fetch-sources.py
python3 packaging/cm2/test-package-tools.py
cd packaging/work/core
cargo test --locked --lib
cargo build --release --locked --bin zaman-sessiond --bin zamanctl \
  --bin zaman-menu-dev-input --bin zaman-menu-dev-pair
cd ../../..
python3 packaging/cm2/build-debs.py --version "${ZAMAN_PACKAGE_VERSION:?set version}"
