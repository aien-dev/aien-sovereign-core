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
[[ -n "${KEEP:-}" ]] || trap 'rm -rf "$WORK"' EXIT
fail() { echo "FAIL: $*" >&2; [[ -z "${KEEP:-}" ]] || echo "WORK=$WORK" >&2; exit 1; }

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

make_release() { # dir signing-key [candidate] [variant]
    local d="$1" k="$2" cand="${3:-CAND-1}" variant="${4:-a}" f aien
    rm -rf "$WORK/pkg"
    mkdir -p "$d" "$WORK/pkg/bin" "$WORK/pkg/imprints/en2-trinity"
    printf '#!/bin/sh\necho aien-%s\nexit 0\n' "$variant" > "$WORK/pkg/bin/aien"
    printf '#!/bin/sh\nexit 0\n' > "$WORK/pkg/bin/cortex-rs"
    printf 'fixture imprint\n' > "$WORK/pkg/imprints/en2-trinity/soul.md"
    chmod 755 "$WORK/pkg/bin/"*
    aien="$(sha256sum "$WORK/pkg/bin/aien" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$WORK/pkg/bin/aien" | cut -d' ' -f1)"
    {
        printf 'schema = "AienReleaseV1"\ncandidate = "%s"\naien-cli-sha256 = "%s"\n\n[files]\n' "$cand" "$aien"
        (cd "$WORK/pkg" && find . -type f ! -name release.toml | LC_ALL=C sort | sed 's|^\./||') | while read -r f; do
            printf '"%s" = "%s"\n' "$f" "$(sha256sum "$WORK/pkg/$f" | cut -d' ' -f1)"
        done
    } > "$WORK/pkg/release.toml"
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


