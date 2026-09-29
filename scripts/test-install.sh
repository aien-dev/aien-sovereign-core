#!/usr/bin/env bash
# Exercise piped installation (install.sh fed to bash on stdin) from an
# arbitrary working directory without network access or real builds.
# git and cargo are replaced by small stand-in scripts on PATH.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROTOCOLS_REV="$(sed -n 's/^PROTOCOLS_REV="\([0-9a-f]\{40\}\)"$/\1/p' "$ROOT/install.sh")"
[[ -n "$PROTOCOLS_REV" ]] || { echo "FAIL: PROTOCOLS_REV not found in install.sh" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/aien-test-install.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/mocks" "$WORK/cwd" "$WORK/tmp"

fail() {
    echo "FAIL: $*" >&2
    [[ -f "$WORK/install.log" ]] && sed 's/^/    install: /' "$WORK/install.log" >&2
    exit 1
}

# Stand-in git: clone creates a directory holding a Cargo.toml, rev-parse
# reports the pinned protocols revision, config answers operator identity.
cat > "$WORK/mocks/git" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$WORK/git.calls"
case "\$1" in
    clone)
        dest="\${!#}"
        mkdir -p "\$dest"
        printf '[workspace]\n' > "\$dest/Cargo.toml"
        ;;
    -C)
        case "\$3" in
            rev-parse) echo "$PROTOCOLS_REV" ;;
        esac
        ;;
    rev-parse) echo "$PROTOCOLS_REV" ;;
    config)
        if [[ "\${!#}" == user.name ]]; then echo "Fixture Operator"; else echo "fixture@example.test"; fi
        ;;
esac
exit 0
EOF

# Stand-in cargo: checks it runs inside a checkout with a locked build that
# includes every installed package, then writes placeholder binaries.
cat > "$WORK/mocks/cargo" <<'EOF'
#!/usr/bin/env bash
[[ -f Cargo.toml ]] || { echo "installer did not locate checkout" >&2; exit 1; }
args=" $* "
[[ "$args" == *" --locked "* ]] || { echo "cargo build is not --locked" >&2; exit 1; }
for pkg in aien-cli spark-cockpit-rs cortex-rs spark-supervisor spark-debugger spark-harness spark-crumbs spark-aegis; do
    [[ "$args" == *" -p $pkg "* ]] || { echo "cargo build is missing package $pkg" >&2; exit 1; }
    mkdir -p target/release
    printf '#!/bin/sh\nexit 0\n' > "target/release/$pkg"
    chmod 755 "target/release/$pkg"
done
EOF
chmod 755 "$WORK/mocks/git" "$WORK/mocks/cargo"

set +e
(
    cd "$WORK/cwd"
    unset AIEN_SOURCE_DIR AIEN_REV
    PATH="$WORK/mocks:$PATH" \
    TMPDIR="$WORK/tmp" \
    AIEN_BIN_DIR="$WORK/bin" \
    AIEN_CONFIG_DIR="$WORK/config" \
    AIEN_INSTALL_NO_PROFILE=1 \
        bash < "$ROOT/install.sh"
) > "$WORK/install.log" 2>&1
status=$?
set -e

[[ $status -eq 0 ]] || fail "piped install.sh exited with status $status"
for bin in aien spark-cockpit cortex spark-supervisor spark-debugger spark-harness spark-crumbs spark-aegis; do
    [[ -x "$WORK/bin/$bin" ]] || fail "binary $bin was not installed"
done
[[ -f "$WORK/config/operator.toml" ]] || fail "operator.toml was not generated"
grep -q '^name = "Fixture Operator"$' "$WORK/config/operator.toml" || fail "operator.toml lacks git identity"
grep -q 'clone --quiet https://github.com/aien-dev/aien-sovereign-core.git' "$WORK/git.calls" || fail "installer did not clone the core repository"
grep -q "checkout --quiet $PROTOCOLS_REV" "$WORK/git.calls" || fail "installer did not pin aien-protocols"
[[ -z "$(ls -A "$WORK/tmp")" ]] || fail "installer left its temporary checkout behind"
[[ -z "$(ls -A "$WORK/cwd")" ]] || fail "installer wrote into the caller's working directory"

echo "PASS: piped installer bootstraps a checkout and installs the CLI from an arbitrary cwd"
