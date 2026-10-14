# #514 chip qualification results (GB10, 2026-10-14)

Plan: `PLAN.md` (written before any run). Runner: `run-all.sh` (each step under its own
`quietlock hold --minutes 20`). Ground per run in `runN-*/ground.txt` (commit, clean tree,
binary and test sha256, omega d0889a94, driver 580.173.02, kernel 7.0.0-1019-nvidia).
Model: Qwen3-30B-A3B fp8-all, fused attention cache, chunk 256 tokens.

## Final run: run3, commit 8a4c36e9 (all PASS)

| item | result | numbers |
|---|---|---|
| 1 fused-cache export/import round trip | PASS | 2079 positions, 204,472,320 bytes; export(import(A)) == A byte for byte, 5/5; 5 exports identical |
| 2a restored turn 2 == uninterrupted (in process) | PASS | tokens and the engine K/V byte for byte; restores 1 |
| 2b restored turn 2 == uninterrupted (server script) | PASS | all 28 checks, incl. torn write, late load, late park, neg-switch, neg-corrupt; restored 800 tokens (turn-1 prompt 750) |
| 3 checkpoint cost | PASS (measured) | export under the GPU lock: median 65.3 ms (min 63.9, max 68.3) for 204 MB; 9 chunks parked, 0 refused, 0 failed; on disk 204,509,184 bytes |
| 4 NVMe restore latency | PASS (measured) | fetch (lookup, read, decrypt, verify) warm median 453.6 ms (452.4 to 470.7); cold median 693.5 ms (652.8 to 696.8), cold proven: block reads 204.5 to 204.9 MB per fetch >= 204.5 MB stored; import into the fused cache median 22.4 ms; fetched payload == A every time |
| 5 two processes | PASS | proc 1 parks and stays loaded, proc 2 loads beside it (both resident, MemAvailable 38.2 GB, RSS 37.3 + 33.8 GB) and restores 800 tokens; both turn 2 replies == the persistence-off turn 2 |

Negative controls (run3): one flipped payload byte comes back at exactly its offset (the
comparator can say no); a truncated payload is refused ("kv payload of 204472319 bytes, 2079
positions need 204472320") and leaves an empty engine; every stored object corrupted -> fetch
refused (miss `invalid`); server neg-corrupt and neg-switch restore nothing. Baseline: the
persistence-off server run and the in-process uninterrupted turn.

## Earlier runs (kept, superseded)

- run1 at 7d036ac4: item 1 FAIL. A 2079-token turn parked 2 of 9 chunks (`park_refused=1`): one
  256-token chunk is 25 MB and the storage worker's park budget is 64 MB (a quarter of
  `AIEN_KV_STORE_MAX_INFLIGHT_BYTES`, 256 MB). The server script PASSED its token checks
  while restoring only 512 of 750+ tokens; it did not check for refusals. Fixed in a639c4e9
  (`submit` waits for credit), red-first host test, and the scripts now check
  `park_refused == 0` and restore coverage.
- run2 at a639c4e9: 9/9 chunks parked, round trip PASS; item 4 FAIL: the 204 MB restore was
  refused as `busy` (restore budget 192 MB). Fixed in 8a4c36e9 (`fetch` waits for credit within
  `LOAD_TIMEOUT`), same red-first test extended. Items 2a, 2b, 5 PASS in run2.

## Limits (UNVERIFIED)

- One conversation shape per item; 5 repetitions for timings, not a distribution under load.
- Warm fetch at about 450 MB/s is dominated by decrypt and verify on the CPU (INFERRED; not profiled).
- Two processes on one store folder at the same time: not tested and not supported (no
  inter-process lock, issue #521); item 5 gives process 2 a copy.
- Contexts longer than 2079 tokens, and concurrent requests, were not run on the chip.

CALC-EXEMPT: every number is copied from the run logs (SNAP lines, health JSON, memory.txt); none is derived.
