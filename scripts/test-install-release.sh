#!/usr/bin/env bash
# Exercise install.sh release mode offline: a fixture release is signed with a
# throwaway key and served over file://. Checks that a valid release installs
# and that a tampered archive or a signature from another key installs nothing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/aien-test-install-release.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

case "$(uname -s)" in Linux) OS=linux ;; Darwin) OS=macos ;; *) fail "unsupported OS" ;; esac
case "$(uname -m)" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=aarch64 ;; *) fail "unsupported arch" ;; esac
[[ "$OS-$ARCH" != macos-aarch64 ]] || ARCH=arm64
ASSET="sovereign-$OS-$ARCH.tar.gz"

ssh-keygen -q -t ed25519 -N '' -C test -f "$WORK/key"
ssh-keygen -q -t ed25519 -N '' -C other -f "$WORK/other"
printf 'aien-release %s\n' "$(cut -d' ' -f1,2 "$WORK/key.pub")" > "$WORK/allowed_signers"

make_release() { # dir signing-key
    local d="$1" k="$2"
    mkdir -p "$d" "$WORK/pkg/bin"
    printf '#!/bin/sh\nexit 0\n' > "$WORK/pkg/bin/aien"
    printf '#!/bin/sh\nexit 0\n' > "$WORK/pkg/bin/cortex-rs"
    chmod 755 "$WORK/pkg/bin/"*
    tar -czf "$d/$ASSET" -C "$WORK/pkg" .
    (cd "$d" && sha256sum "$ASSET" 2>/dev/null > SHA256SUMS.txt || shasum -a 256 "$ASSET" > SHA256SUMS.txt)
    ssh-keygen -q -Y sign -f "$k" -n aien-release "$d/SHA256SUMS.txt"
}

run_install() { # release-dir bin-dir
    AIEN_SOURCE_DIR="$ROOT" AIEN_RELEASE_TAG=v0.0.0-test \
    AIEN_RELEASE_BASE_URL="file://$1" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers" \
    AIEN_BIN_DIR="$2" AIEN_CONFIG_DIR="$WORK/config" AIEN_INSTALL_NO_PROFILE=1 \
        bash "$ROOT/install.sh" > "$WORK/log" 2>&1
}

make_release "$WORK/good" "$WORK/key"
run_install "$WORK/good" "$WORK/bin-good" || { cat "$WORK/log" >&2; fail "valid signed release did not install"; }
[[ -x "$WORK/bin-good/aien" && -x "$WORK/bin-good/cortex" ]] || fail "binaries missing after valid install"

make_release "$WORK/tamper" "$WORK/key"
printf 'x' >> "$WORK/tamper/$ASSET"
if run_install "$WORK/tamper" "$WORK/bin-tamper"; then fail "tampered archive was installed"; fi
grep -q "checksum mismatch" "$WORK/log" || fail "tampered archive not rejected for checksum"
[[ ! -e "$WORK/bin-tamper/aien" ]] || fail "tampered install left a binary"

make_release "$WORK/forged" "$WORK/other"
if run_install "$WORK/forged" "$WORK/bin-forged"; then fail "release signed by another key was installed"; fi
grep -q "signature is NOT valid" "$WORK/log" || fail "forged signature not rejected"
[[ ! -e "$WORK/bin-forged/aien" ]] || fail "forged install left a binary"

echo "PASS: release install verifies signature and checksum, rejects tampered and forged releases"
