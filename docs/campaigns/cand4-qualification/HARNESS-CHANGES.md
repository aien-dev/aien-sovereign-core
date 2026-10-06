# Harness changes after the CAND-4 run

The CAND-4 receipts, `ACCEPTANCE-CAND4.md`, `VERDICT-CAND4.md` and `receipts/` are unchanged. These
changes apply to the next campaign that reuses this harness (sovereign-core issue #240).

## 2026-10-06: record-mark reading hardened, per-round digest check moved into the child

- `a1_read` moved to `a1-read.sh` (sourced by `run-cand4.sh` and `selftest.sh`).
  - `failing_rows` now lists every row whose result is not `PASS`: `FAIL`, `INCOMPLETE`, `NOT_RUN`
    and a missing result (`MISSING`) all count. Exceptions: a row declared `NOT_APPLICABLE`, and a
    report-only row (`threshold` = `report only`, the v5 Latency and Resource use rows) whose
    result is `REPORTED`. `non_pass_rows` lists each such row with its result.
  - `sentinel_unchanged` requires both values of each sentinel pair to be present non-empty strings
    before comparing them. A missing or null pair is `false`, never "unchanged".
- `_q1_round` and `_q2_fixture` (the child processes that run inside the GPU hold) call
  `refuse_unless_digests` first, so `digests_checked_before_round` in the Q1 attempt records and the
  digest list in the q2-fixture record carry that round's own check instead of an empty list. A
  digest mismatch inside the hold exits 3 before any launch; the parent records the round as a
  launch that left no result line (FAIL, not retried).
- `selftest.sh` gained nine `a1_read` cases (INCOMPLETE, NOT_RUN, missing result, REPORTED on a
  thresholded row, missing / null / changed sentinel, malformed mark, and the mark-only PASS shape).
- Replayed over the nine CAND-4 Q1 receipts, the hardened reader gives the same `failing_rows`
  (`Containment: workspace`, `A1`) and the same five non-mark conditions as the committed
  `receipts/q1-a1/` records (mark files are not in the repository, so the mark condition was excluded
  from the replay).
