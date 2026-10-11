# WHOLE-SYSTEM-E2E, RUN-3-GB10 (declared, GB10 native Omega backend, attempt 2): result

Previous attempt: RUN-2-GB10 (`runs/RUN-2-GB10-20261011T030554Z/`, `RESULT-RUN-2-GB10.md`), verdict FAIL, kept as
recorded. On RUN-2 every GB10 effect-machinery row passed with fallback 0. The FAIL came from a text-matching rule:
E3m's topic word was matched exactly and case-sensitively. RUN-3-GB10 is a new attempt under the rule pre-registered in
`ACCEPTANCE-v1.3.1-e3m.md` (sc#416, written after RUN-2 showed the mismatch, committed before the harness change and
before this run). Rows: that file over `ACCEPTANCE-v1.3-gb10.md`, `-v1.3-ctrl-e5.md`, `-v1.2.md`. Contract: FROZEN at
aien-architecture b2400fe, sha256 `9e3d5f4e23d65ac17be8f2f4ca48355f0aaa486f67ff5a8a8044dcdd646d34e8`.

Harness `scripts/whole_system_e2e.sh` v1.3.1 at 352c53d (merged main, includes sc#412 objective identity and sc#418
provenance default), `WSE2E_BACKEND=gb10`, started detached 2026-10-11 04:54:50 UTC, rows 04:56:56 to 05:00:10 UTC,
harness exit 0, never killed. Run folder `runs/RUN-3-GB10-20261011T045450Z/` (17 receipts, `chain/SHA256SUMS`,
`VERDICT_ROWS.txt`, `GB10_SUMMARY.json`, eight daemon logs, acceptance files as run, verifier outputs). Host `spark-b87b`,
NVIDIA GB10, driver 580.173.02, pins omega 6c6180c, physics 6d7cf0d, aienos b84c0a6, CAND-4 (Llama 3.2 1B Instruct).
`sha256sum -c chain/SHA256SUMS` from the committed tree: 0 bad lines. **No program PASS is claimed.**

**Verdict: FAIL.** E4 (memory across a restart) failed: the model's edit of `report.md` was refused three times by
the daemon's proposal rules, so the second objective produced no effect. E3m passes under the v1.3.1 rule.

## Rows as measured (OBSERVED)

`gpu` is `gpu_executions` for the row; `fb` is `fallback_count`; a dash means no model call.

| row | status | gpu | fb | what the receipt says |
|---|---|---|---|---|
| PRE | PASS | - | 0 | native Omega backend line; model digests equal the candidate record; zero-CUDA gate exit 0 |
| CTRL-E3a | PASS | - | 0 | no desk key: daemon exited 1 before serving, names `NoDesk` |
| E1 | NOT_RUN | - | 0 | nothing installed (R8) |
| E2 | PASS | - | 0 | objective recorded through `aien-cli`; record under the harness's `AIEN_PROVENANCE_DIR`, which daemon and CLI share (one environment list) |
| CTRL-E2 | PASS | - | 0 | `EXTERNAL_IRREVERSIBLE`: exit 1, `ForbiddenEffectClass`, no record |
| E3 | PASS | 82014 | 0 | DONE; 146 words; names present; spent once. Report byte-identical to RUN-1 and RUN-2 |
| E3m | PASS | 36382 | 0 | DONE; report byte-identical to RUN-2's; D4 by `d4_rule` "case-insensitive whole word" (`Windmill` in the heading); spent once |
| E4 | **FAIL** | 121070 | 0 | `remember` ok; SIGKILL, restart (`Reconcile: checked 0 unsettled`); second objective: attempt 1 refused "edit reply changes nothing in the file", attempts 2 and 3 refused `path "./report.md" is not a plain relative path`; no effect |
| E6 | PASS | 20706 | 0 | SIGKILL after authorize; no write while dead; same grant DONE once; third execute `AlreadySpent` |
| CTRL-E3b | PASS | 30740 | 0 | revoked before spend: `Revoked`; `ctrl.md` absent |
| NET | PASS | - | 0 | 0 established TCP sockets |
| CTRL-E4b | RECORDED | 142070 | 0 | memory files removed: propose refused (`./report.md` path rule again); report unchanged. Not judged |
| CTRL-E6 | PASS | 30580 | 0 | home removed: same grant refused `E_MARK_TRUNCATED`; no second effect |
| CTRL-E4 | PASS (harness) | - | 0 | store removed: refused `E_MARK_TRUNCATED`; report unchanged. Verifier: NOT_RUN (below) |
| E5, CTRL-E1 | NOT_RUN | - | 0 | R7, R8 |
| CTRL-E5 | PASS | - | 0 | byte 3 of `artifacts/E3-report.md` changed on a copy: rejected `DIGEST_MISMATCH`, chain broken, id 00aa251c to 2e4fc8ef; judged by the verifier |

