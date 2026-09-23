#!/usr/bin/env bash
# Builds libqwencoder_serve.so: resident-weight Qwen3-Coder serve library.
# Reuses the proven MoE pipeline read-only via -I (nothing there is modified).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MOE_DIR="${SCRIPT_DIR}/../qwen3_moe"
cd "${SCRIPT_DIR}"

export MODULAR_CACHE_DIR="${MODULAR_CACHE_DIR:-/tmp/modular_cache}"
mkdir -p "${MODULAR_CACHE_DIR}"

mojo build --emit shared-lib -I . -I "${MOE_DIR}" qwencoder_serve.mojo -o libqwencoder_serve.so
nm -D libqwencoder_serve.so | grep -E ' T qwencoder_'
echo "Built ${SCRIPT_DIR}/libqwencoder_serve.so"
