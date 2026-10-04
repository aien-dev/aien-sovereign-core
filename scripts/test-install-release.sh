#!/usr/bin/env bash
# Exercise install.sh release mode offline. A fixture release is signed with a
# throwaway key and served over file://. Checks:
#   1. the key pinned inside install.sh equals docs/release/allowed_signers
#   2. a valid signed release installs from a checkout
#   3. a tampered archive installs nothing (checksum)
#   4. a release signed by another key installs nothing (signature)
#   5. a standalone copy of install.sh (no source tree, clean HOME) installs the
#      valid release offline when the signer file is given, imprint included
#   6. the same standalone copy with no signer override falls back to the key
#      pinned in install.sh, so the fixture (throwaway key) is refused
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/aien-test-install-release.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

case "$(uname -s)" in Linux) OS=linux ;; Darwin) OS=macos ;; *) fail "unsupported OS" ;; esac
case "$(uname -m)" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=aarch64 ;; *) fail "unsupported arch" ;; esac
[[ "$OS-$ARCH" != macos-aarch64 ]] || ARCH=arm64
ASSET="sovereign-$OS-$ARCH.tar.gz"

# 1. pinned key consistency
PINNED="$(sed -n "s/^AIEN_PINNED_SIGNER='\(.*\)'\$/\1/p" "$ROOT/install.sh")"
[[ -n "$PINNED" ]] || fail "install.sh has no AIEN_PINNED_SIGNER line"
grep -qxF "$PINNED" "$ROOT/docs/release/allowed_signers" \
    || fail "AIEN_PINNED_SIGNER in install.sh is not a line of docs/release/allowed_signers"

ssh-keygen -q -t ed25519 -N '' -C test -f "$WORK/key"
ssh-keygen -q -t ed25519 -N '' -C other -f "$WORK/other"
printf 'aien-release %s\n' "$(cut -d' ' -f1,2 "$WORK/key.pub")" > "$WORK/allowed_signers"

make_release() { # dir signing-key
    local d="$1" k="$2"
    rm -rf "$WORK/pkg"
    mkdir -p "$d" "$WORK/pkg/bin" "$WORK/pkg/imprints/en2-trinity"
    printf '#!/bin/sh\nexit 0\n' > "$WORK/pkg/bin/aien"
    printf '#!/bin/sh\nexit 0\n' > "$WORK/pkg/bin/cortex-rs"
    printf 'fixture imprint\n' > "$WORK/pkg/imprints/en2-trinity/soul.md"
    chmod 755 "$WORK/pkg/bin/"*
    tar -czf "$d/$ASSET" -C "$WORK/pkg" .
    (cd "$d" && sha256sum "$ASSET" 2>/dev/null > SHA256SUMS.txt || shasum -a 256 "$ASSET" > SHA256SUMS.txt)
    ssh-keygen -q -Y sign -f "$k" -n aien-release "$d/SHA256SUMS.txt"
}

run_install() { # release-dir bin-dir config-dir installer [VAR=value ...]
    local rel="$1" bin="$2" cfg="$3" installer="$4"
    shift 4
    env AIEN_RELEASE_TAG=v0.0.0-test AIEN_RELEASE_BASE_URL="file://$rel" \
        AIEN_BIN_DIR="$bin" AIEN_CONFIG_DIR="$cfg" AIEN_INSTALL_NO_PROFILE=1 "$@" \
        bash "$installer" > "$WORK/log" 2>&1
}

# 2. valid release, from the checkout
make_release "$WORK/good" "$WORK/key"
run_install "$WORK/good" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" \
    AIEN_SOURCE_DIR="$ROOT" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers" \
    || { cat "$WORK/log" >&2; fail "valid signed release did not install"; }
[[ -x "$WORK/bin-good/aien" && -x "$WORK/bin-good/cortex" ]] || fail "binaries missing after valid install"

# 3. tampered archive
make_release "$WORK/tamper" "$WORK/key"
printf 'x' >> "$WORK/tamper/$ASSET"
if run_install "$WORK/tamper" "$WORK/bin-tamper" "$WORK/cfg-tamper" "$ROOT/install.sh" \
    AIEN_SOURCE_DIR="$ROOT" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers"; then
    fail "tampered archive was installed"
fi
grep -q "checksum mismatch" "$WORK/log" || fail "tampered archive not rejected for checksum"
[[ ! -e "$WORK/bin-tamper/aien" ]] || fail "tampered install left a binary"

# 4. signature from another key
make_release "$WORK/forged" "$WORK/other"
if run_install "$WORK/forged" "$WORK/bin-forged" "$WORK/cfg-forged" "$ROOT/install.sh" \
    AIEN_SOURCE_DIR="$ROOT" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers"; then
    fail "release signed by another key was installed"
fi
grep -q "signature is NOT valid" "$WORK/log" || fail "forged signature not rejected"
[[ ! -e "$WORK/bin-forged/aien" ]] || fail "forged install left a binary"

# 5. standalone installer, no source tree, clean HOME, offline, signer file given
mkdir -p "$WORK/standalone" "$WORK/home"
cp "$ROOT/install.sh" "$WORK/standalone/install.sh"
run_install "$WORK/good" "$WORK/bin-alone" "$WORK/cfg-alone" "$WORK/standalone/install.sh" \
    HOME="$WORK/home" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers" \
    || { cat "$WORK/log" >&2; fail "standalone offline install did not install"; }
grep -q "Release mode without a source tree" "$WORK/log" || fail "standalone install used a source tree"
[[ -x "$WORK/bin-alone/aien" && -x "$WORK/bin-alone/cortex" ]] || fail "binaries missing after standalone install"
[[ -f "$WORK/cfg-alone/imprints/en2/soul.md" ]] || fail "imprint from the release archive was not installed"
[[ -z "$(ls -A "$WORK/home")" ]] || fail "standalone install wrote into HOME: $(ls -A "$WORK/home")"

# 6. standalone installer, no signer override: the pinned key refuses the fixture
if run_install "$WORK/good" "$WORK/bin-pinned" "$WORK/cfg-pinned" "$WORK/standalone/install.sh" \
    HOME="$WORK/home"; then
    fail "fixture signed by a throwaway key was accepted by the pinned signer"
fi
grep -q "signature is NOT valid" "$WORK/log" || fail "pinned-signer fallback did not reject the fixture"
[[ ! -e "$WORK/bin-pinned/aien" ]] || fail "pinned-signer refusal left a binary"

echo "PASS: release install verifies signature and checksum, rejects tampered and forged releases, installs standalone offline, pinned key matches allowed_signers"
