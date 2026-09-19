#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

export MODULAR_CACHE_DIR="${MODULAR_CACHE_DIR:-/tmp/modular_cache}"
mkdir -p "${MODULAR_CACHE_DIR}"

echo "Compiling Mojo GB10 kernels to shared library..."
mojo build --emit shared-lib lib.mojo -o libaien_kernels.so

echo "Verifying exported symbols:"
nm -D libaien_kernels.so | grep aien_

echo "Kernel build complete: ${SCRIPT_DIR}/libaien_kernels.so"
