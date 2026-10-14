# #514 chip qualification plan (written before any chip run, 2026-10-14)

Candidate: branch `pkv/505-omega-park-restore` with origin/main merged (commit recorded in
RESULTS.md), stacked on `pkv/503-own30b-serve` (#510, not merged yet: qualified against the
branch as-is). Native release build, omega at the `omega.lock` pin (d0889a94), physics at
omega's `physics.lock`, built through `scripts/pins.sh --gpu` (clean tree, so the build id is
trusted). Model: `~/models/qwen3-30b-a3b-instruct-2507-fp8`, fp8-all weights, fused attention
cache (the live server's path). Every chip process takes `/tmp/aien-gb10.lock` around each
generation. Ports 18098/18099 (18097 is left for the Qwen3-8B worker, 18091 is the live
server and must be down, 18095 is spark-cockpit). Before each run: quiet flag clear, at least
45 GB `MemAvailable` per 30B process (unified 128 GB; `cudaMemGetInfo` under-reports, so
`/proc/meminfo` is the gauge).

Docs read: DGX Spark user guide known issues (unified memory, `nvidia-smi` memory "Not
Supported", `cudaMemGetInfo` may under-report). Concurrent use of one store directory by two
processes: DOCS SILENT (ADR 0003 names one runtime per store directory; the store takes no
cross-process lock), so it is not tested as a supported mode.

## Items (PASS rule in brackets)

1. **Fused-cache export/import round trip.** In-process chip test
   `fused_cache_round_trip_cost_and_restore_latency_on_the_chip`: 2048-token prompt plus 32
   generated tokens; export positions 0..n (A, 5 times: all identical); reset; `restore_kv`(A);
   export again (B). [B == A byte for byte]
2. **Restored-turn token identity.** (a) in-process `restored_turn_two_equals_the_uninterrupted_turn_two_on_the_chip`
   (greedy, seed 1, temperature 0; also the engine K/V after turn 2 byte for byte);
   (b) the server script `own30b-pkv-chip-check.sh` (off/on/park/restore and its other phases).
   [restored turn 2 tokens == uninterrupted turn 2, restores == 1]
3. **Checkpoint cost.** From item 1's test: export time under the GPU lock for the whole
   history (ms, min and median of 5) and bytes; the store's `park_bytes` and `parked_chunks`
   after turn 1. [measured; no threshold, numbers recorded]
4. **NVMe restore latency, cold and warm.** Same test: `fetch` (store lookup, read, decrypt,
   verify) of the parked prefix, 5 times warm (page cache) and 5 times cold
   (`sync; echo 3 > /proc/sys/vm/drop_caches` before each, cold proven by `/proc/self/io`
   `read_bytes` growing by at least the stored bytes); plus `restore_kv` import time.
   [fetched payload == A byte for byte every time; numbers recorded]
5. **Two-process test.** Process 1 (server, :18098) parks turn 1 and stays loaded; process 2
   (second server, :18099, a copy of process 1's store, same key) loads beside it and answers
   turn 2 by restoring; process 1 then answers turn 2 from RAM. [both turn 2 replies ==
   the uninterrupted turn 2 of a persistence-off run; process 2 restores == 1]

## Baseline and negative controls

- Baseline: persistence off (same binary, `AIEN_KV_STORE_DIR` unset), same conversation; every
  comparison is against it. The script's `off` phase and the in-process uninterrupted turn.
- Comparator can say no: in item 1, restore A with one byte flipped, export: must differ from A,
  then restore A again and match.
- Engine refuses a truncated payload (`restore_kv` Err, history empty).
- Corrupted checkpoint refused: in item 4, flip one byte in every stored object, drop caches,
  `fetch` must return nothing with a miss counted; server script `neg-corrupt` phase
  (restores == 0, still answers) and `neg-switch` (another identity, restores == 0).

Anything not measured here is UNVERIFIED in RESULTS.md.
