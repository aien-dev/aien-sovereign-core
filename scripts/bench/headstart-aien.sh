#!/usr/bin/env bash
# Headstart M0 (#532): reproducible stable-toolchain build baselines for the
# AIEN monorepo. Measurement only: this script never edits Cargo.toml,
# rust-toolchain.toml, CI, compiler flags or the caller's checkout.
#
# Usage:
#   scripts/bench/headstart-aien.sh [stage ...]
# Stages (default: setup timings cold incr cleanup):
#   setup    create a detached git worktree of HS_COMMIT under HS_WORK, warm
#            the cargo registry (cargo fetch --locked), write manifest.json
#   timings  one cold `cargo build --workspace --timings`, extract unit data
#            and the observed critical path (timings/)
#   cold     interleaved rounds of the cold scenarios (A/A on cold check)
#   incr     10-step body-only and interface-changing edit series on
#            `cargo check --workspace --all-targets`, reverted and verified
#   summary  rebuild summary.json from samples.jsonl
#   cleanup  remove the benchmark worktree and its target directory
#
# Environment (defaults in brackets): HS_COMMIT [HEAD of this checkout],
# HS_WORK [$HOME/workspace/headstart-m0-<short>], HS_OUT
# [benchmarks/data/headstart/baseline-<date>-<short>], HS_SAMPLES [5],
# HS_LOAD_MAX [2], HS_WAIT_CAP_S [600], HS_MON_INTERVAL_S [30],
# HS_MAX_RERUNS [2], HS_SLOW_COLD_S [1200], HS_INCR_STEPS [10],
# HS_INCR_SERIES [2], HS_BODY_FILE, HS_BODY_ANCHOR, HS_IFACE_FILE,
# HS_EDIT_REASON. See benchmarks/headstart/README.md.
set -euo pipefail

SELF_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=${HS_REPO:-$(git -C "$SELF_DIR" rev-parse --show-toplevel)}
SUMMARIZE="$SELF_DIR/headstart-summarize.sh"

HS_COMMIT=$(git -C "$REPO" rev-parse "${HS_COMMIT:-HEAD}")
SHORT=${HS_COMMIT:0:7}
HS_WORK=${HS_WORK:-$HOME/workspace/headstart-m0-$SHORT}
HS_DATE=${HS_DATE:-$(date -u +%Y%m%d)}
HS_OUT=${HS_OUT:-$REPO/benchmarks/data/headstart/baseline-$HS_DATE-$SHORT}
HS_SAMPLES=${HS_SAMPLES:-5}
HS_LOAD_MAX=${HS_LOAD_MAX:-2}
HS_WAIT_CAP_S=${HS_WAIT_CAP_S:-600}
HS_MON_INTERVAL_S=${HS_MON_INTERVAL_S:-30}
HS_MAX_RERUNS=${HS_MAX_RERUNS:-2}
HS_SLOW_COLD_S=${HS_SLOW_COLD_S:-1200}
HS_INCR_STEPS=${HS_INCR_STEPS:-10}
HS_INCR_SERIES=${HS_INCR_SERIES:-2}
HS_QUIETLOCK=${HS_QUIETLOCK:-1}
# Edit targets (repo-relative paths). The anchor is a fixed string that must
# match exactly one line, the opening line of a function body.
HS_BODY_FILE=${HS_BODY_FILE:-}
HS_BODY_ANCHOR=${HS_BODY_ANCHOR:-}
HS_IFACE_FILE=${HS_IFACE_FILE:-}
HS_EDIT_REASON=${HS_EDIT_REASON:-}

SRC="$HS_WORK/src"
TARGET="$HS_WORK/target"
RAWLOG="$HS_WORK/logs"
SAMPLES="$HS_OUT/samples.jsonl"
MANIFEST="$HS_OUT/manifest.json"
RUN_ID="headstart-m0-$HS_DATE-$SHORT"
TIME_BIN=/usr/bin/time
MON_PID=""
EDITED_FILE=""

