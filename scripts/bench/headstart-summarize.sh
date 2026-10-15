#!/usr/bin/env bash
# Headstart M0 (#532) summarizer. Pure bash + jq + sed, no Python.
#
#   headstart-summarize.sh samples <samples.jsonl>            -> summary JSON on stdout
#   headstart-summarize.sh timings <cargo-timing.html>        -> UNIT_DATA as a JSON array
#   headstart-summarize.sh critical <units.json> [jobs]       -> critical-path JSON on stdout
#
# `samples` is deterministic: running it again on the same samples.jsonl
# reproduces summary.json byte for byte (see benchmarks/headstart/README.md).
set -euo pipefail

usage() {
  sed -n '2,9p' "$0" >&2
  exit 64
}

# Shared jq helpers: median, MAD (median absolute deviation), sample stdev.
JQ_STATS='
def r3: (. * 1000 | round) / 1000;
def median: sort | length as $n
  | if $n == 0 then null
    elif $n % 2 == 1 then .[($n - 1) / 2]
    else (.[$n / 2 - 1] + .[$n / 2]) / 2 end;
def mad: median as $m | if $m == null then null else map(. - $m | fabs) | median end;
def stdev: length as $n
  | if $n < 2 then null
    else (add / $n) as $mu | (map((. - $mu) * (. - $mu)) | add / ($n - 1)) | sqrt end;
def stats: if length == 0 then {n: 0, median: null, min: null, max: null, mad: null, stdev: null}
  else {n: length, median: (median | r3), min: (min | r3), max: (max | r3),
        mad: (mad | r3), stdev: (stdev | if . == null then null else r3 end)} end;
def nn_median: map(select(. != null)) | if length == 0 then null else median | r3 end;
'

cmd_samples() {
  local f=$1
  [ -s "$f" ] || { echo "summarize: $f is missing or empty" >&2; exit 65; }
  # Every line must parse; a corrupt line is an error, never silently skipped.
  jq -e -s 'length > 0' "$f" >/dev/null
  jq -S -s "$JQ_STATS"'
    . as $all
    | (map(select(.final == true and .ok == true))) as $okf
    | {
        schema: "headstart-summary/v1",
        source: "samples.jsonl",
        n_lines: ($all | length),
        rules: {
          counted: "final attempt, exit status 0 (failed builds are never counted)",
          clean: "counted and contaminated == false",
          dispersion: "mad = median absolute deviation; stdev = sample standard deviation (n-1)",
          units: "seconds for wall_s, kilobytes for max_rss_kb, percent for cpu_percent"
        },
        groups: (
          group_by([.scenario, .arm])
          | map(
              . as $g
              | (map(select(.final == true and .ok == true))) as $ok
              | (map(select(.final == true and .ok == true and .contaminated == false))) as $clean
              | {
                  scenario: $g[0].scenario,
                  arm: $g[0].arm,
                  kind: $g[0].kind,
                  command: $g[0].command,
                  n_lines: ($g | length),
                  n_final: (map(select(.final == true)) | length),
                  n_failed: (map(select(.final == true and .ok == false)) | length),
                  n_superseded: (map(select(.final == false)) | length),
                  n_contaminated_counted: ($ok | map(select(.contaminated == true)) | length),
                  failures: (map(select(.ok == false)) | map({sample_id, exit_status})),
                  wall_s: ($ok | map(.wall_s) | stats),
                  wall_s_clean: ($clean | map(.wall_s) | stats),
                  max_rss_kb_median: ($ok | map(.time_v.max_rss_kb) | nn_median),
                  cpu_percent_median: ($ok | map(.time_v.cpu_percent) | nn_median)
                }
            )
        ),
        aa_noise: (
          [ $okf[] | select(.aa_pair != null) ] | group_by(.aa_pair)
          | map(
              (map(select(.arm == "A") | .wall_s) | median) as $a
              | (map(select(.arm == "B") | .wall_s) | median) as $b
              | {
                  scenario: .[0].aa_pair,
                  n_a: (map(select(.arm == "A")) | length),
                  n_b: (map(select(.arm == "B")) | length),
                  median_a_s: ($a | if . == null then null else r3 end),
                  median_b_s: ($b | if . == null then null else r3 end),
                  delta_s: (if $a == null or $b == null then null else ($b - $a) | r3 end),
                  delta_pct: (if $a == null or $b == null or $a == 0 then null
                              else (($b - $a) / $a * 100) | r3 end)
                }
            )
        )
      }' "$f"
}

cmd_timings() {
  local html=$1
  [ -s "$html" ] || { echo "summarize: $html is missing or empty" >&2; exit 65; }
  # Cargo embeds the per-unit data as `const UNIT_DATA = [ ... ];` (JSON) in
  # the timing report. Cut that block out and validate it as JSON.
  local block
  block=$(sed -n '/^const UNIT_DATA = \[/,/^\];/p' "$html" | sed -e '1s/^const UNIT_DATA = //' -e '$s/;$//')
  [ -n "$block" ] || { echo "summarize: no UNIT_DATA block in $html" >&2; exit 65; }
  printf '%s\n' "$block" | jq -e 'type == "array" and length > 0' >/dev/null
  printf '%s\n' "$block" | jq -c .
}

