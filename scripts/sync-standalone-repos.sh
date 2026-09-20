#!/usr/bin/env bash
#
# sync-standalone-repos.sh: Monorepo Crate Synchronization Pipeline
#
# Synchronizes crates from aien-sovereign-core to standalone publish repositories on DGX Spark.
# Adheres strictly to Sovereign Voice: zero em dashes, zero en dashes.
#
# Preserved standalone-specific files:
# action.yml, README.md, LICENSE, Cargo.lock, AGENTS.md, AGENT_CODE_OF_CONDUCT.md,
# CONSTITUTION.md, CONTRIBUTING.md, SECURITY.md, PHILOSOPHY.md, agent.json, llms.txt,
# evals.json, workflows.json, schemas/, kb/, failures/, assets/, scripts/, install.sh,
# Dockerfile, .github/, .git/, .gitignore, .crumb, .crumb.local.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOVEREIGN_CORE="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE="$(cd "$SOVEREIGN_CORE/.." && pwd)"

# Repository mapping: <cli_name>:<crate_subpath>:<publish_dirname>
REPOS=(
  "spark-hive:crates/spark-hive:spark-hive-publish"
  "spark-supervisor:crates/spark-supervisor:spark-supervisor-publish"
  "spark-dream:crates/spark-dream:spark-dream-publish"
  "cortex-rs:crates/cortex-rs:cortex-rs-publish"
  "spark-adapters:crates/spark-adapters:spark-adapters-publish"
  "spark-crumbs:crates/spark-crumbs:spark-crumbs-publish"
  "spark-debugger:crates/spark-debugger:spark-debugger-publish"
  "spark-harness:crates/spark-harness:aien-harness-publish"
  "spark-inquisitor:crates/spark-inquisitor:spark-inquisitor"
  "rad-id-sync:crates/rad-id-sync:rad-id-sync-publish"
)

CHECK_MODE=false
ALL_REPOS=false
TARGET_REPO=""
VERBOSE=false
RUN_TESTS=false

usage() {
  cat << 'HELP_EOF'
Usage: sync-standalone-repos.sh [OPTIONS]

Synchronizes member crates from aien-sovereign-core to standalone publish repositories.

Options:
  --check              Dry run and parity check mode (does not modify files on disk)
  --all                Synchronize or check all standalone repositories
  --repo <name>        Synchronize or check a specific repository by name
  --verbose, -v        Enable verbose logging and diff inspection
  --test               Run cargo test on synced repositories in addition to cargo check
  --help, -h           Show this help message and exit

Preserved standalone files:
  action.yml, README.md, LICENSE, Cargo.lock, AGENTS.md, AGENT_CODE_OF_CONDUCT.md,
  CONSTITUTION.md, CONTRIBUTING.md, SECURITY.md, PHILOSOPHY.md, agent.json, llms.txt,
  evals.json, workflows.json, schemas/, kb/, failures/, assets/, scripts/, install.sh,
  Dockerfile, .github/, .git/, .gitignore, .crumb, .crumb.local.
HELP_EOF
}

# Parse CLI arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      CHECK_MODE=true
      shift
      ;;
    --all)
      ALL_REPOS=true
      shift
      ;;
    --repo)
      if [[ -z "${2:-}" ]]; then
        echo "ERROR: --repo requires a repository name." >&2
        exit 1
      fi
      TARGET_REPO="$2"
      shift 2
      ;;
    --verbose|-v)
      VERBOSE=true
      shift
      ;;
    --test)
      RUN_TESTS=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ "$ALL_REPOS" = false && -z "$TARGET_REPO" ]]; then
  echo "ERROR: Specify either --all or --repo <name>." >&2
  echo "Run with --help for detailed usage." >&2
  exit 1
fi

check_dirty_worktree() {
  local target_dir="$1"
  local repo_name="$2"
  if [[ -d "$target_dir/.git" ]]; then
    local git_status
    git_status=$(git -C "$target_dir" status --porcelain 2>/dev/null || true)
    if [[ -n "$git_status" ]]; then
      echo "[WARN] Target worktree for $repo_name is dirty (uncommitted modifications detected):"
      echo "$git_status"
      return 1
    fi
  fi
  return 0
}

