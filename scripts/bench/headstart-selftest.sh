#!/usr/bin/env bash
# Self-test for the Headstart M0 runner and summarizer (#532). Bash + jq only.
# Runs the summarizer on synthetic data, then the full runner end to end on a
# tiny throwaway Cargo workspace (never on the real repo), plus negative cases
# that must fail: corrupt samples, missing UNIT_DATA, a build that does not
# compile, a missing edit anchor and a build that dirties the source tree.
#
#   bash scripts/bench/headstart-selftest.sh      (exit 0 = all checks passed)
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RUNNER="$HERE/headstart-aien.sh"
SUM="$HERE/headstart-summarize.sh"
TOOLCHAIN_FILE="$HERE/../../rust-toolchain.toml"
T=$(mktemp -d "${TMPDIR:-/tmp}/headstart-selftest.XXXXXX")
if [ "${HS_SELFTEST_KEEP:-0}" = 1 ]; then echo "keeping $T"; else trap 'rm -rf "$T"' EXIT; fi
PASS=0; FAIL=0

check() { # check <name> <command...>
  local name=$1; shift
  if "$@" >/dev/null 2>&1; then PASS=$((PASS + 1)); echo "ok   $name"
  else FAIL=$((FAIL + 1)); echo "FAIL $name"; fi
}
jqt() { jq -e "$1" "$2"; } # jq truthiness test on a file
jqs() { jq -e -s "$1" "$2"; } # same, on a JSONL file slurped into an array

# ------------------------------------------------ 1. summarizer, synthetic
S="$T/samples.jsonl"
mk() { # mk scenario arm wall rc final contaminated aa
  jq -cn --arg s "$1" --arg a "$2" --argjson w "$3" --argjson rc "$4" --argjson f "$5" \
    --argjson c "$6" --arg aa "$7" \
    '{scenario: $s, arm: $a, kind: "cold", command: "cargo x", sample_id: ($s + "-" + $a + "-" + ($w|tostring)),
      wall_s: $w, exit_status: $rc, ok: ($rc == 0), final: $f, contaminated: $c,
      aa_pair: (if $aa == "-" then null else $aa end),
      time_v: {available: true, max_rss_kb: 1000, cpu_percent: 500}}' >> "$S"
}
mk s1 A 10 0 true false -
mk s1 A 12 0 true false -
mk s1 A 11 0 true true -
mk s1 A 5 101 true false -
mk s1 A 99 0 false true -
mk aa A 10 0 true false aa
mk aa A 20 0 true false aa
mk aa B 12 0 true false aa
mk aa B 22 0 true false aa
"$SUM" samples "$S" > "$T/summary.json"
G='.groups[] | select(.scenario == "s1")'
check "summary median excludes failed and superseded" jqt "$G | .wall_s.median == 11 and .wall_s.n == 3" "$T/summary.json"
check "summary min/max/mad/stdev" jqt "$G | .wall_s.min == 10 and .wall_s.max == 12 and .wall_s.mad == 1 and .wall_s.stdev == 1" "$T/summary.json"
check "summary counts failures with exit status" jqt "$G | .n_failed == 1 and .failures[0].exit_status == 101" "$T/summary.json"
check "summary clean subset drops contaminated" jqt "$G | .wall_s_clean.n == 2 and .wall_s_clean.median == 11" "$T/summary.json"
check "summary superseded count" jqt "$G | .n_superseded == 1" "$T/summary.json"
check "A/A delta" jqt '.aa_noise[0] | .median_a_s == 15 and .median_b_s == 17 and .delta_s == 2 and .delta_pct == 13.333' "$T/summary.json"
"$SUM" samples "$S" > "$T/summary2.json"
check "summary is byte-for-byte reproducible" cmp "$T/summary.json" "$T/summary2.json"
cp "$S" "$T/bad.jsonl"; echo '{"scenario": broken' >> "$T/bad.jsonl"
check "NEGATIVE: corrupt samples line is rejected" bash -c "! '$SUM' samples '$T/bad.jsonl'"
: > "$T/empty.jsonl"
check "NEGATIVE: empty samples file is rejected" bash -c "! '$SUM' samples '$T/empty.jsonl'"

