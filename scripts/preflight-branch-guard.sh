#!/usr/bin/env bash
# Branch policy check used by agent-preflight.sh.
#
# AIEN_PREFLIGHT_CONTEXT selects what is being validated:
#   developer    (default) work in progress. Direct work on main is refused.
#   merged-tree  a tree that already landed on main (CI on push to main).
#                Being on main is expected, so the branch rule does not apply.
# Any other value is an error, so a typo can never silently skip the guard.
set -eo pipefail

CONTEXT="${AIEN_PREFLIGHT_CONTEXT:-developer}"
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")

case "$CONTEXT" in
    developer)
        if [ "$BRANCH" = "main" ]; then
            echo "FAILED"
            echo "Error: Cannot commit directly to main. Create a feature branch (e.g. feat/, fix/)."
            exit 1
        fi
        echo "PASSED (Branch: $BRANCH)"
        ;;
    merged-tree)
        # Only honoured on the real CI runner validating main; a local shell cannot claim it.
        if [ "${GITHUB_ACTIONS:-}" != "true" ] || [ "${GITHUB_REF:-}" != "refs/heads/main" ]; then
            echo "FAILED"
            echo "Error: merged-tree context is only valid in GitHub Actions on refs/heads/main."
            exit 1
        fi
        echo "PASSED (merged-tree context; branch rule not applicable, checked out: $BRANCH)"
        ;;
    *)
        echo "FAILED"
        echo "Error: unknown AIEN_PREFLIGHT_CONTEXT '$CONTEXT' (use developer or merged-tree)."
        exit 1
        ;;
esac