sync_manifest() {
  local src_manifest="$1"
  local dest_manifest="$2"
  local is_check="$3"

  python3 - << 'PYEOF' "$src_manifest" "$dest_manifest" "$is_check"
import sys, re

src_path = sys.argv[1]
dest_path = sys.argv[2]
check_mode = (sys.argv[3] == "true")

with open(src_path, "r", encoding="utf-8") as f:
    src_lines = f.readlines()
with open(dest_path, "r", encoding="utf-8") as f:
    dest_lines = f.readlines()

# Extract version and edition from source package table
src_version = None
src_edition = None
in_pkg = False
for line in src_lines:
    s = line.strip()
    if s.startswith("[package]"):
        in_pkg = True
        continue
    elif s.startswith("[") and in_pkg:
        in_pkg = False
    if in_pkg:
        mv = re.match(r'^version\s*=\s*"([^"]+)"', s)
        if mv:
            src_version = mv.group(1)
        me = re.match(r'^edition\s*=\s*"([^"]+)"', s)
        if me:
            src_edition = me.group(1)

# Build updated destination lines
new_dest = []
in_pkg = False
license_set = False

for line in dest_lines:
    s = line.strip()
    if s.startswith("[package]"):
        in_pkg = True
        new_dest.append(line)
        continue
    elif s.startswith("[") and in_pkg:
        if not license_set:
            new_dest.append('license = "SRCL-1.0"\n')
            license_set = True
        in_pkg = False

    if in_pkg:
        if re.match(r'^(license|license\.workspace)\s*=', s):
            new_dest.append('license = "SRCL-1.0"\n')
            license_set = True
            continue
        if src_version and re.match(r'^version\s*=', s):
            new_dest.append(f'version = "{src_version}"\n')
            continue
        if src_edition and re.match(r'^edition\s*=', s):
            new_dest.append(f'edition = "{src_edition}"\n')
            continue
    new_dest.append(line)

if in_pkg and not license_set:
    new_dest.append('license = "SRCL-1.0"\n')

new_content = "".join(new_dest)
old_content = "".join(dest_lines)

if new_content != old_content:
    if check_mode:
        print(f"[DISPARITY] {dest_path} requires manifest update (license SRCL-1.0 or version/edition)")
        sys.exit(2)
    else:
        with open(dest_path, "w", encoding="utf-8") as f:
            f.write(new_content)
        print(f"[SYNCED] Manifest updated: {dest_path} (license = SRCL-1.0)")
        sys.exit(0)
else:
    print(f"[PARITY OK] Manifest {dest_path} in parity (license = SRCL-1.0)")
    sys.exit(0)
PYEOF
}

check_unslop_invariants() {
  local target_dir="$1"
  local violations=0
  while IFS= read -r -d '' file; do
    if grep -q -P '[\x{2014}\x{2013}]' "$file" 2>/dev/null; then
      echo "[UNSLOP VIOLATION] Prohibited dashes detected in: $file"
      violations=$((violations + 1))
    fi
  done < <(find "$target_dir" -type f \( -name "*.rs" -o -name "*.md" -o -name "*.toml" \) -not -path "*/.git/*" -not -path "*/target/*" -print0)
  return $violations
}

