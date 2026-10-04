#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# AIEN Sovereign Stack Universal Multi-OS Developer Installer
# Single-command install for the entire sovereign monorepo ecosystem.
# Free and open-source for humanity. Zero telemetry. Zero subscriptions.
# ==============================================================================

echo "=================================================================="
echo "      ⚡ AIEN Sovereign Stack: Universal Developer Installer       "
echo "=================================================================="

# Release mode (AIEN_RELEASE_TAG=vX.Y.Z) installs a published, signed release and
# needs no source tree and no network beyond the three release assets.
RELEASE_TAG="${AIEN_RELEASE_TAG:-}"

# Release signing key, pinned here so a release can be verified with nothing but
# this file and the assets (offline). Must equal docs/release/allowed_signers;
# scripts/test-install-release.sh checks that they match.
AIEN_PINNED_SIGNER='aien-release ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAID6WDVy2x8aV80ag10iPonS4J8k4seWrgsi5cuq8AIZJ'

# Run from a checkout, or bootstrap one when the script is piped to Bash for a
# source build. Release mode runs without a checkout.
INSTALL_WORKSPACE=""
SOURCE_ROOT=""
if [[ -n "${AIEN_SOURCE_DIR:-}" ]]; then
    SOURCE_ROOT="$(cd "$AIEN_SOURCE_DIR" && pwd)"
elif [[ -f "${BASH_SOURCE[0]:-}" && -f "$(dirname "${BASH_SOURCE[0]}")/Cargo.toml" ]]; then
    SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elif [[ -z "$RELEASE_TAG" ]]; then
    INSTALL_WORKSPACE="$(mktemp -d "${TMPDIR:-/tmp}/aien-install.XXXXXX")"
    trap 'rm -rf "$INSTALL_WORKSPACE"' EXIT
    git clone --quiet https://github.com/aien-dev/aien-sovereign-core.git "$INSTALL_WORKSPACE/core"
    git -C "$INSTALL_WORKSPACE/core" checkout --quiet "${AIEN_REV:-main}"
    SOURCE_ROOT="$INSTALL_WORKSPACE/core"
fi
if [[ -n "$SOURCE_ROOT" ]]; then
    [[ -f "$SOURCE_ROOT/Cargo.toml" ]] || { echo "AIEN source manifest is missing" >&2; exit 1; }
    cd "$SOURCE_ROOT"
    echo "[*] Source commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
else
    echo "[*] Release mode without a source tree: $RELEASE_TAG"
fi
# Where the EN2 imprint comes from: the checkout, or the release archive (set below).
IMPRINT_SRC="${SOURCE_ROOT:+$SOURCE_ROOT/imprints/en2-trinity}"

OS="$(uname -s | tr "[:upper:]" "[:lower:]")"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64|amd64) TARGET_ARCH="x86_64" ;;
    aarch64|arm64) TARGET_ARCH="aarch64" ;;
    *) echo "Error: Unsupported CPU architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux) TARGET_OS="linux" ;;
    darwin) TARGET_OS="macos" ;;
    *) echo "Error: Unsupported operating system: $OS"; exit 1 ;;
esac

echo "[*] Detected Platform: $TARGET_OS ($TARGET_ARCH)"

# Release asset names: linux uses x86_64 or aarch64, macOS uses arm64.
RELEASE_ARCH="$TARGET_ARCH"
[[ "$TARGET_OS" != macos || "$TARGET_ARCH" != aarch64 ]] || RELEASE_ARCH="arm64"

# Release mode (AIEN_RELEASE_TAG=vX.Y.Z): install a published, signed release
# instead of building from source. Fails closed: nothing is installed unless
# SHA256SUMS.txt carries a valid signature from the key pinned in
# docs/release/allowed_signers and the archive matches its listed checksum.
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