say() { printf '[headstart %s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
die() { say "ABORT: $*"; exit 1; }

cleanup_on_exit() {
  local rc=$?
  [ -n "$MON_PID" ] && kill "$MON_PID" 2>/dev/null || true
  if [ -n "$EDITED_FILE" ] && [ -d "$SRC" ]; then
    git -C "$SRC" checkout -- "$EDITED_FILE" 2>/dev/null || true
    say "reverted in-flight edit of $EDITED_FILE"
  fi
  exit "$rc"
}
trap cleanup_on_exit EXIT
trap 'exit 130' INT TERM

need() { command -v "$1" >/dev/null 2>&1 || die "missing required tool: $1"; }
need git; need jq; need awk; need sed; need cargo; need pgrep

now_ns() { date +%s%N; }
secs_between() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", (b - a) / 1e9 }'; }
loadavg_json() { awk '{ printf "[%s,%s,%s]", $1, $2, $3 }' /proc/loadavg; }
load1() { awk '{ print $1 }' /proc/loadavg; }
mem_avail_kb() { awk '/^MemAvailable:/ { print $2 }' /proc/meminfo; }

# PIDs of this runner and every descendant (our own cargo/rustc).
own_pids() {
  ps -e -o pid=,ppid= | awk -v root="$$" '
    { parent[$1] = $2; pids[NR] = $1 }
    END {
      own[root] = 1; changed = 1
      while (changed) { changed = 0
        for (i in pids) { p = pids[i]; if (!(p in own) && (parent[p] in own)) { own[p] = 1; changed = 1 } } }
      for (p in own) print p
    }'
}

# Foreign rustc/cargo/clippy-driver/rustdoc processes as a JSON array of
# "pid:comm:crate" strings. Excludes our process tree and anything whose
# command line mentions our work directory.
foreign_json() {
  local own line pid comm crate out=()
  own=" $(own_pids | tr '\n' ' ') "
  while read -r pid; do
    [ -n "$pid" ] || continue
    case "$own" in *" $pid "*) continue ;; esac
    comm=$(cat "/proc/$pid/comm" 2>/dev/null || true)
    [ -n "$comm" ] || continue
    line=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)
    case "$line" in *"$HS_WORK"*) continue ;; esac
    crate=$(printf '%s' "$line" | sed -n 's/.*--crate-name \([^ ]*\).*/\1/p')
    # A compiler process counts only when it compiles a crate; this skips
    # idle probes such as an orphaned `rustc --version`.
    [ "$comm" != cargo ] && [ -z "$crate" ] && continue
    out+=("$pid:$comm:${crate:--}")
  done < <(pgrep -x 'rustc|cargo|clippy-driver|rustdoc' || true)
  if [ ${#out[@]} -eq 0 ]; then echo '[]'; else printf '%s\n' "${out[@]}" | head -40 | jq -R . | jq -cs .; fi
}

# Respect the host's shared quiet flag (`quietlock check` exits non-zero while
# another job holds it for a quiet measurement). No cap: the holder publishes
# an expected end. Prints the seconds waited.
wait_quietlock() {
  local start
  start=$(date +%s)
  if [ "$HS_QUIETLOCK" = 1 ] && command -v quietlock >/dev/null 2>&1; then
    while ! quietlock check >/dev/null 2>&1; do sleep 10; done
  fi
  echo "$(( $(date +%s) - start ))"
}

# Wait until 1-minute loadavg < HS_LOAD_MAX, at most HS_WAIT_CAP_S seconds.
# Prints "<waited_s> <capped:true|false>".
wait_quiet() {
  local start waited
  start=$(date +%s)
  while awk -v l="$(load1)" -v m="$HS_LOAD_MAX" 'BEGIN { exit !(l >= m) }'; do
    waited=$(( $(date +%s) - start ))
    if [ "$waited" -ge "$HS_WAIT_CAP_S" ]; then echo "$waited true"; return; fi
    sleep 5
  done
  echo "$(( $(date +%s) - start )) false"
}

# Parse a /usr/bin/time -v report into JSON.
time_v_json() {
  local f=$1
  if [ ! -s "$f" ]; then echo '{"available":false,"reason":"no /usr/bin/time report"}'; return; fi
  awk -F': ' '
    /Maximum resident set size/ { rss = $2 }
    /Percent of CPU this job got/ { cpu = $2; sub(/%/, "", cpu) }
    /User time \(seconds\)/ { usr = $2 }
    /System time \(seconds\)/ { sys = $2 }
    /Elapsed \(wall clock\) time/ { el = $NF }
    /Exit status/ { ex = $2 }
    END {
      if (cpu == "?" || cpu == "") cpu = "null"
      printf "{\"available\":true,\"max_rss_kb\":%s,\"cpu_percent\":%s,\"user_s\":%s,\"sys_s\":%s,\"elapsed\":\"%s\",\"exit_status\":%s,\"max_rss_scope\":\"ru_maxrss of the waited cargo tree: the largest single process, not a sum (getrusage(2))\"}",
        (rss == "" ? "null" : rss), cpu, (usr == "" ? "null" : usr), (sys == "" ? "null" : sys), el, (ex == "" ? "null" : ex)
    }' "$f"
}

ensure_clean() {
  local where=$1
  git -C "$SRC" diff --quiet || die "worktree not clean after $where: $(git -C "$SRC" diff --stat | tail -1)"
  git -C "$SRC" diff --cached --quiet || die "worktree index not clean after $where"
}

# run_sample <scenario> <arm> <kind> <round> <step> <aa_pair|-> <prep> -- cargo args...
# prep: "cold" removes the target dir before each attempt, "warm" keeps it.
run_sample() {
  local scenario=$1 arm=$2 kind=$3 round=$4 step=$5 aa=$6 prep=$7
  shift 8
  local cmd=("$@") cmdline attempt=0 final
  cmdline="${cmd[*]}"
  while :; do
    attempt=$((attempt + 1))
    local sid="${scenario}-${arm}-r${round}-s${step}-a${attempt}"
    [ "$prep" = cold ] && rm -rf "$TARGET"
    local wq waited capped lb la fstart mon t0 t1 rc wall tv tf log tail_rel contaminated maxseen ticks memb qwait
    qwait=$(wait_quietlock)
    wq=$(wait_quiet); waited=${wq% *}; capped=${wq#* }
    lb=$(loadavg_json); memb=$(mem_avail_kb)
    fstart=$(foreign_json)
    mon="$RAWLOG/$sid.mon"; : > "$mon"
    ( while sleep "$HS_MON_INTERVAL_S"; do foreign_json >> "$mon"; done ) &
    MON_PID=$!
    tf="$RAWLOG/$sid.time"; log="$RAWLOG/$sid.log"; rm -f "$tf"
    say "sample $sid: ${cmdline} (waited ${waited}s, load $lb)"
    t0=$(now_ns)
    set +e
    if [ -x "$TIME_BIN" ]; then
      (cd "$SRC" && CARGO_TARGET_DIR="$TARGET" "$TIME_BIN" -v -o "$tf" "${cmd[@]}") > "$log" 2>&1
    else
      (cd "$SRC" && CARGO_TARGET_DIR="$TARGET" "${cmd[@]}") > "$log" 2>&1
    fi
    rc=$?
    set -e
    t1=$(now_ns)
    kill "$MON_PID" 2>/dev/null || true; wait "$MON_PID" 2>/dev/null || true; MON_PID=""
    la=$(loadavg_json)
    wall=$(secs_between "$t0" "$t1")
    if [ -x "$TIME_BIN" ]; then tv=$(time_v_json "$tf"); else tv='{"available":false,"reason":"/usr/bin/time not found"}'; fi
    ticks=$(wc -l < "$mon")
    maxseen=$( { echo "$fstart"; cat "$mon"; } | jq -s 'map(length) | max')
    contaminated=false; [ "$maxseen" -gt 0 ] && contaminated=true
    tail_rel="logs/$sid.tail.log"
    local nt=30; [ "$rc" -ne 0 ] && nt=80
    tail -n "$nt" "$log" | LC_ALL=C tr -cd '\11\12\15\40-\176' > "$HS_OUT/$tail_rel"
    final=true
    if [ "$contaminated" = true ] && [ "$attempt" -le "$HS_MAX_RERUNS" ]; then final=false; fi
    jq -cn \
      --arg run_id "$RUN_ID" --arg sid "$sid" --arg scenario "$scenario" --arg arm "$arm" \
      --arg kind "$kind" --argjson round "$round" --argjson step "$step" --argjson attempt "$attempt" \
      --arg aa "$aa" --arg prep "$prep" --arg cmdline "$cmdline" --arg cwd "$SRC" --arg target "$TARGET" \
      --arg started "$(date -u -d "@$((t0 / 1000000000))" +%Y-%m-%dT%H:%M:%SZ)" \
      --argjson wall "$wall" --argjson tv "$tv" --argjson rc "$rc" \
      --argjson lb "$lb" --argjson la "$la" --argjson waited "$waited" --argjson capped "$capped" \
      --argjson qwait "$qwait" --argjson memb "${memb:-null}" --argjson fstart "$fstart" --argjson maxseen "$maxseen" \
      --argjson ticks "$ticks" --argjson contaminated "$contaminated" --argjson final "$final" \
      --arg tail "$tail_rel" --arg edit "${HS_CUR_EDIT:-}" \
      --slurpfile mon <(cat "$mon") \
      '{schema: "headstart-sample/v1", run_id: $run_id, sample_id: $sid, scenario: $scenario, arm: $arm,
        kind: $kind, round: $round, step: $step, attempt: $attempt,
        aa_pair: (if $aa == "-" then null else $aa end), prep: $prep, edit: (if $edit == "" then null else $edit end),
        command: $cmdline, cwd: $cwd, cargo_target_dir: $target, started_at: $started,
        wall_s: $wall, time_v: $tv, exit_status: $rc, ok: ($rc == 0),
        loadavg_before: $lb, loadavg_after: $la, quiet_flag_wait_s: $qwait,
        load_wait_s: $waited, load_wait_capped: $capped,
        mem_available_kb_before: $memb,
        foreign: {at_start: $fstart, monitor_ticks: $ticks, max_seen: $maxseen,
                  seen: ([$fstart[], ($mon[] | .[])] | unique | .[0:40])},
        contaminated: $contaminated, final: $final, log_tail: $tail}' >> "$SAMPLES"
    rm -f "$log"
    say "  -> rc=$rc wall=${wall}s contaminated=$contaminated final=$final"
    LAST_WALL=$wall
    LAST_RC=$rc
    [ "$final" = true ] && break
  done
}

# ---------------------------------------------------------------- stages

stage_setup() {
  say "setup: commit $HS_COMMIT, work $HS_WORK, out $HS_OUT"
  [ -x "$TIME_BIN" ] || say "WARNING: $TIME_BIN not found; max RSS and CPU% will be recorded as unavailable"
  mkdir -p "$HS_WORK" "$RAWLOG" "$HS_OUT/logs" "$HS_OUT/timings"
  case "$HS_WORK/" in "$REPO/"*) die "HS_WORK must be outside the checkout ($REPO)";; esac
  if [ ! -d "$SRC/.git" ] && [ ! -f "$SRC/.git" ]; then
    git -C "$REPO" worktree add --detach "$SRC" "$HS_COMMIT" >&2
  fi
  [ "$(git -C "$SRC" rev-parse HEAD)" = "$HS_COMMIT" ] || die "worktree HEAD is not $HS_COMMIT"
  ensure_clean setup
  local df_line t0 t1 frc
  df_line=$(df -h "$HS_WORK" | tail -1)
  say "disk: $df_line"
  t0=$(now_ns)
  set +e
  (cd "$SRC" && cargo fetch --locked) > "$RAWLOG/fetch.log" 2>&1
  frc=$?
  set -e
  t1=$(now_ns)
  [ "$frc" -eq 0 ] || die "cargo fetch --locked failed (rc=$frc), see $RAWLOG/fetch.log"
  local rustc_vv cargo_v cpu_models cfgs
  rustc_vv=$(cd "$SRC" && rustc -vV)
  cargo_v=$(cd "$SRC" && cargo -V)
  cpu_models=$(lscpu 2>/dev/null | awk -F': *' '/^Model name/ { print $2 }' | jq -R . | jq -cs .)
  cfgs=$(for f in "$HOME/.cargo/config.toml" "$HOME/.cargo/config" "$SRC/.cargo/config.toml" "$SRC/.cargo/config"; do
           if [ -e "$f" ]; then echo "$f"; fi; done | jq -R . | jq -cs .)
  jq -n \
    --arg run_id "$RUN_ID" --arg commit "$HS_COMMIT" --arg short "$SHORT" \
    --arg origin "$(git -C "$REPO" remote get-url origin 2>/dev/null || echo unknown)" \
    --arg src "$SRC" --arg target "$TARGET" --arg rustc_vv "$rustc_vv" --arg cargo_v "$cargo_v" \
    --arg toolchain_file "$(cat "$SRC/rust-toolchain.toml" 2>/dev/null || echo '<absent>')" \
    --arg arch "$(uname -m)" --arg kernel "$(uname -r)" --argjson nproc "$(nproc)" \
    --argjson memtotal "$(awk '/^MemTotal:/ { print $2 }' /proc/meminfo)" --argjson memavail "$(mem_avail_kb)" \
    --argjson cpu_models "$cpu_models" --argjson cfgs "$cfgs" --arg df "$df_line" \
    --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson fetch_s "$(secs_between "$t0" "$t1")" \
    --arg rustflags "${RUSTFLAGS-<unset>}" --arg wrapper "${RUSTC_WRAPPER-<unset>}" \
    --arg jobs_env "${CARGO_BUILD_JOBS-<unset>}" --arg incr_env "${CARGO_INCREMENTAL-<unset>}" \
    --argjson time_v "$([ -x "$TIME_BIN" ] && echo true || echo false)" \
    --argjson samples "$HS_SAMPLES" --argjson loadmax "$HS_LOAD_MAX" --argjson waitcap "$HS_WAIT_CAP_S" \
    --argjson mon "$HS_MON_INTERVAL_S" --argjson reruns "$HS_MAX_RERUNS" --argjson slow "$HS_SLOW_COLD_S" \
    '{schema: "headstart-manifest/v1", run_id: $run_id, created_at: $date,
      source: {commit: $commit, short: $short, origin: $origin, worktree: $src,
               worktree_kind: "git worktree add --detach (outside the caller checkout)"},
      toolchain: {rustc_vV: $rustc_vv, cargo_V: $cargo_v, rust_toolchain_toml: $toolchain_file,
                  cargo_config_files: $cfgs,
                  env: {RUSTFLAGS: $rustflags, RUSTC_WRAPPER: $wrapper, CARGO_BUILD_JOBS: $jobs_env, CARGO_INCREMENTAL: $incr_env}},
      host: {arch: $arch, kernel: $kernel, nproc: $nproc, cpu_models: $cpu_models,
             mem_total_kb: $memtotal, mem_available_kb_at_setup: $memavail, disk_at_setup: $df},
      build: {features: "default (no --features / --all-features / --no-default-features flags)",
              profiles: {check: "dev", build: "dev", "test --no-run": "test (inherits dev)"},
              target_dir: $target, jobs: "cargo default (-j not passed, CARGO_BUILD_JOBS as recorded) = nproc",
              cold_definition: "CARGO_TARGET_DIR removed before every cold attempt; ~/.cargo/registry and ~/.cargo/git kept warm",
              registry_cache: {state: "warm", how: "cargo fetch --locked ran once in setup before any sample", fetch_wall_s: $fetch_s}},
      protocol: {samples_per_scenario: $samples, load1_max: $loadmax, load_wait_cap_s: $waitcap,
                 foreign_monitor_interval_s: $mon, max_reruns_when_contaminated: $reruns,
                 slow_cold_threshold_s: $slow,
                 foreign_definition: "cargo processes, or rustc / clippy-driver / rustdoc processes with --crate-name (an actual compile), outside this runner process tree and whose command line does not mention the benchmark work dir",
                 contaminated_definition: "a foreign process was seen at sample start or at any monitor tick",
                 time_v_available: $time_v},
      scenarios: [], stages: []}' > "$MANIFEST"
  say "manifest written: $MANIFEST"
}

