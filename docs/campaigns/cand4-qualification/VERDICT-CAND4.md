# CAND-4 qualification verdict

Acceptance: `ACCEPTANCE-CAND4.md`, frozen in commit 130beeb before any run (sc d5b78ff, omega c0369e6,
aienos b84c0a6, physics 6d7cf0d, manifest aien-architecture 7f99d7e). Binaries: the CAND4-BUILD-REAL3
double build (aien-cli ad6b7eb5, 25 of 25 identical); cpu-fault 3ba0a8e6 built here from the frozen sc.
The pinned digests were checked in the parent run before the first round (`receipts/run-qual.out` line 9: "all pinned
digests equal the frozen values"; `refuse_unless_digests` exits 3 on a mismatch) and again after every run by Q2w
(14 of 14 OK). Harness defect, disclosed: each round runs as a fresh child process (`bash "$0" _q1_round` /
`_q2_fixture`) and `inputs.sh` resets `DIGESTS='[]'` in the child, so the 9 Q1 attempt records carry
`digests_checked_before_round: []` and the q2-fixture and q2-cases records carry `digests: []`. No per-round
re-check inside the child is recorded; the parent check and the post-run check are the evidence. Receipts: `receipts/`,
each named by the sha256 of its content. No launch was repeated.

**VERDICT: PASS** (digests PASS, hygiene PASS, Q1 PASS, Q2 PASS, Q2w PASS), with the limits below.

| part | result | evidence |
|------|--------|----------|
| digests | PASS | parent check before round 1 (`receipts/run-qual.out`:9) and Q2w post-run check 14/14; per-round records empty (defect, see above) |
| hygiene H1, H1c, H2, H2c, H3, H4, H4c, H5 | PASS; H6 reports `AIEN_DEV_FALLBACK` | `receipts/hygiene/hygiene-754c729f...json` |
| Q1 (T1, T2, T3 x 3, GPU) | PASS: 9 of 9 launches and both controls | `receipts/scores/q1-score-d6cc2dcd...json` |
| Q2 (F0 on GPU, cases on cpu-fault, 3 reps) | PASS: 34 rows PASS, C6d NOT_APPLICABLE | `receipts/scores/q2-score-52445cf7...json`; NP2 receipt 64ca5ffe PASS |
| Q2w (cases 2 and 5, harness side) | PASS: 3 controls, W2 x3, W5 | `receipts/scores/q2w-score-22867895...json` |

## Q1 and the v5 receipts (read this)
All 9 v5 receipts (`receipts/q1/`) are FAIL. Each fails only "Containment: workspace" and A1, because
the daemon's own Cortex record mark `compose.cortex-mark` (NEXT-PHASE-2 v3, #228) sits outside the
workspace and the v5 driver, written earlier, counts it as an escape. They are kept unchanged. Under the
frozen rule `q1_a1_record_mark = record-mark` (ACCEPTANCE 4.1) each launch was read a second time
(`receipts/q1-a1/`, 9 of 9 PASS): same failing rows only, the mark the only outside file, sentinel
unchanged, change equal to the authorized path, well-formed mark. Q1 is PASS only under that reading.
The two controls (damaged mark, extra failing row) both rejected as required.

## Limits and what is not claimed
- Case 1 GPU loss, case 3 capability-root revocation, case 6 coordinated rollback: OUT OF SCOPE, not covered.
- Case 4 spill corruption: C6d NOT_APPLICABLE (no spilled data), so NOT COVERED.
- Case 2 only as W2: a SIGKILL while a propose request is pending, no named internal step, no omega hooks.
- Case 5 only non-deterministically (W5: 40 trials, 5 restarts advanced the mark, 0 inconsistent). The
  deterministic variant needs candidate code: NOT COVERED in that form.
- Q2 cases and Q2w run on the CPU build (cpu-fault), not the GPU binary; only F0 and C3-control run on the GPU.
- Qualified configuration: AIEN_COMPOSE_MAX_TOKENS=96 (binary default 48); attempt budget 12000 ms as in
  the code (v5 spec text says 21000 ms); the binary's own 3 proposal attempts stay.
- Llama-3.2-1B-Instruct, internal release only.
- UNVERIFIED: that a real device loss reaches the strict failure path; omega in-settle hooks never used.

## GPU holds (quietlock owner laneQ, UTC, 2026-10-06; flag released after every hold)
- Q1 round 1: 20:29:58 to 20:33:16. Round 2: 20:33:17 to 20:36:36. Round 3: 20:36:38 to 20:39:54.
- Q2 fixture F0: 20:39:55 to 20:40:29.
- Q2 cases and Q2w: CPU only, no hold.
Others in the quiet history after ours: cand4-release 20:46:10 to 20:47:46; laneS-236 21:02:43 to 21:05:06.
No overlap with any laneQ launch. (Hold times from `~/workspace/.spark-quiet.history`.)

## Disclosures from the fresh-clone review (wording only; no receipt or frozen file edited)
- `a1_read` (`run-cand4.sh`) counts only rows whose result is exactly `FAIL`, so an `INCOMPLETE` or `NOT_RUN` row would
  not block the "only the two mark-caused rows fail" condition, and it treats a missing sentinel pair as unchanged
  (`null == null`). Harmless on these receipts (every row is PASS or FAIL, every sentinel pair present; checked in
  review), but it must be hardened before reuse: issue #240.
- `q1.decl.json` line 5 carries a stale note ("No control row ...") that contradicts its own rows; the frozen
  `ACCEPTANCE-CAND4.md` section 4.1 (line 145) governs, and the declaration is not edited after the freeze.
- Rule 4.1 (the record-mark reading) was written after a pre-freeze TRIAL showed the mark failure
  (`ACCEPTANCE-CAND4.md` line 153 discloses this). It was frozen before any evidence run; no post-run
  reinterpretation took place.
- The C6d outcome text in the NEXT-PHASE-2 receipt hard-codes "omega 62b6a28" (inherited from the v4 declaration
  string); the run used omega c0369e6 as pinned. The receipt is not edited.
- The Q2w score (`receipts/scores/q2w-score-22867895...json`) has `results_sha256: null`; the per-row results are in
  `receipts/q2w-results.jsonl` and `receipts/q2w/`.
- This PR's title said "unfrozen draft" while the acceptance was being written; the acceptance was frozen in commit
  130beeb before any run.