install_release() {
    local tag="$1"
    command -v ssh-keygen >/dev/null 2>&1 || { echo "Error: ssh-keygen is required to verify the release signature" >&2; exit 1; }
    command -v curl >/dev/null 2>&1 || { echo "Error: curl is required to download the release" >&2; exit 1; }
    local base="${AIEN_RELEASE_BASE_URL:-https://github.com/aien-dev/aien-sovereign-core/releases/download/$tag}"
    local asset="sovereign-${TARGET_OS}-${RELEASE_ARCH}.tar.gz"
    DL_DIR=""
    DL_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aien-release.XXXXXX")"
    trap 'rm -rf "$DL_DIR"; [[ -z "$INSTALL_WORKSPACE" ]] || rm -rf "$INSTALL_WORKSPACE"' EXIT
    # Signer file: explicit override, else the checkout's copy, else the key pinned above.
    local signers="${AIEN_ALLOWED_SIGNERS:-}"
    if [[ -z "$signers" && -n "$SOURCE_ROOT" && -f "$SOURCE_ROOT/docs/release/allowed_signers" ]]; then
        signers="$SOURCE_ROOT/docs/release/allowed_signers"
    fi
    if [[ -z "$signers" ]]; then
        signers="$DL_DIR/allowed_signers"
        printf '%s\n' "$AIEN_PINNED_SIGNER" > "$signers"
    fi
    [[ -f "$signers" ]] || { echo "Error: pinned signer file not found: $signers" >&2; exit 1; }
    echo "[*] Downloading $tag: $asset, SHA256SUMS.txt, SHA256SUMS.txt.sig"
    local f
    for f in SHA256SUMS.txt SHA256SUMS.txt.sig "$asset"; do
        curl --proto '=https,file' --fail --silent --show-error --location -o "$DL_DIR/$f" "$base/$f" \
            || { echo "Error: could not download $f from $base" >&2; exit 1; }
    done
    echo "[*] Verifying signature against $signers"
    ssh-keygen -Y verify -f "$signers" -I aien-release -n aien-release \
        -s "$DL_DIR/SHA256SUMS.txt.sig" < "$DL_DIR/SHA256SUMS.txt" >/dev/null \
        || { echo "Error: SHA256SUMS.txt signature is NOT valid. Nothing was installed." >&2; exit 1; }
    local want got
    want="$(awk -v a="$asset" '$2 == a || $2 == "*" a { print $1 }' "$DL_DIR/SHA256SUMS.txt")"
    [[ "$(printf '%s\n' "$want" | grep -c .)" -eq 1 ]] \
        || { echo "Error: $asset is not listed exactly once in the signed SHA256SUMS.txt" >&2; exit 1; }
    got="$(sha256_of "$DL_DIR/$asset")"
    [[ "$want" == "$got" ]] \
        || { echo "Error: checksum mismatch for $asset (signed $want, downloaded $got). Nothing was installed." >&2; exit 1; }
    echo "[+] Signature and checksum verified for $asset"
    mkdir -p "$DL_DIR/x" "$BIN_DIR"
    tar -xzf "$DL_DIR/$asset" -C "$DL_DIR/x"
    [[ -d "$DL_DIR/x/bin" ]] || { echo "Error: archive has no bin/ directory" >&2; exit 1; }
    [[ ! -d "$DL_DIR/x/imprints/en2-trinity" ]] || IMPRINT_SRC="$DL_DIR/x/imprints/en2-trinity"
    local b name
    for b in "$DL_DIR/x/bin/"*; do
        name="$(basename "$b")"
        case "$name" in
            spark-cockpit-rs) name=spark-cockpit ;;
            cortex-rs) name=cortex ;;
        esac
        cp "$b" "$BIN_DIR/$name"
        chmod +x "$BIN_DIR/$name"
        echo "    - $name -> $BIN_DIR/$name"
    done
}

# Verify Rust Toolchain (source builds only)
if [[ -z "$RELEASE_TAG" ]] && ! command -v cargo >/dev/null 2>&1; then
    echo "[!] Cargo not found. Installing Rust toolchain via rustup..."
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

BIN_DIR="${AIEN_BIN_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${AIEN_CONFIG_DIR:-$HOME/.config/sovereign}"
IMPRINT_DIR="$CONFIG_DIR/imprints/en2"

mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$IMPRINT_DIR"

# Ensure ~/.local/bin is in PATH for current and future shells
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    export PATH="$BIN_DIR:$PATH"
fi

if [[ "${AIEN_INSTALL_NO_PROFILE:-0}" != "1" ]]; then
for RC in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
    if [ -f "$RC" ]; then
        if ! grep -Fq '$HOME/.local/bin' "$RC"; then
            echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> "$RC"
        fi
    fi
done
fi

if [[ -n "$RELEASE_TAG" ]]; then
    install_release "$RELEASE_TAG"
