#!/bin/sh
# Build the verifier with rustc directly (no cargo, no crates). Usage: build.sh [output-path]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/aien-verify}
rustc --edition 2021 -O "$here/src/main.rs" -o "$out"
echo "built $out"