# 7. upgrade: a newer candidate installs over the first; one previous release is kept; the
#    record names both; operator.toml is untouched
SIGN_ENV=(AIEN_SOURCE_DIR="$ROOT" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers")
live() { readlink "$1/releases/current"; }
inst_args() { # bin cfg args... : run install.sh with arguments in explicit dirs
    local bin="$1" cfg="$2"; shift 2
    env AIEN_BIN_DIR="$bin" AIEN_CONFIG_DIR="$cfg" AIEN_INSTALL_NO_PROFILE=1 HOME="$WORK/home" \
        bash "$ROOT/install.sh" "$@" > "$WORK/log" 2>&1
}
echo '# local edit' >> "$WORK/cfg-good/operator.toml"
OP_BEFORE="$(sha256sum "$WORK/cfg-good/operator.toml" | cut -d' ' -f1)"
ID1="$(live "$WORK/cfg-good")"
[[ -n "$ID1" && -f "$WORK/cfg-good/installed.toml" ]] || fail "first install left no live release or record"
[[ "$("$WORK/bin-good/aien")" == "aien-a" ]] || fail "first install does not run"
make_release "$WORK/rel2" "$WORK/key" CAND-2 b
run_install "$WORK/rel2" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" "${SIGN_ENV[@]}" \
    || { cat "$WORK/log" >&2; fail "upgrade did not install"; }
ID2="$(live "$WORK/cfg-good")"
[[ "$ID2" != "$ID1" ]] || fail "upgrade did not change the live release"
[[ "$("$WORK/bin-good/aien")" == "aien-b" ]] || fail "installed aien is not the upgraded one"
grep -q "^previous = \"$ID1\"" "$WORK/cfg-good/installed.toml" || fail "record does not name the previous release"
grep -q 'candidate = "CAND-2"' "$WORK/cfg-good/installed.toml" || fail "record lacks the new candidate"
[[ "$(sha256sum "$WORK/cfg-good/operator.toml" | cut -d' ' -f1)" == "$OP_BEFORE" ]] || fail "upgrade changed operator.toml"
[[ "$(ls "$WORK/cfg-good/releases" | grep -vc '^current$')" -eq 2 ]] || fail "expected exactly two releases kept"

# 8. rollback returns to the first release and records it; a second rollback goes forward again
inst_args "$WORK/bin-good" "$WORK/cfg-good" --rollback || { cat "$WORK/log" >&2; fail "rollback failed"; }
[[ "$(live "$WORK/cfg-good")" == "$ID1" && "$("$WORK/bin-good/aien")" == "aien-a" ]] || fail "rollback did not restore the first release"
grep -q '^last-action = "rollback"' "$WORK/cfg-good/installed.toml" || fail "rollback not recorded"
grep -q "^previous = \"$ID2\"" "$WORK/cfg-good/installed.toml" || fail "record does not name the rolled-back-from release"
inst_args "$WORK/bin-good" "$WORK/cfg-good" --rollback || fail "second rollback failed"
[[ "$(live "$WORK/cfg-good")" == "$ID2" ]] || fail "second rollback did not swap forward"
inst_args "$WORK/bin-good" "$WORK/cfg-good" --rollback || fail "third rollback failed"
if inst_args "$WORK/bin-none" "$WORK/cfg-none" --rollback; then fail "rollback with nothing installed succeeded"; fi
grep -q "no previous release" "$WORK/log" || fail "rollback with nothing installed not explained"

# 9. downgrade guard: an older candidate is refused unless --allow-downgrade
inst_args "$WORK/bin-good" "$WORK/cfg-good" --rollback >/dev/null || true   # live = ID2 (CAND-2)
[[ "$(live "$WORK/cfg-good")" == "$ID2" ]] || fail "setup for the downgrade case failed"
make_release "$WORK/rel0" "$WORK/key" CAND-1 c
if run_install "$WORK/rel0" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" "${SIGN_ENV[@]}"; then
    fail "older candidate installed without --allow-downgrade"
fi
grep -q "allow-downgrade" "$WORK/log" || fail "downgrade refusal not explained"
[[ "$(live "$WORK/cfg-good")" == "$ID2" ]] || fail "refused downgrade changed the live release"
env AIEN_RELEASE_TAG=v0.0.0-test AIEN_RELEASE_BASE_URL="file://$WORK/rel0" AIEN_BIN_DIR="$WORK/bin-good" \
    AIEN_CONFIG_DIR="$WORK/cfg-good" AIEN_INSTALL_NO_PROFILE=1 "${SIGN_ENV[@]}" \
    bash "$ROOT/install.sh" --allow-downgrade > "$WORK/log" 2>&1 \
    || { cat "$WORK/log" >&2; fail "downgrade with --allow-downgrade failed"; }

# 10. package that does not match its own release.toml installs nothing
make_release "$WORK/badpkg" "$WORK/key" CAND-9 d
mkdir -p "$WORK/unp"; tar -xzf "$WORK/badpkg/$ASSET" -C "$WORK/unp"; printf 'x' >> "$WORK/unp/bin/cortex-rs"
tar -czf "$WORK/badpkg/$ASSET" -C "$WORK/unp" .
(cd "$WORK/badpkg" && sha256sum "$ASSET" > SHA256SUMS.txt && rm -f SHA256SUMS.txt.sig \
    && ssh-keygen -q -Y sign -f "$WORK/key" -n aien-release SHA256SUMS.txt)
LIVE_BEFORE="$(live "$WORK/cfg-good")"
if run_install "$WORK/badpkg" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" "${SIGN_ENV[@]}"; then
    fail "package with a file that does not match release.toml was installed"
fi
grep -q "does not match its release.toml" "$WORK/log" || fail "manifest mismatch not reported"
[[ "$(live "$WORK/cfg-good")" == "$LIVE_BEFORE" ]] || fail "rejected package changed the live release"

# 11. interrupted install: kill the installer mid-copy and before the swap; the live install is
#     still the old version, runs, and a rerun completes the upgrade
make_release "$WORK/rel3" "$WORK/key" CAND-3 e
for point in mid-copy after-copy before-swap; do
    OLD="$(live "$WORK/cfg-good")"
    env AIEN_TEST_PAUSE_AT="$point" AIEN_RELEASE_TAG=v0.0.0-test AIEN_RELEASE_BASE_URL="file://$WORK/rel3" \
        AIEN_BIN_DIR="$WORK/bin-good" AIEN_CONFIG_DIR="$WORK/cfg-good" AIEN_INSTALL_NO_PROFILE=1 \
        AIEN_SOURCE_DIR="$ROOT" AIEN_ALLOWED_SIGNERS="$WORK/allowed_signers" \
        bash "$ROOT/install.sh" > "$WORK/log-int" 2>&1 &
    IPID=$!
    for _ in $(seq 1 100); do grep -q "paused at $point" "$WORK/log-int" 2>/dev/null && break; sleep 0.1; done
    grep -q "paused at $point" "$WORK/log-int" || fail "installer never reached $point"
    pkill -9 -P "$IPID" 2>/dev/null || true; kill -9 "$IPID"; wait "$IPID" 2>/dev/null || true
    [[ "$(live "$WORK/cfg-good")" == "$OLD" ]] || fail "killed at $point changed the live release"
    out="$("$WORK/bin-good/aien")"; [[ "$out" == "aien-b" || "$out" == "aien-c" ]] || fail "live install does not run after kill at $point ($out)"
done
run_install "$WORK/rel3" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" "${SIGN_ENV[@]}" \
    || { cat "$WORK/log" >&2; fail "rerun after interruption failed"; }
[[ "$("$WORK/bin-good/aien")" == "aien-e" ]] || fail "rerun did not install the new release"
[[ -z "$(ls -A "$WORK/cfg-good/releases" | grep '^\.stage\.' || true)" ]] || fail "leftover stage after rerun"

# 12. an existing release directory that is incomplete is never overwritten
ID3="$(live "$WORK/cfg-good")"
make_release "$WORK/rel4" "$WORK/key" CAND-4 f
ID4="$(sha256sum "$WORK/pkg/bin/aien" | cut -c1-12)"
mkdir -p "$WORK/cfg-good/releases/$ID4"; echo partial > "$WORK/cfg-good/releases/$ID4/junk"
if run_install "$WORK/rel4" "$WORK/bin-good" "$WORK/cfg-good" "$ROOT/install.sh" "${SIGN_ENV[@]}"; then
    fail "install over an incomplete release directory succeeded"
fi
grep -q "was not overwritten" "$WORK/log" || fail "incomplete release directory not reported"
[[ "$(live "$WORK/cfg-good")" == "$ID3" && -f "$WORK/cfg-good/releases/$ID4/junk" ]] || fail "refusal changed state"
echo "PASS: release install verifies signature, checksum and every package file, rejects tampered and forged releases, installs standalone offline, pinned key matches allowed_signers, upgrades atomically, keeps one previous release, rolls back, refuses downgrades, survives an interrupted install"
