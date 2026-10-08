#!/usr/bin/env bash
# Offline build check: fetch locked dependencies into a temp CARGO_HOME (needs network, once),
# then build the release crates with --offline --locked and, where possible, with the network
# namespace denied. This is an offline build from a warm cargo cache, not a vendored build.
#   scripts/test-offline-build.sh            (slow; not run in CI)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
CRATES=(aien-cli spark-cockpit-rs spark-inquisitor cortex-encoder-rs cortex-rs spark-supervisor spark-debugger)
WORK="$(mktemp -d "${TMPDIR:-/tmp}/aien-offline-build.XXXXXX")"
[[ -n "${KEEP:-}" ]] || trap 'rm -rf "$WORK"' EXIT
export CARGO_HOME="$WORK/cargo-home" CARGO_TARGET_DIR="$WORK/target"
# keep the toolchain from the real rustup home
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
start=$SECONDS
echo "[*] cargo fetch --locked (network allowed)"
cargo fetch --locked
echo "[+] fetched in $((SECONDS - start))s"
args=(); for c in "${CRATES[@]}"; do args+=(-p "$c"); done
build=(cargo build --release --offline --locked "${args[@]}")
if unshare -rn true 2>/dev/null; then
    echo "[*] building with the network namespace denied (unshare -rn)"
    NETMODE="network denied (unshare -rn)"
    unshare -rn "${build[@]}"
else
    echo "WARNING: unshare -rn unavailable; network denial NOT enforced, only --offline was used" >&2
    NETMODE="only --offline (network denial not enforced)"
    "${build[@]}"
fi
echo "PASS: offline build from a warm cargo cache (cargo fetch --locked, then --offline --locked), $NETMODE, crates: ${CRATES[*]}, $((SECONDS - start))s total"