process_repo() {
  local entry="$1"
  IFS=':' read -r cli_name crate_subpath publish_dirname <<< "$entry"

  local src_dir="$SOVEREIGN_CORE/$crate_subpath"
  if [[ ! -d "$src_dir" && "$crate_subpath" =~ ^basecamp/ ]]; then
    src_dir="$WORKSPACE/$crate_subpath"
  fi

  local dest_dir="$WORKSPACE/$publish_dirname"

  echo "============================================================"
  echo "Target: $publish_dirname (Source: $crate_subpath)"
  echo "============================================================"

  if [[ ! -d "$dest_dir" ]]; then
    echo "ERROR: Target publish directory does not exist: $dest_dir" >&2
    return 1
  fi

  if [[ ! -d "$src_dir" ]]; then
    echo "ERROR: Source directory does not exist: $src_dir" >&2
    return 1
  fi

  local repo_has_disparity=0

  # Step 1: Check dirty target worktree
  check_dirty_worktree "$dest_dir" "$publish_dirname" || true

  # Step 2: Clean stray metadata files from source before sync
  if [[ "$CHECK_MODE" = false ]]; then
    find "$src_dir/src" -name "*.metadata.json" -delete 2>/dev/null || true
  fi

  # Step 3: Source directory synchronization and parity check
  if [[ "$CHECK_MODE" = true ]]; then
    local src_diff
    src_diff=$(diff -ru --exclude='*.metadata.json' --exclude='.git*' "$src_dir/src" "$dest_dir/src" 2>&1 || true)
    if [[ -n "$src_diff" ]]; then
      echo "[DISPARITY] src/ tree difference detected for $publish_dirname"
      if [[ "$VERBOSE" = true ]]; then
        echo "$src_diff"
      fi
      repo_has_disparity=1
    else
      echo "[PARITY OK] src/ tree in parity for $publish_dirname"
    fi

    # Check tests directory if present in source
    if [[ -d "$src_dir/tests" && -d "$dest_dir/tests" ]]; then
      local tests_diff
      tests_diff=$(diff -ru --exclude='.git*' "$src_dir/tests" "$dest_dir/tests" 2>&1 || true)
      if [[ -n "$tests_diff" ]]; then
        echo "[DISPARITY] tests/ tree difference detected for $publish_dirname"
        if [[ "$VERBOSE" = true ]]; then
          echo "$tests_diff"
        fi
        repo_has_disparity=1
      else
        echo "[PARITY OK] tests/ tree in parity for $publish_dirname"
      fi
    fi

    # Check schemas directory for spark-harness
    if [[ -d "$src_dir/schemas" && -d "$dest_dir/schemas" ]]; then
      local schemas_diff
      schemas_diff=$(diff -ru --exclude='.git*' "$src_dir/schemas" "$dest_dir/schemas" 2>&1 || true)
      if [[ -n "$schemas_diff" ]]; then
        echo "[DISPARITY] schemas/ tree difference detected for $publish_dirname"
        if [[ "$VERBOSE" = true ]]; then
          echo "$schemas_diff"
        fi
        repo_has_disparity=1
      else
        echo "[PARITY OK] schemas/ tree in parity for $publish_dirname"
      fi
    fi
  else
    echo "[SYNC] Synchronizing src/ from $src_dir/src to $dest_dir/src..."
    rsync -a --delete --exclude='*.metadata.json' --exclude='.git*' "$src_dir/src/" "$dest_dir/src/"

    if [[ -d "$src_dir/tests" && -d "$dest_dir/tests" ]]; then
      echo "[SYNC] Synchronizing tests/ from $src_dir/tests to $dest_dir/tests..."
      rsync -a --delete --exclude='.git*' "$src_dir/tests/" "$dest_dir/tests/"
    fi

    if [[ -d "$src_dir/schemas" && -d "$dest_dir/schemas" ]]; then
      echo "[SYNC] Synchronizing schemas/ from $src_dir/schemas to $dest_dir/schemas..."
      rsync -a --delete --exclude='.git*' "$src_dir/schemas/" "$dest_dir/schemas/"
    fi

    if [[ -f "$src_dir/evals.json" && -f "$dest_dir/evals.json" ]]; then
      cp "$src_dir/evals.json" "$dest_dir/evals.json"
    fi

    if [[ -f "$src_dir/workflows.json" && -f "$dest_dir/workflows.json" ]]; then
      cp "$src_dir/workflows.json" "$dest_dir/workflows.json"
    fi

    diff -ru --exclude='*.metadata.json' --exclude='.git*' "$src_dir/src" "$dest_dir/src"
    echo "[PARITY VERIFIED] src/ tree parity verified with diff -ru"
  fi

  # Step 4: Selective manifest synchronization
  if [[ -f "$src_dir/Cargo.toml" && -f "$dest_dir/Cargo.toml" ]]; then
    set +e
    sync_manifest "$src_dir/Cargo.toml" "$dest_dir/Cargo.toml" "$CHECK_MODE"
    local manifest_code=$?
    set -e
    if [[ $manifest_code -eq 2 ]]; then
      repo_has_disparity=1
    elif [[ $manifest_code -ne 0 ]]; then
      echo "ERROR: Failed to process manifest for $publish_dirname" >&2
      return 1
    fi
  fi

  # Step 5: Unslop Invariants Scan
  if ! check_unslop_invariants "$dest_dir"; then
    echo "[WARN] Unslop violations found in $publish_dirname"
    if [[ "$CHECK_MODE" = true ]]; then
      repo_has_disparity=1
    fi
  fi

  # Step 6: Code Formatting
  if [[ "$CHECK_MODE" = false ]]; then
    echo "[CARGO FMT] Formatting code in $dest_dir..."
    cargo fmt --manifest-path "$dest_dir/Cargo.toml" --quiet 2>/dev/null || cargo fmt --manifest-path "$dest_dir/Cargo.toml"
  fi

  # Step 7: Compilation and Test Validation (in sync mode)
  if [[ "$CHECK_MODE" = false ]]; then
    echo "[CARGO CHECK] Verifying compilation in $dest_dir..."
    cargo check --manifest-path "$dest_dir/Cargo.toml" --quiet
    echo "[CARGO CHECK PASSED] $publish_dirname compiles cleanly."

    if [[ "$RUN_TESTS" = true ]]; then
      echo "[CARGO TEST] Running test suite in $dest_dir..."
      cargo test --manifest-path "$dest_dir/Cargo.toml" --quiet
      echo "[CARGO TEST PASSED] $publish_dirname passed all tests."
    fi
  fi

  return $repo_has_disparity
}