# ------------------------------------------------ 2. timings + critical path
cat > "$T/timing.html" <<'EOF'
<html><script>
DURATION = 6;
const UNIT_DATA = [
  {"i": 0, "name": "a", "version": "0.1.0", "mode": "todo", "target": "", "features": [], "start": 0.0, "duration": 4.0,
   "unblocked_units": [], "unblocked_rmeta_units": [1], "sections": [["frontend", {"start": 0.0, "end": 1.0}], ["codegen", {"start": 1.0, "end": 4.0}]]},
  {"i": 1, "name": "b", "version": "0.1.0", "mode": "todo", "target": "", "features": [], "start": 1.1, "duration": 3.0,
   "unblocked_units": [2], "unblocked_rmeta_units": [], "sections": [["frontend", {"start": 0.0, "end": 2.0}], ["codegen", {"start": 2.0, "end": 3.0}]]},
  {"i": 2, "name": "c", "version": "0.1.0", "mode": "todo", "target": " c \"bin\"", "features": [], "start": 4.2, "duration": 1.0,
   "unblocked_units": [], "unblocked_rmeta_units": [], "sections": null}
];
const CONCURRENCY_DATA = [];
</script></html>
EOF
"$SUM" timings "$T/timing.html" > "$T/units.json"
check "timings extracts UNIT_DATA" jqt 'length == 3 and .[2].name == "c"' "$T/units.json"
"$SUM" critical "$T/units.json" 2 > "$T/crit.json"
check "critical chain a -> b -> c" jqt '[.observed.chain[].name] == ["a","b","c"]' "$T/crit.json"
check "critical gates rmeta then full" jqt '[.observed.chain[1:][].gated_by.gate] == ["rmeta","full"] and .observed.chain[1].gate_time_s == 1 and .observed.chain[2].gate_time_s == 4.1' "$T/crit.json"
check "estimate bounds" jqt '.estimate.floor_s == 4 and .estimate.rmeta_wait_on_chain_s == 1 and .estimate.upper_bound_saving_s == 1 and .estimate.lower_bound_saving_s == 0' "$T/crit.json"
printf '<html>no data</html>\n' > "$T/nodata.html"
check "NEGATIVE: report without UNIT_DATA is rejected" bash -c "! '$SUM' timings '$T/nodata.html'"

