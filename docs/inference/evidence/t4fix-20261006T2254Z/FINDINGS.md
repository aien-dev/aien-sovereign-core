# T4 fix verification on GB10, 2026-10-06 ~22:54-23:20Z (quietlock owner t4fix-verify)

Build: sovereign-core sc#247 head 8c5294c (AIEN_OMEGA_CTA_BUDGET setting), omega c0369e6, physics 6d7cf0d, aienos lock b84c0a6; has_omega_gpu + has_omega_compose linked. Script: gb10-t4fix-verify.sh in this dir (P0 parity gate added by orchestrator review).

## P0: prefill-sized shapes at cta 256 (never chip-run before)
106x2048x2048, 106x2048x512, 106x2048x8192, 106x8192x2048: ok_calls 100/100 each, e2e bad 0/0, repeat_mismatch 0 -> parity ok: true. Two rows are marked FAIL only by the test's median < 5 ms speed ceiling (medians 5.989 and 7.222 ms).

## T4, one launch per budget, AIEN_STEP_LOG=1
| | cta 64 (default) | cta 256 |
|---|---|---|
| warm-up (outside B) | 10 508 ms | 9 434 ms |
| T4 prefill | 2 102 ms | 1 660 ms |
| decode mean | 184.7 ms/token (148 steps) | 89.7 ms/token (244 steps) |
| S3 wall (skill, B = 29 000 ms) | 29 094 ms (timeout) | 23 678 ms |
| S4-S6 (commit/check) | FAIL | PASS |
| docs/GUIDE.md | not written | 24 lines, 936 bytes (goal: >= 12 lines) |

## Limits
- One launch each; this is a measurement, not the frozen NP1 v7 run; rows not scored.
- Token identity between cta 64 and 256 not compared (64 timed out); v7 row R1 (GB10 vs CPU reference ids) will check it. UNVERIFIED until then.
- Faster than the worker's 118 ms estimate; why (beyond the matmul medians) UNKNOWN.