else
echo "[*] Building Core Monorepo Binaries (High-Performance Release Mode)..."
echo "    - aien-cli (Universal Terminal CLI & Orchestrator)"
echo "    - spark-cockpit-rs (3D Sovereign Glass Cockpit Server)"
echo "    - cortex-rs (Canonical Memory & Vector Store)"
echo "    - spark-supervisor (Resilient Process Supervisor)"
echo "    - spark-debugger (Zero-Leak Health & Telemetry Auditor)"
echo "    - spark-harness (5-Layer Verification Engine)"
echo "    - spark-crumbs (Blake3 Scent & Action Vector Tracker)"
echo "    - spark-aegis (Defensive Boundary & Containment Engine)"

cargo build --locked --release -p aien-cli -p spark-cockpit-rs -p cortex-rs -p spark-supervisor -p spark-debugger -p spark-harness -p spark-crumbs -p spark-aegis

echo "[*] Installing Binaries to $BIN_DIR..."
cp target/release/aien-cli "$BIN_DIR/aien"
cp target/release/spark-cockpit-rs "$BIN_DIR/spark-cockpit"
cp target/release/cortex-rs "$BIN_DIR/cortex"
cp target/release/spark-supervisor "$BIN_DIR/spark-supervisor"
cp target/release/spark-debugger "$BIN_DIR/spark-debugger"
cp target/release/spark-harness "$BIN_DIR/spark-harness"
cp target/release/spark-crumbs "$BIN_DIR/spark-crumbs"
cp target/release/spark-aegis "$BIN_DIR/spark-aegis"
chmod +x "$BIN_DIR/aien" "$BIN_DIR/spark-cockpit" "$BIN_DIR/cortex" "$BIN_DIR/spark-supervisor" "$BIN_DIR/spark-debugger" "$BIN_DIR/spark-harness" "$BIN_DIR/spark-crumbs" "$BIN_DIR/spark-aegis"

echo "[+] Binaries successfully linked:"
echo "    - aien -> $BIN_DIR/aien"
echo "    - spark-cockpit -> $BIN_DIR/spark-cockpit"
echo "    - cortex -> $BIN_DIR/cortex"
echo "    - spark-supervisor -> $BIN_DIR/spark-supervisor"
echo "    - spark-debugger -> $BIN_DIR/spark-debugger"
echo "    - spark-harness -> $BIN_DIR/spark-harness"
echo "    - spark-crumbs -> $BIN_DIR/spark-crumbs"
echo "    - spark-aegis -> $BIN_DIR/spark-aegis"
fi

echo "[*] Initializing Sovereign Operator Profile..."
if [ ! -f "$CONFIG_DIR/operator.toml" ]; then
    GIT_NAME="$(git config user.name 2>/dev/null || echo "Sovereign Operator")"
    GIT_EMAIL="$(git config user.email 2>/dev/null || echo "operator@local")"
    
    cat << CFG > "$CONFIG_DIR/operator.toml"
[operator]
name = "$GIT_NAME"
email = "$GIT_EMAIL"
handle = "operator"
sign_commits = false

[engine]
mode = "max"
api_base_url = "http://127.0.0.1:18006/v1"
api_key = ""
model_id = "atlas-lightning-omni"
max_port = 18006
context_window = 32768
temperature = 0.7
CFG
    echo "[+] Generated default configuration at $CONFIG_DIR/operator.toml"
else
    echo "[+] Existing configuration found at $CONFIG_DIR/operator.toml"
fi

echo "[*] Installing Free EN2 Trinity Imprint..."
if [[ -n "$IMPRINT_SRC" && -d "$IMPRINT_SRC" ]]; then
    cp -r "$IMPRINT_SRC"/* "$IMPRINT_DIR/"
    echo "[+] EN2 Trinity Imprint installed locally to $IMPRINT_DIR"
fi

echo "=================================================================="
echo "  ⚡ Portable binaries installed. Model and access-token provisioning remain."
echo "=================================================================="
echo "  Primary Command: aien"
echo ""
echo "  Quickstart:"
echo "    aien start      # Starts Cortex, Cockpit, and background daemons"
echo "    aien status     # Real-time health, ports, and memory status"
echo "    aien cockpit    # Launches Sovereign Glass Cockpit in browser"
echo "    aien chat       # Interactive sovereign terminal pairing"
echo "    aien harness    # Run 5-layer verification checks"
echo "    aien aegis      # Run defensive boundary security audit"
echo "    aien doctor     # System diagnostic and TPM key vault audit"
echo "=================================================================="
