#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
UPSTREAM_URL=https://github.com/nesdev-org/MesenCE.git
UPSTREAM_TAG=2.2.1
UPSTREAM_COMMIT=20ba206cef5ba207c21203176d02cb9f43dda9fb
PATCH_FILE="$REPO_DIR/patches/mesence/2.2.1/0001-preserve-process-for-graceful-sigint.patch"
SOURCE_DIR=${MESENCE_SOURCE_DIR:-"$REPO_DIR/build/mesence-2.2.1"}

for tool in git make dotnet clang clang++ sdl2-config; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing required build tool: $tool" >&2
        exit 1
    fi
done

if [ -e "$SOURCE_DIR" ]; then
    echo "Refusing to replace existing source directory: $SOURCE_DIR" >&2
    echo "Set MESENCE_SOURCE_DIR to a new, empty path and rerun." >&2
    exit 1
fi

mkdir -p "$(dirname -- "$SOURCE_DIR")"
git clone --depth 1 --branch "$UPSTREAM_TAG" "$UPSTREAM_URL" "$SOURCE_DIR"

ACTUAL_COMMIT=$(git -C "$SOURCE_DIR" rev-parse HEAD)
if [ "$ACTUAL_COMMIT" != "$UPSTREAM_COMMIT" ]; then
    echo "Unexpected MesenCE commit: $ACTUAL_COMMIT" >&2
    echo "Expected tag $UPSTREAM_TAG at: $UPSTREAM_COMMIT" >&2
    exit 1
fi

git -C "$SOURCE_DIR" apply --unidiff-zero --check "$PATCH_FILE"
git -C "$SOURCE_DIR" apply --unidiff-zero "$PATCH_FILE"

make -C "$SOURCE_DIR" USE_AOT=true

MESEN_BINARY=$(find "$SOURCE_DIR/bin" -type f -name Mesen -perm -u+x -print | head -n 1)
if [ -z "$MESEN_BINARY" ]; then
    echo "Build completed but no executable named Mesen was found." >&2
    exit 1
fi

echo "Patched MesenCE build completed:"
echo "$MESEN_BINARY"
sha256sum "$MESEN_BINARY"