# ------------------------------------------------ 3. runner end to end on a fixture
mkfixture() { # mkfixture <dir> [broken_runtime] [dirty_buildrs]
  local d=$1 broken=${2:-no} dirty=${3:-no}
  mkdir -p "$d/crates"/{hsbase,spark-harness,spark-supervisor,aien-runtime}/src
  printf '[workspace]\nresolver = "2"\nmembers = ["crates/hsbase", "crates/spark-harness", "crates/spark-supervisor", "crates/aien-runtime"]\n' > "$d/Cargo.toml"
  [ -f "$TOOLCHAIN_FILE" ] && cp "$TOOLCHAIN_FILE" "$d/rust-toolchain.toml"
  pkg() { printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n%s\n' "$1" "$2"; }
  pkg hsbase "$([ "$dirty" = yes ] && echo 'build = "build.rs"')" > "$d/crates/hsbase/Cargo.toml"
  printf 'pub fn core_value() -> u64 {\n    7\n}\n' > "$d/crates/hsbase/src/lib.rs"
  if [ "$dirty" = yes ]; then
    printf 'fn main() {\n    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs");\n    let mut s = std::fs::read_to_string(p).unwrap();\n    s.push_str("\\n// dirtied by build script\\n");\n    std::fs::write(p, s).unwrap();\n}\n' > "$d/crates/hsbase/build.rs"
  fi
  pkg spark-harness '[dependencies]
hsbase = { path = "../hsbase" }' > "$d/crates/spark-harness/Cargo.toml"
  printf 'pub fn harness() -> u64 {\n    hsbase::core_value() + 1\n}\n' > "$d/crates/spark-harness/src/lib.rs"
  pkg spark-supervisor '[dependencies]
hsbase = { path = "../hsbase" }' > "$d/crates/spark-supervisor/Cargo.toml"
  printf 'fn main() {\n    println!("{}", hsbase::core_value());\n}\n' > "$d/crates/spark-supervisor/src/main.rs"
  pkg aien-runtime '[dependencies]
spark-harness = { path = "../spark-harness" }' > "$d/crates/aien-runtime/Cargo.toml"
  if [ "$broken" = yes ]; then
    printf 'pub fn runtime() -> u64 {\n    spark_harness::harness() + "not a number"\n}\n' > "$d/crates/aien-runtime/src/lib.rs"
  else
    printf 'pub fn runtime() -> u64 {\n    spark_harness::harness() * 2\n}\n' > "$d/crates/aien-runtime/src/lib.rs"
  fi
  (cd "$d" && git init -q && cargo generate-lockfile -q --offline && git add -A \
     && git -c user.name=selftest -c user.email=selftest@invalid commit -q -m fixture)
}

ANCHOR_DEFAULT='pub fn core_value() -> u64 {'
runner() { # runner <fixture> <tag> [stages...]
  local fx=$1 tag=$2; shift 2
  HS_REPO="$fx" HS_COMMIT=HEAD HS_WORK="$T/work-$tag" HS_OUT="$T/out-$tag" HS_DATE=selftest \
  HS_SAMPLES=1 HS_LOAD_MAX=100000 HS_WAIT_CAP_S=0 HS_MAX_RERUNS=0 HS_MON_INTERVAL_S=1 \
  HS_INCR_STEPS=2 HS_INCR_SERIES=1 HS_BODY_FILE=crates/hsbase/src/lib.rs \
  HS_BODY_ANCHOR="${HS_TEST_ANCHOR:-$ANCHOR_DEFAULT}" HS_IFACE_FILE=crates/hsbase/src/lib.rs \
  HS_EDIT_REASON=selftest bash "$RUNNER" "$@" >> "$T/runner-$tag.log" 2>&1
}

FX="$T/fixture"; mkfixture "$FX"
runner "$FX" good setup timings cold incr cleanup
RC=$?
O="$T/out-good"
check "runner full matrix exits 0" test "$RC" -eq 0
check "runner wrote 18 samples, all ok" jqs 'length == 18 and all(.[]; .ok)' "$O/samples.jsonl"
check "runner manifest has commit, rustc and scenarios" jqt '(.source.commit | length) == 40 and (.toolchain.rustc_vV | test("rustc ")) and (.scenarios | length) >= 11 and .build.registry_cache.state == "warm"' "$O/manifest.json"
check "runner records quiet-flag and load waits per sample" jqs 'all(.[]; (.quiet_flag_wait_s | type) == "number" and (.load_wait_s | type) == "number" and (.loadavg_before | length) == 3)' "$O/samples.jsonl"
check "runner summary reproducible from samples" bash -c "'$SUM' samples '$O/samples.jsonl' | cmp - '$O/summary.json'"
check "runner A/A arms present" jqt '.aa_noise[0].n_a == 1 and .aa_noise[0].n_b == 1' "$O/summary.json"
check "runner critical path extracted" jqt '.observed.chain_length >= 2' "$O/timings/critical-path.json"
check "body edits recompiled the edited crate" grep -q 'Checking hsbase' "$O/logs/incr_body_check-A-r1-s1-a1.tail.log"
check "iface edits recorded the edit" jqs 'map(select(.scenario == "incr_iface_check")) | length == 2 and all(.[]; .edit | test("pub fn headstart_m0_probe_"))' "$O/samples.jsonl"
check "caller fixture checkout untouched" git -C "$FX" diff --quiet
check "benchmark worktree removed" bash -c "[ \$(git -C '$FX' worktree list | wc -l) -eq 1 ] && [ ! -e '$T/work-good' ]"

HS_TEST_ANCHOR='no such anchor line' runner "$FX" anchor setup incr
RC=$?
check "NEGATIVE: missing edit anchor aborts" test "$RC" -ne 0
check "NEGATIVE: abort message names the anchor" grep -q 'body anchor must match exactly one line' "$T/runner-anchor.log"
check "NEGATIVE: aborted run left worktree clean" git -C "$T/work-anchor/src" diff --quiet
runner "$FX" anchor cleanup

FXB="$T/fixture-broken"; mkfixture "$FXB" yes
runner "$FXB" broken setup cold cleanup
RC=$?
check "broken build: runner still exits 0 (failure recorded, not hidden)" test "$RC" -eq 0
check "NEGATIVE: failed build recorded with exit status" jqs 'map(select(.scenario == "cold_build_aien_runtime")) | length == 1 and .[0].ok == false and .[0].exit_status != 0' "$T/out-broken/samples.jsonl"
check "NEGATIVE: failed build never counted in median" jqt '.groups[] | select(.scenario == "cold_build_aien_runtime") | .n_failed == 1 and .wall_s.n == 0 and .wall_s.median == null' "$T/out-broken/summary.json"
check "broken build: unaffected crate still measured" jqt '.groups[] | select(.scenario == "cold_build_spark_harness") | .wall_s.n == 1' "$T/out-broken/summary.json"

FXD="$T/fixture-dirty"; mkfixture "$FXD" no yes
runner "$FXD" dirty setup timings
RC=$?
check "NEGATIVE: build that dirties the worktree aborts the run" test "$RC" -ne 0
check "NEGATIVE: abort message names the dirty tree" grep -q 'not clean' "$T/runner-dirty.log"
git -C "$T/work-dirty/src" checkout -q -- . 2>/dev/null
runner "$FXD" dirty cleanup

echo "selftest: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
