# Headstart M0: stable Rust build baselines

Tracking issue: #532. This directory holds the instructions and the record
schema for the stable-toolchain build baselines that later Headstart work
(#536) compares against. It is measurement only: the runner never edits
`Cargo.toml`, `rust-toolchain.toml`, CI, compiler flags or the caller's
checkout.

| Path | What it is |
| --- | --- |
| `scripts/bench/headstart-aien.sh` | the runner (bash) |
| `scripts/bench/headstart-summarize.sh` | summarizer and Cargo timings extractor (bash + jq) |
| `scripts/bench/headstart-selftest.sh` | self-test on a throwaway fixture workspace, with negative cases |
| `benchmarks/headstart/schema.json` | JSON Schema for `manifest.json` and each `samples.jsonl` line |
| `benchmarks/data/headstart/baseline-<date>-<short>/` | one run: raw samples, manifest, summary, timings extract, log tails |

No Python is used anywhere. Requirements: bash, git, jq, awk, sed, pgrep,
cargo through rustup, and GNU `/usr/bin/time` for max RSS and CPU% (when it
is missing the samples record `time_v.available: false` instead of numbers).

## How a run works

1. `setup`: `git worktree add --detach $HS_WORK/src <commit>` outside the
   caller checkout (refused when `HS_WORK` is inside it), its own
   `CARGO_TARGET_DIR=$HS_WORK/target`, then `cargo fetch --locked` once so the
   cargo registry and git caches are warm. Writes `manifest.json` (commit,
   `rustc -vV`, `cargo -V`, arch, nproc, CPU models, MemTotal/MemAvailable,
   features, profiles, target dir, jobs, build env, date).
2. `timings`: one cold `cargo build --workspace --timings`. The `UNIT_DATA`
   block of `cargo-timing.html` is cut out with sed and validated with jq
   (`timings/units.json`), the observed critical path and the pipelining
   estimate are computed into `timings/critical-path.json`, and the report is
   kept gzipped.
3. `cold`: interleaved rounds. Each round runs, in order: cold check (arm A or
   B), cold build workspace, cold build `spark-harness`, cold test
   `--no-run`, cold check (the other arm), cold build `spark-supervisor`, cold
   build `aien-runtime`. The A/B arm order swaps every round; the two arms of
   the same command are the A/A noise estimate. "Cold" means the target dir
   is deleted before every attempt while the registry stays warm. If a cold
   workspace sample in round 1 exceeds 1200 s the run drops to 3 rounds and
   says so in `manifest.protocol.samples_reduced`.
4. `incr`: prime `cargo check --workspace --all-targets`, three no-op checks,
   then for each series a 10-step body-only edit and a 10-step
   interface-changing edit (each step reverts the previous edit first, so
   every step is a fresh one-edit change). After every series the edit is
   reverted, `git diff --quiet` is verified (abort when not clean) and an
   untimed-for-stats `incr_settle` check brings the target dir back to the
   pristine source. Two series per kind run as body, iface, body, iface.
5. `cleanup`: deletes the target dir, the raw logs and the worktree.

Before every sample the runner first waits, uncapped, while the host's
shared quiet flag is held by another job (`quietlock check`; disable with
`HS_QUIETLOCK=0` on hosts without it), recording `quiet_flag_wait_s`. In the
committed run the timings stage predates this check, so its three samples
lack that field. Then it waits until the 1-minute load average is below
`HS_LOAD_MAX` (cap `HS_WAIT_CAP_S`), records load before and after, and lists
foreign compiler processes at the start and every `HS_MON_INTERVAL_S` seconds.
A foreign process is a `cargo` process, or a `rustc` / `clippy-driver` /
`rustdoc` process with `--crate-name` (an actual compile), that is not in the
runner's process tree. A sample that saw one is `contaminated: true` and is
re-run up to `HS_MAX_RERUNS` times; earlier attempts stay in `samples.jsonl`
with `final: false`.

Every command's exit status is recorded. A failed build is a failure
(`ok: false`, with the last 80 log lines kept); it is never counted in a
median and never replaced by a smaller workload.

## Running it

From the repository root, with the stable toolchain pinned by
`rust-toolchain.toml`:

```bash
bash scripts/bench/headstart-selftest.sh            # about a minute, must print 0 failed
HS_COMMIT=<sha> \
HS_BODY_FILE=<repo-relative .rs file> HS_BODY_ANCHOR='<exact fn opening line>' \
HS_IFACE_FILE=<repo-relative lib.rs> HS_EDIT_REASON='<why these targets>' \
bash scripts/bench/headstart-aien.sh                # setup timings cold incr cleanup
```

Stages can be run separately (`setup timings`, then `cold incr cleanup`)
against the same `HS_WORK` and `HS_OUT`, which is how the edit targets were
chosen from the measured critical path before the incremental stage. The
other knobs are listed at the top of the runner.

## Reproducing the summary

`summary.json` is a pure function of `samples.jsonl`:

```bash
D=benchmarks/data/headstart/baseline-<date>-<short>
bash scripts/bench/headstart-summarize.sh samples $D/samples.jsonl | cmp - $D/summary.json && echo identical
```

Per scenario and arm it reports `wall_s` (counted samples: final attempt and
exit 0) and `wall_s_clean` (counted and not contaminated), each with n,
median, min, max, MAD (median absolute deviation) and sample stdev, plus
failures with exit status, superseded attempts, median max RSS and median
CPU%. `aa_noise` gives the A/A median difference in seconds and percent.

Max RSS comes from `/usr/bin/time -v`, which reports `ru_maxrss` of the
waited process tree: per getrusage(2) that is the largest single process, not
the sum over parallel rustc jobs.

## Critical path and the estimate

`critical-path.json` walks back from the last unit to finish along the edge
that unblocked each unit: cargo records, per unit, which units became
runnable when it finished (`unblocked_units`) or when its metadata was ready
(`unblocked_rmeta_units`), and `sections[frontend].end` marks when the
metadata was ready. That chain is the observed critical path of that one
build (scheduling delays included).

The estimate (always labeled ESTIMATE) of what a metadata-pipelining compiler
change could save on that build:

- upper bound = min(sum of predecessor frontend time on rmeta-gated links of
  the chain, build end minus floor), where floor = max(longest single unit,
  sum of unit durations / jobs). Full-gated links (build scripts, proc macros,
  linked binaries) need finished artifacts and are not shortened.
- lower bound = 0 s, because stable cargo exposes no unit graph
  (`--unit-graph` is unstable), so the slack of the next-longest chain is
  unknown and the critical path may simply move to another chain.
