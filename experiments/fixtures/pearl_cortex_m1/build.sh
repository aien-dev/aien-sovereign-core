#!/bin/sh
# Rebuild the PEARL Cortex M1 synthetic snapshot and eval fixture.
#
#   experiments/fixtures/pearl_cortex_m1/build.sh [OUT_DIR]
#
# Needs a C compiler and the sqlite3 command-line tool. The schema is taken
# verbatim from the migration SQL in crates/cortex-rs/src/db.rs, so the file
# opens at user_version 5 and Database::open runs no migration on it.
# Writes OUT_DIR/snapshot.sqlite and OUT_DIR/fixture.json (default: this folder).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
out=${1:-$here}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cc -O2 -std=c11 -D_POSIX_C_SOURCE=200809L -Wall -Wextra -Werror \
    -o "$work/gen_m1" "$here/gen_m1.c" -lm

# Migration SQL: every execute_batch string inside Database::run_migrations.
awk '
    /fn run_migrations\(/ { inside = 1; next }
    inside && /^    (pub )?fn / { exit }
    inside && /"BEGIN;/ { grab = 1 }
    grab { print }
    grab && /COMMIT;"/ { grab = 0 }
' "$root/crates/cortex-rs/src/db.rs" |
    sed -e 's/^[[:space:]]*"BEGIN;/BEGIN;/' -e 's/COMMIT;",[[:space:]]*$/COMMIT;/' \
        > "$work/schema.sql"
test "$(grep -c 'PRAGMA user_version' "$work/schema.sql")" -eq 5

"$work/gen_m1" "$work/data.sql" "$work/fixture.json"

db="$work/snapshot.sqlite"
sqlite3 "$db" < "$work/schema.sql"
sqlite3 "$db" < "$work/data.sql"
sqlite3 "$db" "PRAGMA journal_mode = DELETE; VACUUM;" > /dev/null
test "$(sqlite3 "$db" 'PRAGMA user_version;')" -eq 5
test ! -e "$db-wal" && test ! -e "$db-shm"

mkdir -p "$out"
cp "$db" "$out/snapshot.sqlite"
cp "$work/fixture.json" "$out/fixture.json"
echo "wrote $out/snapshot.sqlite and $out/fixture.json"
