# NEXT-PHASE-2 (ACCEPTANCE-v3): campaign verdict

```text
receipt  = 5bd665307665a8f7e014fa9c11c9d228f9e61c54d4cb9e2cd57f40308d4decff.json (never edited)
VERDICT  = FAIL (one row: C7a)
C6c      = PASS (v2: FAIL)
C2a      = PASS under the frozen G1 rule (v2: UNVERIFIED)
C6d      = NOT_APPLICABLE (guard; named, not counted as a pass)
C6i      = PASS (run exactly once)
C7a      = FAIL
```

This note sits beside the receipt and is written after the run. The receipt is not edited. Every
reading below is the frozen ACCEPTANCE-v3 rule applied as written; no rule was added after the run.

## 1. Inputs

- sovereign-core `3fa00e4` (code at `d3e597c`, crates unchanged since); omega compose `62b6a28`.
- Builds (receipt `builds`, `build_env`, `link_proof`): `gpu` sha256 `0ab7abc4…5fc95b`,
  `cpu-fault` sha256 `7aa59474…bf846b`. Both carry the `librx_compose.a` skill names
  (`compose.candidate.0`, `compose.verify`, `compose.commit`) and no compose stub warning; `gpu`
  carries `NVIDIA_DGX_SPARK_GB10_SM121` from `libomega_gpu.a` and no GPU stub warning;
  `reconcile_panic` appears only in `cpu-fault`.
- F0v3 valid: S3 committed on `OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)`,
  Llama-3.2-1B-Instruct, inside one quietlock hold (17:06-17:07Z, start and release whispers
  delivered). The pre-registered F0v2 fallback was not needed. F0v2 was used only as the C8a
  input; its files sha256 was re-checked by the harness before each C8a run.

## 2. Rows

All v2 rows not replaced: PASS (C1a, C1b, C2b, C2c, C2d, C3a, C3b, C4, C5a, C5b, C5c, C6a, C6b,
C6e, C6f, C6g, C6h, each 3 runs, controls PASS). C3 control PASS. Rows of ACCEPTANCE-v3 section 5:

| Row | Verdict | Reading (receipt checks) |
|---|---|---|
| C2a | PASS x3 | records 23 -> 28 across the restart; ids 24..28 are class 1, kind 65538, subjects 1..5; host count unchanged |
| C6c | PASS x3 | `Reconcile: refused` and recall both name `E_MARK_TRUNCATED` ("holds 25 records, its record mark says 26"); A2 refused; no write; journal and mark byte-identical to the damaged state |
| C6c-ctl | PASS x3 | start log `record mark advanced 25->26`; recall ok; no `E_MARK`; A2 DONE once |
| C6d | NOT_APPLICABLE x3 | `jspace.data` 0 bytes and `spill_end` 0 before; still 0 bytes at the end of the case, so the late injection never fired |
| C6i | PASS x1 | refused `E_REPLAY` (journal behind its J-Space anchor); `jspace.meta` still absent; `cortex.cx` byte-identical (9608 bytes) |
| C7a | FAIL x3 | see section 3 |
| C7b | PASS x3 | start refused `E_MARK_TRUNCATED`; recover `mark_lost` 1, old mark kept as `.cortex-mark.lost-9`, byte-identical; repair constraint with `"lost":1`; A3 refused `ReconcileFailed`, no write; reconcile ok; A3 DONE once |
| C8a | PASS x3 | one `mark adopted (seq 1, records 22)` line; 128-byte mark; recall ok; second start quiet |
| C8b | PASS x3 | one `mark adopted` line after the mark was deleted; recall ok; re-execute `AlreadySpent` |

R6 (no `E_MARK` in controls, C1..C5, C6c-ctl, C7a, C8): held in every run.

## 3. C7a FAIL

The rule expects the daemon to keep serving after a start-up reconcile panic and print
`Reconcile: failed: …`. In all three runs the daemon aborted instead (log: `panicked at
crates/aien-runtime/src/effects.rs:807:9`, shell `Aborted (core dumped)`), so no socket was
served and no command ran.

Cause (source): the workspace release profile sets `panic = "abort"` (`Cargo.toml:58`), and
`cpu-fault` is a release build (ACCEPTANCE-v2 section 4). A panic inside `spawn_blocking` ends
the process before `JoinHandle::await` can return `Err`, so the `Err(e)` branch at
`crates/aien-runtime/src/server.rs:190-197` (G4's `unwrap_or_else` path) cannot run in a release
build. G4 was read from source without checking the panic strategy, so the frozen injection
tests a path that release builds never take.

What this means in practice (OBSERVED): a start-up reconcile panic stops the daemon before it
serves anything, so no effect can run. That is fail-closed, but the daemon will not start until
the cause is fixed. The error path (`Reconcile: refused` plus the `ReconcileFailed` gate) is
proven by C6a..c, C6e..g, C6i and C7b.

For the next cut (not decided here): either amend the C7a rule so that a non-zero exit with no
socket counts as the expected outcome under `panic = "abort"`, or change the hook to return an
error instead of panicking.

## 4. Not proved (declared)

As in the receipt: the five items of v2, plus tamper resistance of the record mark, a real
process kill between an append and its mark update (C6c-ctl reproduces only the on-disk state),
and J-Space spill damage while compose never spills (C6d).

## 5. Disclosure

Before the campaign, one smoke run of the harness (REPS=1, on F0v2, rows C2a C6c C6c-ctl C6d C7a
C7b C8a C8b C3a C6a; never C6i) was used to debug it. It showed the same C7a abort. Its results
are not part of the receipt.

## 6. Review additions (fresh-clone review of sc#228, before merge)

Added after review; no code change, receipt untouched.

1. **Stale socket disclosure.** C7a's aborted daemon left a stale socket file in its run folder,
   with no process behind it. It is not recorded in the receipt.
2. **Claim statement.** The claim is at most one intent per authorization (and at most one
   write per intent), or an explicit `UNRESOLVED`; no exactly-once claim. This is inherited from
   ACCEPTANCE-v2 (line 57 "One authorization opens at most one intent, ever", R2 at line 92, and
   line 106 "no receipt or document text claims exactly-once"), which v3 keeps unchanged; v3
   itself has no separate line for it.
3. **Path note.** ACCEPTANCE-v3 cites omega files as `om/...`. Those files live at
   `src/runtime/...` in omega; the line numbers are correct. ACCEPTANCE-v3 line 15 defines the
   shorthand (`om/` = omega `src/runtime/`). The frozen file is not edited; this note records it.
4. **Test limit.** 5 of the 6 cortex_mark integration tests skip under `AIEN_FORCE_CPU_STUB`, so
   on a stub build the mark logic is not exercised by `cargo test`. The proof against the real
   library is the receipt's link proof plus its campaign rows.
