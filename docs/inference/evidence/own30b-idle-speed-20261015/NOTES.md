# OWN-30B idle-GPU speed check, merged main ca695de9 / omega 4799f9eb (2026-10-15)

CALC-EXEMPT: every number here is copied from a measured log in this folder (the `*.log` files are force-added past
.gitignore so SHA256SUMS checks on a fresh clone) or from the notes of #525 (`own30b-omega4799-20261015`),
#524 (`own30b-requal-main-20261015`) and #514 (`pkv514-chip-20261014`); none is derived.

## Why
#525 pinned omega 4799f9eb but measured speed only with the GB10 shared with the MAX Qwen3-8B experiment
(decode ~2.5 tok/s), so the idle speed of the merged build was unmeasured. The experiment is paused and MAX on
:18097 is stopped; this folder reruns the speed-relevant gates on an idle GPU.

## Build and commands
- sc main ca695de9 (= #525's build afe59ae1 plus evidence docs), omega 4799f9eb, libomega_gpu.a 0629ad9c.
- Binaries: the pinned set `~/handoffs/pin-bin-afe59ae1` (aien-serve-http sha256 8f58298f..., own30b_check
  8220e466...; full digests in each `*.start` and in #525's `binaries.sha256`).
- `own30b-idle-gates.sh`: the same commands as #525's `own30b-gates.sh` (which copied #524), without
  `--force-memory` (not needed: no tool refused on memory), and only check-full, ttft, graph-off, graph-on.
  One quietlock hold (owner IDLESPEED, 20 min max) per run, released between runs, each under /tmp/aien-gb10.lock.
- `pkv-item5-run.sh`: #514's `run-all.sh` item5-two-process step, same env, `scripts/own30b-pkv-two-process.sh`
  from main ca695de9, pinned aien-serve-http.

## Machine state
Before the first run (09:56Z): `nvidia-smi` no compute apps, GPU 0 %, P8, 40 C, 5.04 W; MemAvailable
112612368 kB; listeners only spark-cockpit :18095 and spark-mail :18092 (no :18091, no :18097).
Every `*.start` records the compute-app list (empty for all four runs), GPU utilization (0-1 %) and
MemAvailable (112135808 to 112684012 kB); the tools printed "mem available before load" 106.9 to 107.5 GB.
PKV item 5 `pkv-item5/ground.txt`: no compute apps, MemAvailable 112493976 kB.

## Speed and verdicts, idle GPU (this folder) vs #524 (idle) and #525 (shared)

| run | this folder, idle, omega 4799f9eb | #524, idle, omega 62833658 | #525, shared with MAX |
|---|---|---|---|
| check-full decode p1 / p2 / p3 / p4 tok/s | 38.8 / 36.1 / 37.2 / 36.6 | 38.9 / 35.9 / 37.4 / 37.0 | 2.4 / 2.4 / 2.6 / 2.5 |
| check-full verdicts | rc 0, parity PASS (layers 0, 23, 47), 10/10 CAUGHT, prefill chunk 64 vs 1 PASS, tokens IDENTICAL x4 | same | same |
| ttft 512 rep0 / rep1, OFF / ON dev | 7.66 / 0.315 s, 7.69 / 0.322 s | 7.66 / 0.33 s, 7.63 / 0.31 s | 48.0 / 1.43 s, 46.7 / 1.60 s |
| ttft 2048 rep0 / rep1, OFF / ON dev | 32.0 / 0.891 s, 31.9 / 0.840 s | 32.2 / 0.88 s, 32.1 / 0.85 s | 204.4 / 2.52 s, 202.6 / 2.52 s |
| ttft verdicts | rc 0; SUMMARY tokens DIFFER (OFF vs ON, at 7 for len 512; IDENTICAL for len 2048); devhost vs host BIT-IDENTICAL, 0 expert flips | same | same |
| graph-off (CTA budget 64) tok/s | 38.8 / 36.3 / 37.5 / 36.9, PASS, 271 submits per step | 38.5 / 35.7 / 36.9 / 36.0 | 2.6 / 2.4 / 2.5 / 2.5 |
| graph-on (+ OWN30B_DECODE_GRAPH=1) tok/s | 42.8 / 39.8 / 41.1 / 40.2, PASS, 15/16 (p2 7/7) steps one list, 1.0 submits per step | 42.6 / 39.5 / 40.8 / 40.1 | 15.2 / 13.9 / 14.3 / 14.4 |

TTFT ON dev in ms from `ttft.raw`: 512 = 315.2 / 322.2, 2048 = 890.8 / 839.5; OFF 7661.6 / 7686.3 and
32043.7 / 31892.2. TTFT ON host (no devhost): 512 = 802.1 / 617.0 ms, 2048 = 1986.1 / 1998.5 ms.
graph-off and graph-on also report parity PASS, 10 CAUGHT lines each and tokens IDENTICAL x4.

Result: on an idle GPU the merged 4799f9eb build runs at #524's speed (decode within 0.4 tok/s per prompt
either way, TTFT@2048 ON 0.84-0.89 s vs 0.85-0.88 s). No regression from the pin. Every correctness verdict
is the same as #524 and #525.

## Persistent KV item 5, two 30B processes at once (pkv-item5/)
`item5-two-process rc=0`, `VERDICT PASS` (`pkv-item5/two-process/summary.txt`), all 7 checks PASS:
proc1 turn 1 == uninterrupted turn 1; proc2 restored turn 2 == uninterrupted turn 2; proc2 restores == 1;
proc1 park refused no chunk; proc2 restore covered at least the 750-token turn-1 prompt; proc1 RAM turn 2 ==
uninterrupted turn 2; proc1 restores == 0.

| measure | this folder, omega 4799f9eb | #514 run3, omega d0889a94 |
|---|---|---|
| verdict | PASS | PASS |
| MemAvailable before proc2 load | 71.0 GB | 71.1 GB |
| MemAvailable with both loaded | 38.2 GB | 38.2 GB |
| RSS proc1 / proc2 | 37290944 / 33802212 kB | 37294816 / 33803920 kB |
| proc2 restored_tokens | 800 (restore_bytes 78643200, 13 reads) | 800 |

This closes the item #525 could not run beside the MAX experiment.

## Server left running
After the runs, `:18091` was started from `~/handoffs/own30b-serve-cmdline-merged.txt` (log
`~/workspace/exp445-runlogs/own30b-18091.log`, not in this folder): "omega engine ready in 26.0 s", 30.53 GB
resident, `/v1/models` answers `qwen3-30b-a3b-instruct-2507`. #525's smoke under GPU sharing took 71 s.

## Limits
- One run per gate (ttft has 2 reps). The idle numbers match #524 to within run-to-run spread; a difference
  smaller than that spread is not excluded.
- check-wp, gate and gate-short were not rerun: they carry no speed figure, and #525 already recorded their
  verdicts on this build (same as #524).
- PKV items 1-4 were not rerun here; #525 recorded them on this build (PASS) under GPU sharing.
