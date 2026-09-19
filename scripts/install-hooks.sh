#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
HOOKS_DIR="$ROOT_DIR/.git/hooks"

mkdir -p "$HOOKS_DIR"

cat << 'HOOK_EOF' > "$HOOKS_DIR/pre-commit"
#!/usr/bin/env bash
set -euo pipefail

# Sovereign Pre-Commit Hook: Enforcing Security Invariants & Advisory Styling
if ! git diff --cached --quiet; then
    TMP_DIFF="$(mktemp --suffix=.diff)"
    git diff --cached > "$TMP_DIFF"

    if command -v spark-inquisitor >/dev/null 2>&1; then
        spark-inquisitor audit --diff "$TMP_DIFF" --advisory-style
        STATUS=$?
        rm -f "$TMP_DIFF"
        if [ $STATUS -ne 0 ]; then
            echo "Pre-commit hook failed: critical security invariants violated (plaintext secrets or telemetry)."
            exit 1
        fi
    else
        rm -f "$TMP_DIFF"
    fi
fi
HOOK_EOF

chmod +x "$HOOKS_DIR/pre-commit"
echo "Sovereign pre-commit hook installed successfully in $HOOKS_DIR/pre-commit"