manifest_add() { # manifest_add <jq filter> [jq args...]
  local filter=$1; shift
  local tmp="$MANIFEST.tmp"
  jq "$@" "$filter" "$MANIFEST" > "$tmp" && mv "$tmp" "$MANIFEST"
}

stage_begin() {
  manifest_add '.stages += [{stage: $s, started_at: $t, script: "scripts/bench/headstart-aien.sh"}]' \
    --arg s "$1" --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
stage_end() {
  manifest_add '(.stages[] | select(.stage == $s and .ended_at == null)) |= . + {ended_at: $t}' \
    --arg s "$1" --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
scenario_add() { # scenario_add <id> <kind> <command>
  manifest_add 'if any(.scenarios[]; .id == $id) then . else .scenarios += [{id: $id, kind: $k, command: $c, cwd: "worktree root"}] end' \
    --arg id "$1" --arg k "$2" --arg c "$3"
}

require_setup() {
  [ -s "$MANIFEST" ] || die "no manifest at $MANIFEST; run the setup stage first"
  [ -d "$SRC" ] || die "no worktree at $SRC; run the setup stage first"
  [ "$(git -C "$SRC" rev-parse HEAD)" = "$HS_COMMIT" ] || die "worktree HEAD moved"
  mkdir -p "$RAWLOG" "$HS_OUT/logs" "$HS_OUT/timings"
  manifest_add '.protocol.quiet_flag //= {respected: ($q == "1"),
      how: "before each sample, wait (uncapped) while `quietlock check` reports the host quiet flag held by another job; seconds waited are in quiet_flag_wait_s",
      first_stage: $s}' --arg q "$HS_QUIETLOCK" --arg s "$CUR_STAGE"
}

stage_timings() {
  require_setup; stage_begin timings
  local c=(cargo build --workspace --timings)
  scenario_add timings_build_ws timings "${c[*]}"
  run_sample timings_build_ws A timings 1 0 - cold -- "${c[@]}"
  ensure_clean timings
  local html="$TARGET/cargo-timings/cargo-timing.html"
  if [ "$LAST_RC" -eq 0 ] && [ -s "$html" ]; then
    "$SUMMARIZE" timings "$html" > "$HS_OUT/timings/units.json"
    "$SUMMARIZE" critical "$HS_OUT/timings/units.json" "$(nproc)" > "$HS_OUT/timings/critical-path.json"
    sed -n 's#.*<td>Max concurrency:</td><td>\(.*\)</td>.*#\1#p; s#.*<td>Total time:</td><td>\(.*\)</td>.*#\1#p' "$html" \
      | jq -R . | jq -s '{max_concurrency: .[0], total_time: .[1], source: "cargo-timing.html summary table"}' \
      > "$HS_OUT/timings/report-header.json"
    gzip -9 -c "$html" > "$HS_OUT/timings/cargo-timing.html.gz"
    say "critical path: $(jq -r '[.observed.chain[].name] | join(" -> ")' "$HS_OUT/timings/critical-path.json")"
  else
    say "timings build failed (rc=$LAST_RC); no critical path extracted"
  fi
  stage_end timings
}

stage_cold() {
  require_setup; stage_begin cold
  local ids=(cold_check_ws cold_build_ws cold_test_norun_ws cold_build_spark_harness cold_build_spark_supervisor cold_build_aien_runtime)
  declare -A cmd=(
    [cold_check_ws]="cargo check --workspace --all-targets"
    [cold_build_ws]="cargo build --workspace"
    [cold_test_norun_ws]="cargo test --workspace --no-run"
    [cold_build_spark_harness]="cargo build -p spark-harness"
    [cold_build_spark_supervisor]="cargo build -p spark-supervisor"
    [cold_build_aien_runtime]="cargo build -p aien-runtime"
  )
  local id; for id in "${ids[@]}"; do scenario_add "$id" cold "${cmd[$id]}"; done
  local n=$HS_SAMPLES round=1
  while [ "$round" -le "$n" ]; do
    # A/A: cold_check_ws runs as arms A and B, first and middle of the round,
    # with the arm order swapped every round. Other scenarios sit between.
    local first=A second=B
    [ $((round % 2)) -eq 0 ] && { first=B; second=A; }
    local order=("cold_check_ws:$first" cold_build_ws cold_build_spark_harness cold_test_norun_ws "cold_check_ws:$second" cold_build_spark_supervisor cold_build_aien_runtime)
    local item sid arm words
    for item in "${order[@]}"; do
      sid=${item%%:*}; arm=A; aa=-
      if [ "$sid" = cold_check_ws ]; then arm=${item#*:}; aa=cold_check_ws; fi
      read -ra words <<< "${cmd[$sid]}"
      run_sample "$sid" "$arm" cold "$round" 0 "$aa" cold -- "${words[@]}"
      ensure_clean "$sid round $round"
      case "$sid" in
        *_ws)
          if [ "$round" -eq 1 ] && [ "$n" -gt 3 ] && [ "$LAST_RC" -eq 0 ] \
             && awk -v w="$LAST_WALL" -v s="$HS_SLOW_COLD_S" 'BEGIN { exit !(w > s) }'; then
            n=3
            say "cold workspace sample took ${LAST_WALL}s (> ${HS_SLOW_COLD_S}s): reducing to 3 rounds"
            manifest_add '.protocol.samples_reduced = {to: 3, reason: $r}' \
              --arg r "$sid took ${LAST_WALL}s in round 1, above slow_cold_threshold_s"
          fi ;;
      esac
    done
    "$SUMMARIZE" samples "$SAMPLES" > "$HS_OUT/summary.json"
    round=$((round + 1))
  done
  manifest_add '.protocol.cold_rounds_run = $n' --argjson n "$n"
  stage_end cold
}

apply_edit() { # apply_edit body|iface <k>
  local kind=$1 k=$2 f n
  if [ "$kind" = body ]; then
    f=$HS_BODY_FILE
    git -C "$SRC" checkout -- "$f"
    n=$(grep -nF -- "$HS_BODY_ANCHOR" "$SRC/$f" | cut -d: -f1 || true)
    [ "$(printf '%s\n' "$n" | grep -c .)" -eq 1 ] || die "body anchor must match exactly one line in $f"
    sed -i "${n}a\\    let _headstart_m0_probe = std::hint::black_box(${k}u64);" "$SRC/$f"
    HS_CUR_EDIT="body: $f line $((n + 1)) inserted 'let _headstart_m0_probe = std::hint::black_box(${k}u64);'"
  else
    f=$HS_IFACE_FILE
    git -C "$SRC" checkout -- "$f"
    printf '\n/// Headstart M0 benchmark probe (never committed).\npub fn headstart_m0_probe_%s() -> u64 {\n    %s\n}\n' "$k" "$k" >> "$SRC/$f"
    HS_CUR_EDIT="iface: $f appended 'pub fn headstart_m0_probe_${k}() -> u64'"
  fi
  EDITED_FILE=$f
  ! git -C "$SRC" diff --quiet || die "edit $kind step $k produced no change"
}

revert_edit() {
  [ -n "$EDITED_FILE" ] && git -C "$SRC" checkout -- "$EDITED_FILE"
  EDITED_FILE=""; HS_CUR_EDIT=""
}

stage_incr() {
  require_setup; stage_begin incr
  [ -n "$HS_BODY_FILE" ] && [ -n "$HS_BODY_ANCHOR" ] && [ -n "$HS_IFACE_FILE" ] \
    || die "set HS_BODY_FILE, HS_BODY_ANCHOR and HS_IFACE_FILE (see README)"
  [ -f "$SRC/$HS_BODY_FILE" ] || die "no such file in worktree: $HS_BODY_FILE"
  [ -f "$SRC/$HS_IFACE_FILE" ] || die "no such file in worktree: $HS_IFACE_FILE"
  manifest_add '.incremental = {command: "cargo check --workspace --all-targets", steps: $s, series_per_kind: $n,
      order: "body series, iface series, repeated (ABAB); untimed-for-stats settle check after every revert",
      body: {file: $bf, anchor: $ba, edit: "insert `let _headstart_m0_probe = std::hint::black_box(<k>u64);` after the anchor line; previous step reverted first"},
      iface: {file: $if, edit: "append `pub fn headstart_m0_probe_<k>() -> u64 { <k> }` (new pub item, new name every step); previous step reverted first"},
      reason: $why}' \
    --argjson s "$HS_INCR_STEPS" --argjson n "$HS_INCR_SERIES" --arg bf "$HS_BODY_FILE" \
    --arg ba "$HS_BODY_ANCHOR" --arg if "$HS_IFACE_FILE" --arg why "$HS_EDIT_REASON"
  local c=(cargo check --workspace --all-targets)
  scenario_add incr_prime prime "${c[*]}"
  scenario_add incr_noop_check incremental "${c[*]}"
  scenario_add incr_body_check incremental "${c[*]}"
  scenario_add incr_iface_check incremental "${c[*]}"
  scenario_add incr_settle settle "${c[*]}"
  run_sample incr_prime A prime 0 0 - warm -- "${c[@]}"
  [ "$LAST_RC" -eq 0 ] || die "prime check failed; incremental scenarios cannot run"
  local i; for i in 1 2 3; do run_sample incr_noop_check A incremental 0 "$i" - warm -- "${c[@]}"; done
  ensure_clean "noop"
  local series kind k settle_step
  for series in $(seq 1 "$HS_INCR_SERIES"); do
    settle_step=0
    for kind in body iface; do
      settle_step=$((settle_step + 1))
      for k in $(seq 1 "$HS_INCR_STEPS"); do
        apply_edit "$kind" "$((series * 100 + k))"
        run_sample "incr_${kind}_check" A incremental "$series" "$k" - warm -- "${c[@]}"
      done
      revert_edit
      ensure_clean "incr_${kind} series $series"
      # step 1 = settle after the body series, 2 = after the iface series
      run_sample incr_settle A settle "$series" "$settle_step" - warm -- "${c[@]}"
    done
  done
  ensure_clean incr
  "$SUMMARIZE" samples "$SAMPLES" > "$HS_OUT/summary.json"
  stage_end incr
}

