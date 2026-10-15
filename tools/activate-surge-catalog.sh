#!/usr/bin/env bash
set -Eeuo pipefail
exec bash "$(dirname -- "${BASH_SOURCE[0]}")/activate-shared-catalog.sh" "$@"
