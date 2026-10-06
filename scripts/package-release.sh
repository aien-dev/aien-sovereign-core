#!/usr/bin/env bash
# Package the release binaries into a byte-reproducible tarball.
#
#   scripts/package-release.sh <output.tar.gz>
#
# Environment:
#   AIEN_DRY_RUN=1        package even when the release gate fails (candidate "NOT-RELEASABLE"); never for a release
#   AIEN_RELEASE_BIN_DIR  directory holding the built binaries (default target/release)
#   SOURCE_DATE_EPOCH     mtime for every entry (default 0)
#
# The archive carries release.toml: schema, candidate id, the sha256 of the packaged aien-cli,
# the optional [model] table and a [files] table with the sha256 of every other file. install.sh
# verifies every listed file against it before it activates a release.
# Same inputs give the same bytes: sorted entries, fixed mtime, owner 0, no atime (gnu format),
# normalised modes, gzip -n (no name or time in the gzip header).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ARTIFACT="${1:?provide output archive path}"
BIN_DIR="${AIEN_RELEASE_BIN_DIR:-target/release}"
# The candidate id and model table come from the release gate, never from the caller. A tree that fails the
# gate is not packaged, except with AIEN_DRY_RUN=1, which labels the package NOT-RELEASABLE (it then fails
# check-release-candidate.sh --package and cannot be installed as a candidate).
GATE_MODEL="$(mktemp)"
if CANDIDATE="$(bash scripts/check-release-candidate.sh --model "$GATE_MODEL" | sed -n 's/^candidate=//p')" && [[ -n "$CANDIDATE" ]]; then
    AIEN_MODEL_INPUTS="$GATE_MODEL"
elif [[ "${AIEN_DRY_RUN:-}" == 1 ]]; then
    echo "WARNING: release gate failed; dry run packages this tree as NOT-RELEASABLE" >&2
    CANDIDATE="NOT-RELEASABLE"; AIEN_MODEL_INPUTS=""
else
    echo "release gate failed; refusing to package" >&2; exit 1
fi
EPOCH="${SOURCE_DATE_EPOCH:-0}"
TAR=tar
command -v gtar >/dev/null 2>&1 && TAR=gtar
"$TAR" --version 2>/dev/null | grep -q 'GNU tar' || { echo "GNU tar is required (on macOS: brew install gnu-tar)" >&2; exit 1; }
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
PACKAGE_DIR="$(mktemp -d)"
trap 'rm -rf "$PACKAGE_DIR" "$GATE_MODEL"' EXIT
mkdir -p "$PACKAGE_DIR/bin"
for binary in aien-cli spark-cockpit-rs spark-inquisitor cortex-encoder-rs cortex-rs spark-supervisor spark-debugger; do
    [[ -x "$BIN_DIR/$binary" ]] || { echo "Missing release binary: $binary" >&2; exit 1; }
    name="$binary"
    [[ "$binary" != aien-cli ]] || name=aien
    cp "$BIN_DIR/$binary" "$PACKAGE_DIR/bin/$name"
done
cp CONSTITUTION.md README.md install.sh "$PACKAGE_DIR/"
if [[ -d imprints/en2-trinity ]]; then
    mkdir -p "$PACKAGE_DIR/imprints"
    cp -R imprints/en2-trinity "$PACKAGE_DIR/imprints/"
fi
find "$PACKAGE_DIR" -type d -exec chmod 755 {} +
find "$PACKAGE_DIR" -type f -exec chmod 644 {} +
chmod 755 "$PACKAGE_DIR/bin/"* "$PACKAGE_DIR/install.sh"
{
    echo 'schema = "AienReleaseV1"'
    echo "candidate = \"$CANDIDATE\""
    echo "aien-cli-sha256 = \"$(sha "$PACKAGE_DIR/bin/aien")\""
    if [[ -n "${AIEN_MODEL_INPUTS:-}" ]]; then
        echo
        grep -q '^\[model\]' "$AIEN_MODEL_INPUTS" || { echo "AIEN_MODEL_INPUTS must start with [model]" >&2; exit 1; }
        cat "$AIEN_MODEL_INPUTS"
    fi
    echo
    echo '[files]'
    (cd "$PACKAGE_DIR" && find . -type f ! -name release.toml | LC_ALL=C sort | sed 's|^\./||') | while read -r f; do
        printf '"%s" = "%s"\n' "$f" "$(sha "$PACKAGE_DIR/$f")"
    done
} > "$PACKAGE_DIR/release.toml"
chmod 644 "$PACKAGE_DIR/release.toml"
(cd "$PACKAGE_DIR" && "$TAR" --sort=name --mtime="@$EPOCH" --owner=0 --group=0 --numeric-owner \
    --format=gnu -cf - .) | gzip -n -9 > "$ARTIFACT"
echo "packaged $ARTIFACT sha256 $(sha "$ARTIFACT")"