stage_summary() {
  "$SUMMARIZE" samples "$SAMPLES" > "$HS_OUT/summary.json"
  say "summary written: $HS_OUT/summary.json"
}

stage_cleanup() {
  say "cleanup: removing $TARGET and worktree $SRC"
  rm -rf "$TARGET"
  if [ -d "$SRC" ]; then
    ensure_clean cleanup
    git -C "$REPO" worktree remove --force "$SRC"
  fi
  rm -rf "$RAWLOG"
  rmdir "$HS_WORK" 2>/dev/null || true
  [ -s "$MANIFEST" ] && manifest_add '.cleanup = {at: $t, removed: [$w]}' \
    --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg w "$HS_WORK"
  return 0
}

CUR_STAGE=""
STAGES=("$@")
[ ${#STAGES[@]} -gt 0 ] || STAGES=(setup timings cold incr cleanup)
mkdir -p "$HS_OUT"
for s in "${STAGES[@]}"; do
  case "$s" in
    setup|timings|cold|incr|summary|cleanup) CUR_STAGE=$s; "stage_$s" ;;
    *) die "unknown stage: $s" ;;
  esac
done
[ -s "$SAMPLES" ] && "$SUMMARIZE" samples "$SAMPLES" > "$HS_OUT/summary.json"
say "done: ${STAGES[*]}"