cmd_critical() {
  local units=$1 jobs=${2:-null}
  jq -S --argjson jobs "$jobs" "$JQ_STATS"'
    # frontend end (seconds after unit start) = when its .rmeta is ready.
    def fe_end: if (.sections // null) == null then null
      else ([.sections[] | select(.[0] == "frontend") | .[1].end] | first) end;
    . as $u
    | ($u | map({key: (.i | tostring), value: .}) | from_entries) as $by
    # Each unit records which units became runnable when it finished
    # (unblocked_units) or when its metadata was ready (unblocked_rmeta_units).
    # That edge is the dependency that actually gated the unblocked unit.
    | ([ $u[] | .i as $d
         | ((.unblocked_units[]? | {key: tostring, value: {pred: $d, gate: "full"}}),
            (.unblocked_rmeta_units[]? | {key: tostring, value: {pred: $d, gate: "rmeta"}})) ]
       | from_entries) as $pred
    | ($u | max_by(.start + .duration)) as $last
    | [ $last.i | recurse(($pred[tostring] // empty).pred) ] | reverse
    | map($by[tostring]) as $chain
    | ($chain | to_entries | map(
        .value as $c
        | ($pred[$c.i | tostring]) as $p
        | ($p | if . == null then null else $by[.pred | tostring] end) as $pu
        | {
            i: $c.i, name: $c.name, version: $c.version, target: $c.target, mode: $c.mode,
            start_s: ($c.start | r3), duration_s: ($c.duration | r3),
            frontend_s: ($c | fe_end | if . == null then null else r3 end),
            gated_by: (if $p == null then null else {i: $p.pred, name: $pu.name, gate: $p.gate} end),
            gate_time_s: (if $p == null then null
                          elif $p.gate == "full" then ($pu.start + $pu.duration) | r3
                          else (($pu | fe_end) as $fe | if $fe == null then null else ($pu.start + $fe) | r3 end) end),
            pred_frontend_s: (if $p == null or $p.gate != "rmeta" then null
                              else ($pu | fe_end | if . == null then null else r3 end) end)
          })) as $links
    | ($u | map(.duration) | add) as $cpu
    | ($u | max_by(.duration)) as $longest
    | ($last.start + $last.duration) as $wall
    | ([ $links[] | select(.gated_by.gate == "rmeta") | .pred_frontend_s // 0 ] | add // 0) as $rmeta_wait
    | ([ $links[] | select(.gated_by.gate == "full") ] | length) as $n_full
    | (if $jobs == null then null else $cpu / $jobs end) as $cpu_floor
    | ([$longest.duration, ($cpu_floor // 0)] | max) as $floor
    | {
        schema: "headstart-critical-path/v1",
        units: ($u | length),
        observed: {
          build_end_s: ($wall | r3),
          sum_unit_duration_s: ($cpu | r3),
          longest_unit: {name: $longest.name, target: $longest.target, mode: $longest.mode, duration_s: ($longest.duration | r3)},
          chain_length: ($links | length),
          chain_full_gated_links: $n_full,
          chain_rmeta_gated_links: ([ $links[] | select(.gated_by.gate == "rmeta") ] | length),
          chain: $links
        },
        estimate: {
          label: "ESTIMATE, not a measurement",
          method: ("Walk back from the last unit to finish along the edge that unblocked each unit "
            + "(cargo unblocked_units / unblocked_rmeta_units). On rmeta-gated links the dependent "
            + "waited for the predecessor frontend (sections.frontend.end). A metadata-pipelining "
            + "compiler change can at most remove those waits on this chain; full-gated links "
            + "(build scripts, proc macros, linked binaries) need finished artifacts and are not "
            + "shortened. Upper bound = min(sum of predecessor frontend time on rmeta-gated chain links, "
            + "build_end - floor) where floor = max(longest unit, sum of unit durations / jobs). "
            + "Lower bound = 0 s: without the full unit graph (stable cargo has no --unit-graph) the "
            + "slack of the next-longest chain is unknown, so the chain may simply shift."),
          jobs: $jobs,
          floor_s: ($floor | r3),
          rmeta_wait_on_chain_s: ($rmeta_wait | r3),
          upper_bound_saving_s: ([$rmeta_wait, ($wall - $floor)] | min | if . < 0 then 0 else . end | r3),
          upper_bound_saving_pct: (([$rmeta_wait, ($wall - $floor)] | min | if . < 0 then 0 else . end) / $wall * 100 | r3),
          lower_bound_saving_s: 0
        }
      }' "$units"
}

[ $# -ge 2 ] || usage
case "$1" in
  samples) cmd_samples "$2" ;;
  timings) cmd_timings "$2" ;;
  critical) cmd_critical "$2" "${3:-null}" ;;
  *) usage ;;
esac
