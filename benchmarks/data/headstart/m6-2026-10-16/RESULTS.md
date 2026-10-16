# Headstart M6 held A/B samples, Spark, 2026-10-16 (#538)

Verdict: **10% gate NOT MET.** Turning provisional preparation on gives no
measurable wall-time gain on this workload. On the chain and parallel shapes
the on arm is about 1% to 2% slower, which is inside the A/A spread. The
feature stays disabled by default.

## Setup

- Build LINKED (real librx_compose, real World commits), platform SPARK
  (aarch64), rustc 1.98.1, threads 4, branch `feat/m6-headstart-qualification`.
- `scripts/bench/headstart-m6-ab.sh 5` with the defaults: 5 sample indices,
  shapes chain, parallel and failure, arm order alternating per index, plus
  an A/A pair (off, off) per index and shape.
- Each sample is one process under `quietlock hold` (owner hs-m6, 5 minutes
  at most). Run started 2026-10-16T07:40:24Z. Peer sessions were told about
  the window before and after.
- Raw data: `samples.jsonl` (60 lines), `manifest.json`, `logs/`,
  `summary.txt`.

## Gates checked per sample

- 60 of 60 samples report `"build":"LINKED"` and `"deadlock":false`.
- `failures.txt` is absent: every sample passed `check_sample` (no task
  committed twice, every commit record final-verified) and the script's
  identity check (one new line matching build, sample, shape, arm and tag).
- Peak RSS per process stayed between 21 and 23 MB in every arm.

## Measured (median wall ms, n=5 each)

| shape | A (off) | B (on) | B vs A | AA1 (off) | AA2 (off) | A/A spread |
|---|---|---|---|---|---|---|
| chain | 818.350 | 827.020 | +1.06% slower | 842.596 | 831.742 | 1.31% |
| parallel | 1023.187 | 1042.630 | +1.90% slower | 1072.389 | 1101.359 | 2.70% |

Percentages: chain B vs A calc:241b5b8a38b447044c1500e64948210825c83cc41f9f83160e39ab1e8064a125,
parallel B vs A calc:a001643bcd67f3464e11e75b2988df723237b9cb09eaae394ebd0be5315c9c1f,
chain A/A calc:41ee6c07f0a1d03095d0b8cfb0c699e2bdf5b46a2702022e398d7782fe1b3673,
parallel A/A calc:821235093eea80da0a63fa1434c4f9dfded0215a42b71e8bbed32af9d52d70c3.

The failure shape is a correctness check, not a timing comparison: its on
arm waits on purpose for consumers to prepare before the producer fails.

## Estimate (not measured)

The per-sample `ceiling_estimate` (median) is 0.049 (off) and 0.056 (on) on
chain, 0.023 (off) and 0.018 (on) on parallel. Read-only preparation is at
most about 2% to 6% of a sample's wall time here, so even perfect overlap
could not reach 10%. A workload where preparation is a larger share is needed
before this feature can pass the gate.

## Limits

- One machine, one day, n=5 per arm. The on arm's small slowdown is not
  separated from noise.
- Synthetic shapes from `headstart_ab.rs`, not a recorded production
  workflow.