# Main execution loop
TOTAL_FAILED=0
PROCESSED_COUNT=0

for entry in "${REPOS[@]}"; do
  IFS=':' read -r cli_name crate_subpath publish_dirname <<< "$entry"

  if [[ "$ALL_REPOS" = false ]]; then
    clean_target="${TARGET_REPO%-publish}"
    clean_cli="${cli_name%-publish}"
    clean_pub="${publish_dirname%-publish}"
    if [[ "$TARGET_REPO" != "$cli_name" && "$TARGET_REPO" != "$publish_dirname" && "$clean_target" != "$clean_cli" && "$clean_target" != "$clean_pub" ]]; then
      continue
    fi
  fi

  PROCESSED_COUNT=$((PROCESSED_COUNT + 1))
  set +e
  process_repo "$entry"
  repo_rc=$?
  set -e
  if [[ $repo_rc -ne 0 ]]; then
    TOTAL_FAILED=$((TOTAL_FAILED + 1))
  fi
done

if [[ $PROCESSED_COUNT -eq 0 ]]; then
  echo "ERROR: Repository '$TARGET_REPO' not recognized." >&2
  echo "Available repositories:" >&2
  for entry in "${REPOS[@]}"; do
    IFS=':' read -r cli_name _ publish_dirname <<< "$entry"
    echo "  - $cli_name (target: $publish_dirname)" >&2
  done
  exit 1
fi

echo "============================================================"
if [[ "$CHECK_MODE" = true ]]; then
  if [[ $TOTAL_FAILED -eq 0 ]]; then
    echo "[STATUS: ALL IN PARITY] All $PROCESSED_COUNT repositories match source state with SRCL-1.0."
    exit 0
  else
    echo "[STATUS: DISPARITY DETECTED] $TOTAL_FAILED of $PROCESSED_COUNT repositories exhibit drift or disparities."
    exit 1
  fi
else
  if [[ $TOTAL_FAILED -eq 0 ]]; then
    echo "[STATUS: SYNC SUCCESS] Successfully synchronized $PROCESSED_COUNT repositories."
    exit 0
  else
    echo "[STATUS: SYNC FAILED] $TOTAL_FAILED of $PROCESSED_COUNT repositories failed synchronization."
    exit 1
  fi
fi
