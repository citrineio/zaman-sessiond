#!/bin/bash
# Native ARM64, Debian 12. Qt is private; system EGL/GLES/GBM remain vendor-owned.
set -euo pipefail
cd "$(dirname "$0")/../.."
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates git build-essential \
  cmake ninja-build pkg-config python3 meson bison flex patchelf dpkg-dev \
  libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev libgbm-dev libdrm-dev \
  libwayland-dev libxkbcommon-dev libinput-dev libdbus-1-dev libfontconfig1-dev \
  libfreetype6-dev libharfbuzz-dev libpng-dev libjpeg-dev libzstd-dev zlib1g-dev \
  libglib2.0-dev libpipewire-0.3-dev libjemalloc-dev libcli11-dev libxcb1-dev \
  libssl-dev libpcre2-dev libvulkan-dev spirv-tools dbus
QT_VERSION=$(python3 -c 'import json;print(json.load(open("packaging/cm2/sources.json"))["qt"])')
QS_VERSION=$(python3 -c 'import json;print(json.load(open("packaging/cm2/sources.json"))["quickshell"])')
PROTOCOLS_VERSION=$(python3 -c 'import json;print(json.load(open("packaging/cm2/sources.json"))["wayland_protocols"])')
prefix=/opt/zaman/runtime
src="$PWD/packaging/runtime-src"
mkdir -p "$src"
export CMAKE_PREFIX_PATH="$prefix"
export PATH="$prefix/bin:$PATH"
export PKG_CONFIG_PATH="/opt/zaman/build-deps/share/pkgconfig:$prefix/lib/pkgconfig"
export CMAKE_BUILD_PARALLEL_LEVEL=3

# Quickshell 0.3 needs newer protocol XML than Bookworm provides. Protocol XML
# has no runtime ABI; install it into a private build-only prefix.
if [ ! -f /opt/zaman/build-deps/share/pkgconfig/wayland-protocols.pc ]; then
  git clone --depth 1 --branch "$PROTOCOLS_VERSION" \
    https://gitlab.freedesktop.org/wayland/wayland-protocols.git "$src/wayland-protocols"
  meson setup "$src/protocol-build" "$src/wayland-protocols" --prefix=/opt/zaman/build-deps -Dtests=false
  ninja -C "$src/protocol-build" install
fi

for module in qtbase qtshadertools qtdeclarative qtwayland qtsvg; do
  if [ -f "$prefix/.zaman-$module-$QT_VERSION" ]; then continue; fi
  git clone --depth 1 --branch "v$QT_VERSION" "https://github.com/qt/$module.git" "$src/$module"
  opts=(-GNinja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix"
    -DCMAKE_INSTALL_LIBDIR=lib -DBUILD_SHARED_LIBS=ON
    -DQT_BUILD_TESTS_BY_DEFAULT=OFF -DQT_BUILD_EXAMPLES_BY_DEFAULT=OFF
    -DCMAKE_INSTALL_RPATH="$prefix/lib")
  if [ "$module" = qtbase ]; then
    opts+=(-DQT_FEATURE_opengles2=ON -DQT_FEATURE_opengl_desktop=OFF
      -DQT_FEATURE_xcb=OFF -DQT_FEATURE_vulkan=OFF -DQT_FEATURE_sql=OFF
      -DQT_FEATURE_printsupport=OFF)
    cmake -S "$src/$module" -B "$src/build-$module" "${opts[@]}"
  else
    "$prefix/bin/qt-cmake" -S "$src/$module" -B "$src/build-$module" "${opts[@]}"
  fi
  cmake --build "$src/build-$module"
  cmake --install "$src/build-$module"
  mkdir -p "$prefix/share/licenses/$module"
  cp -a "$src/$module/LICENSES/." "$prefix/share/licenses/$module/"
  touch "$prefix/.zaman-$module-$QT_VERSION"
  rm -rf "$src/build-$module"
done

git clone --depth 1 --branch "$QS_VERSION" https://github.com/quickshell-mirror/quickshell.git "$src/quickshell"
cmake -S "$src/quickshell" -B "$src/build-qs" -GNinja \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PREFIX_PATH="$prefix" \
  -DCMAKE_INSTALL_RPATH="$prefix/lib" -DDISTRIBUTOR='Kawn Electro Debian 12 ARM64' \
  -DCRASH_HANDLER=OFF -DSERVICE_PAM=OFF -DSERVICE_POLKIT=OFF \
  -DSERVICE_GREETD=OFF -DHYPRLAND=OFF -DSCREENCOPY=OFF
cmake --build "$src/build-qs"
cmake --install "$src/build-qs"
mkdir -p "$prefix/share/licenses/quickshell"
find "$src/quickshell" -maxdepth 1 -iname '*license*' -type f -exec cp {} "$prefix/share/licenses/quickshell/" \;
"$prefix/bin/quickshell" --version
testroot=$(mktemp -d /tmp/zaman-runtime-test.XXXXXXXX)
chmod 700 "$testroot"
cat > "$testroot/shell.qml" <<'EOF'
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Bluetooth
import Quickshell.Networking
import Quickshell.Services.Pipewire
ShellRoot { Component.onCompleted: Qt.quit() }
EOF
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software XDG_RUNTIME_DIR="$testroot" \
  timeout 30s dbus-run-session -- "$prefix/bin/quickshell" --no-duplicate -p "$testroot/shell.qml"
rm -rf "$testroot"
python3 packaging/cm2/runtime-deb.py --version "${ZAMAN_PACKAGE_VERSION:?set version}"
