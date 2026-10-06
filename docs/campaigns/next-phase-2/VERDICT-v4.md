# NEXT-PHASE-2 (ACCEPTANCE-v4): campaign verdict

```text
receipt  = e401fffe189506ecc0152f82bfe895c1374842746ae7cf4b5ac83462ccafe13c.json (never edited)
VERDICT  = PASS
C7a      = PASS (v3: FAIL)
C7c      = PASS (new row: real panic at start)
C6d      = NOT_APPLICABLE (guard; named, not counted as a pass)
C6i      = PASS (run exactly once)
```

This note sits beside the receipt and is written after the run. The receipt is not edited. Every
reading below is the frozen ACCEPTANCE-v4 rule (over v3 and v2) applied as written; no rule was
added after the run.

## 1. Inputs

- sovereign-core `4f2776c` (code at `d79f1c5`; `4f2776c` changes only the harness and the receipt
  script); omega compose `62b6a28`. ACCEPTANCE-v4 frozen alone at `9582043`.
- Builds (receipt `builds`, `build_env`, `link_proof`): `gpu` sha256 `0ab7abc4…5fc95b`,
  `cpu-fault` sha256 `ca0cfe84…f833e`. Both carry the `librx_compose.a` skill names and no compose
  stub warning; `gpu` carries `NVIDIA_DGX_SPARK_GB10_SM121` and no GPU stub warning; the two fault
  hook strings (`reconcile_panic`, `reconcile_error`) appear only in `cpu-fault`. The `gpu` binary
  is byte-identical to the v3 `gpu` binary: the cut changes only code behind the `fault-hold`
  feature.
- F0v4 valid: S3 committed on `OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)`,
  Llama-3.2-1B-Instruct, inside one quietlock hold with start and release whispers. The
  pre-registered F0v3 fallback was not needed. F0v2 was used only as the C8a input (files sha256
  re-checked by the harness before each C8a run).

## 2. Rows

All 86 runs: 83 PASS, 3 NOT_APPLICABLE (C6d), 0 FAIL, 0 NOT_RUN. Every control PASS; C3 control
PASS. Every row of v2 and v3 not replaced: PASS. Rows of ACCEPTANCE-v4 section 5:

| Row | Verdict | Reading (receipt checks) |
|---|---|---|
| C7a | PASS x3 | start line `Reconcile: refused: fault hold reconcile_error … effect commands refuse until a successful reconcile (aien compose reconcile)`; socket up; authorize answered; first execute `ReconcileFailed`, no intent, no write; reconcile ok; second execute DONE, target written once; restart `checked 0` |
| C7c | PASS x3 | the hook daemon exited by itself with status 134 (abort); log names `fault hold reconcile_panic`; `recall` failed to connect; home hashes identical before and after; target unchanged; provenance empty; a socket file was left in all three runs (recorded, not scored, H3); the restart, with that file kept, cleared it and came up `checked 0`; recall ok; ledger empty |
| C6d | NOT_APPLICABLE x3 | `jspace.data` 0 bytes and `spill_end` 0 before and after the case |
| C6i | PASS x1 | refused `E_REPLAY`; `jspace.meta` still absent; `cortex.cx` byte-identical |

R6 (no `E_MARK` in controls, C1..C5, C6c-ctl, C7a, C7c, C8): held in every run.

## 3. What this means in practice

- A start-up reconcile that returns an error leaves the daemon serving reads and controls while
  every effect command refuses, until an operator reconcile succeeds. Proven on the release path
  (C7a, C7b).
- A start-up reconcile that panics stops the daemon before it serves anything, with nothing
  written to the home, the workspace or provenance; the next start clears the left socket file
  and comes up normally (C7c). That a daemon whose panic cause persists keeps refusing to serve
  is inferred, not tested: the C7c restart ran without the hook.

## 4. Not proved (declared)

As in the receipt: the v3 list, plus the `Err(e)` arm of `server.rs` (`Reconcile: failed:`), which
release builds cannot reach (panic = abort), and the cause of a real start-up panic.

## 5. Disclosure

No smoke run preceded this campaign. Before ACCEPTANCE-v4 was frozen, one probe start of the v3
`cpu-fault` binary with `reconcile_panic` on a scratch copy of F0v3 established the C7c
expectations (ACCEPTANCE-v4 H4); it is not part of the receipt. Workspace `cargo test` on the stub
build: four failures outside this cut (`spark-adapters` tier1 f61 and f63, which check the git
branch and remote of the checkout; tier2 f36 axum route, also failing on the v3 run; and
`spark-cockpit-rs` sub-millisecond latency, which passed when run alone).

## 6. Where this PASS is weaker than it looks (fresh-clone review of sc#229, before merge)

Added after review; no code change, receipt untouched.

1. **C7a's error is synthetic.** The `reconcile_error` hook returns
   `ControlResponse::Error` without calling `reconcile()` (`crates/aien-runtime/src/effects.rs:814-822`),
   which feeds the error arm at `effects.rs:843-846`. C7a proves the handling after an error (gate
   set, effect commands refused, operator reconcile clears it), not that a genuine failure
   produces an error. Coverage of the same handling by genuine errors: C6a, C6b, C6c, C6e, C6f,
   C6g, C6i and C7b. The `Err(e)` arm of `server.rs:192-198` stays not proved (section 4).
2. **C7c is a confirmation, not a blind prediction.** Its outcome was first seen in the
   pre-freeze probe of the v3 `cpu-fault` binary (ACCEPTANCE-v4 H4). The panic code did not change
   between v3 and v4, so the campaign re-confirms an observed outcome.
3. **Persistent panic not tested.** Section 3 states this: the restart ran without the hook.
4. **State dir not checked in C7c.** H4 records that the state dir stayed empty in the probe; the
   C7c row checks the home, the target and the provenance dir, not the state dir. For the campaign,
   the state dir after the aborted start is not checked.
5. **Receipt script repetition count.** `make-receipt.sh` lines 43-44 score a row PASS on any
   number of passing repetitions (at least one). No effect on this receipt: every injected row has
   its 3 runs and C6i its 1. To be fixed in the next spec version, not in this cut.