GB10 rules, OBSERVED: `fallback_detected` false, 0 fallback lines over all 8 daemon logs, every model attempt's
`native_fallbacks` 0; all 7 serving daemons printed the native Omega line and `STRICT strict=true
dev_fallback_build=false`; the no-key start exited at the desk check as required. Load average 1.0 to 1.2, `MemFree`
about 29 GB, `MemAvailable` about 97 GB at the receipts; no GPU memory refusal in any log (omega#327 not seen).

Independent verifier (`tools/aien-verify` 16d45e7e..., unchanged, Lane E): chain unbroken, 0 chain problems.
Final `verdict_id` `9ca9b7af6f9255fc7247acae33492768252d5f2017b40c29c159e04fbca833d5` (covers the CTRL-E5 receipt);
before CTRL-E5 `00aa251c0f9e805ed944100d7498c6d326e70dcc22e9a57cf065b86f3d7421f6`
(`artifacts/VERIFIER_VERDICT.pre-ctrl-e5.txt`). Contract rows: E2, E3, E6, CTRL-E2, CTRL-E3, CTRL-E5, CTRL-E6 PASS; E4
FAIL; CTRL-E4, E1, E5, CTRL-E1 NOT_RUN. Two status disagreements, recorded:

1. E3m: harness PASS, verifier FAIL (`D4 ... ["windmill"]`). Expected and pre-registered in `ACCEPTANCE-v1.3.1-e3m.md`:
   the verifier still matches E3m topics exactly.
2. CTRL-E4: harness PASS, verifier NOT_RUN ("E4 did not show a recall, so the control tests store loss only, not
   memory loss"). The verifier's rule is stricter than the harness's; the verifier's reading stands for the contract.

## What follows (INFERRED)

1. The GB10 effect machinery is now shown twice (RUN-2, RUN-3) with fallback 0 and identical model output for E3 and
   E3m: same prompt, same binary family, greedy decoding gave byte-identical reports on the chip.
2. E4's FAIL is the model's proposal under the daemon's edit rules, not the chip, the harness or a budget: the calls
   took 3 to 5 s each, no fallback, and the refusals are named by the daemon. On CPU (RUN-1) the starting report was
   different text (see RESULT-RUN-2-GB10) and the model answered `filename: report.md`; here it answered once with an
   unchanged file, then twice with `./report.md`, which the daemon's plain-relative-path rule refuses.
3. Whether a `./` prefix should be accepted is a product decision about the proposal rules, not a matching defect in
   the harness. It is not changed here.

## Not claimed

Program PASS; memory recall on the chip (E4 failed, CTRL-E4 not judged by the verifier); an installed artifact (E1,
CTRL-E1); cross-machine agreement (E5); numerical equality of GB10 and CPU arithmetic; prose quality beyond the checks.

## Limits

One host, one attempt under v1.3.1, shared machine (load in every receipt). The verifier was built from the tree it
verifies; independence comes from R7. The compose home is in the job's temporary folder; the run folder's artifacts
are the evidence.
