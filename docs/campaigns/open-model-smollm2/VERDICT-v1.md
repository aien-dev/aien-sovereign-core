# OPEN-MODEL-SMOLLM2 campaign v1: verdict

Verdict by the frozen rows: **FAIL**. T1 NOT_RUN (GPU warm-up crash), T2 FAIL, T3 FAIL.
The Llama-3.2-1B baseline and CAND-4 are unaffected. This file is a record; it changes
nothing in `ACCEPTANCE-v1.md` (sha256 `1a4fd43fda2b84c43ba28ab6081c918c1ecba61ce856cc8fe247f022613ecd25`)
or `run-smollm2.sh` (sha256 `d7ce4c23bbadfbc0450d60927c57f62b7d88aa6e1bce062a09705ae73d223b0c`);
both digests were re-checked unchanged before this file was written.

Binary: sovereign-core commit `12716defccc8915cde53e229ac7617ada45c3d55`, kept as git tag `campaign/smollm2-v1-run2` (PR #233 head 4aaa4e6 plus the
frozen files), omega.lock 62b6a2899fa230e57fcf7855ea7e2c26404514b6, GPU library sha256
`ef80e4313d4e678335dceac3493aadec8d0df4352dd5356854b92e0a9e070887`, GB10 backend, no CUDA.

## 1. Run history (nothing deleted)

- **run_1 (void).** Every step failed in about 6 ms with
  `librx_compose.a is not linked (stub build)`. The campaign binary had been built without the
  compose library (setup error). No proposal attempt was made and no model output was produced, so
  no task result was seen. Voided for that reason only; kept locally in `void-run1/`.
- **run_2.** An explicit rerun after run_1, made after rebuilding with the compose library linked
  (AIEN_OMEGA_DIR set). It was not chosen after seeing any result. One launch per task, one attempt
  per task. The CPU diagnostic replies had been seen before the freeze (ACCEPTANCE-v1 Section 2.1).

## 2. Per task

| task | result | receipt |
|------|--------|---------|
| T1 | NOT_RUN: daemon did not start (driver exit 2), counts as FAIL under the frozen rule | none |
| T2 | FAIL | `e48454215373d00564419d23b1f0379d84e7a94ccb5b9503571213f51eb8f4c9.json` |
| T3 | FAIL | `b5d786295525e803c0d9ea94b702cba0599d80e13317f4d2ce5da8953631f33e.json` |

Receipts, summaries and replies are in this directory under `run2/`.

**T2 (model behaviour).** Row Q1 FAIL: the model proposed `CONTACT.txt`, the task asked for
`docs/CONTACT.txt`. Reply: `filename: CONTACT.txt` then `<Ada Lovelace, ada@example.org>`.
Q2, Q3 (eos), Q4 and A2..A6 PASS.

**T3 (model behaviour).** Row Q2 FAIL: phrase "tag the release" missing. Row Q3 FAIL: finish reason
`max_tokens` at 64 tokens. The reply was a markdown list (`# TODO.md`, `## Task 1: Write tests`,
`### Description: ...`) that was cut off before the third task. Q1 PASS (path TODO.md).

**Driver limitation, not a model fault.** On T2 and T3 the rows "Containment: workspace" and A1 FAIL
because of `./compose.cortex-mark`, a file the NP2 v3 composition code writes outside the workspace
and that the v5 driver does not exempt. This affects every model run through this driver on this base,
Llama included. CAND-4 handles it with a record-mark rule and the NEXT-PHASE-1 v6 owner is amending
its driver. The frozen rows were not changed for this run, so the rows still count as FAIL; the model
failures above stand on their own (T2 Q1; T3 Q2 and Q3).

## 3. Time per attempt against the 12 s attempt budget (measured, GB10)

| task | attempt 1 | tokens | stop | after it (of 29 s) |
|------|-----------|--------|------|--------------------|
| T2 | 9 973 ms | 19 | eos | 19.0 s left, a retry would have started |
| T3 | 20 652 ms | 64 | max_tokens | 8.3 s left, less than 12 s, no retry |

The pre-freeze estimate was 7.5 to 10.7 s for a normal reply and about 22.6 s for a full 64-token
attempt; both held. 12 s fits a normal SmolLM2 reply barely and does not fit a full-length attempt.
Constants A (12 000 ms) and B (29 000 ms) were not changed. Warm-up per daemon start was 15.2 to
17.5 s (declared, outside the task window). VmHWM about 61 GB.

## 4. T1 warm-up crash, from existing logs only (no GPU, no daemon start)

First lines of `camp-run/T1/daemon-1.log` after the socket bound (verbatim):

```text
OMEGA_BACKEND chip error: rmsnorm dim=2048: omega_gpu rc=-4 (CHIP_FAIL) (stage: m16_native_create_channel: RM_ALLOC class 0xcec0 status 0x51)

thread 'tokio-rt-worker' (196206) panicked at crates/aien-inference-abi/src/strict.rs:52:9:
STRICT_REAL_MODEL_VIOLATION: OmegaGb10Backend fell back to CPU in rmsnorm; this production build refuses to fall back (a dev/test run sets AIEN_DEV_FALLBACK=1 or builds with --features dev-fallback)
```

- DOCUMENTED: status 0x51 is `NV_ERR_NO_MEMORY` ("Out of memory") in nvidia-open 580.173.02
  `nvstatuscodes.h:110`. The failing call is the first channel allocation of the first GPU use
  (warm-up), before any task step. The daemon aborted at the strict no-fallback guard, as designed.
- OBSERVED: T1 started at about 15:03:15 local, about two minutes after run_1's last GPU daemon
  exited; T2 and T3 of the same run, same binary, started and warmed up normally. In run_1 all
  daemons started and warmed up.
- OBSERVED timing (local CDT, 2026-10-06): daemon 1 of T1 started 15:02:42, bound its socket at
  15:03:15.256, crashed at 15:03:15.455, about 0.2 s into warm-up, so at the first GPU call and not
  during a long prefill. The warm-up prompt is the fixed `WARM_UP_TEXT` (`server.rs`), 151 prompt tokens
  once ChatML-templated (T2 log: `Warm-up: 1 token in 17462 ms over 151 prompt tokens`).
- Hypothesis, **UNVERIFIED**: the channel allocation failed with an out-of-memory status because memory
  was momentarily short at that instant (this process maps about 61 GB on the shared 128 GB pool, and
  other heavy jobs on the machine, builds or a previous daemon's memory not yet returned, may have
  overlapped). Nothing in the logs shows which. Not reproduced, not investigated further; a check would
  need a GPU window and a memory trace before the launch. UNKNOWN whether it recurs.

## 5. Limits

- One launch per task, one attempt per task; T1 produced no model output at all.
- Decoding is greedy but GB10 output can differ from the reference on near-ties (diagnostic: 2 of 6
  prompts, T1 and V3). The T1 near-tie was not exercised because T1 did not run; T2 and T3 replies
  equal the diagnostic GB10 output, and T3 overruns on the CPU reference and Hugging Face too.
- The campaign does not show SmolLM2-1.7B can never do these tasks; it shows that under the frozen
  prompt, budget and rows it did not (wrong destination on T2, over-long output on T3).
- No prompt or format tuning was tried (frozen spec).
