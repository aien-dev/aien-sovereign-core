#!/usr/bin/env bash
# Builds libaien_qwen3_moe.so, the GB10 routed expert layer, with a C ABI for Rust.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

export MODULAR_CACHE_DIR="${MODULAR_CACHE_DIR:-/tmp/modular_cache}"
mkdir -p "${MODULAR_CACHE_DIR}"

mojo build --emit shared-lib -I . lib.mojo -o libaien_qwen3_moe.so
nm -D libaien_qwen3_moe.so | grep -E ' T aien_qwen3_'
echo "Built ${SCRIPT_DIR}/libaien_qwen3_moe.so"
