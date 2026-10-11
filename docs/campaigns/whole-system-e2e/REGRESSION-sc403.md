# Regression evidence, sc#403 (CTRL-E5 receipt), Lane E, 2026-10-11

Pre-registered in `ACCEPTANCE-v1.3-ctrl-e5.md` (committed before the code). Verifier built with `tools/aien-verify/build.sh` (rustc only).

| check | result |
|---|---|
| `tools/aien-verify/tests/run.sh` | passed 43, failed 0 (was 34; nine new CTRL-E5 and RUN-1 checks) |
| `rustc --test` unit tests | 2 passed |
| `scripts/whole_system_e2e.sh selftest` | PASS |
| RUN-1-20261010T154724Z, Spark base id `a50fccb6...` as peer | `verdict_id 437467db4149b013738578c10ef68b7168dfb95feae8dea882de17a6118fc5fd`, `base_verdict_id a50fccb63363ef9224a19bd8bf4f249754abf5d88667ba0fd6ce93dfb8d63d80`, CTRL-E5 NOT_RUN (unchanged) |
| Harness tail run against a scratch copy of RUN-1 (daemon-free; the 017 placeholder removed) | pre-CTRL-E5 `verdict_id` a50fccb6...; scratch copy mutated at byte 3 of `artifacts/E3-report.md` (6f to 58): rejected=true, chain_unbroken=false, mutated id starts `cb72e9d9` (as R7 by hand); receipt CTRL-E5 PASS; final verdict `CTRL-E5 PASS`, chain_unbroken true, `verdict_id 02a8f01309fba46776b62e34ae31594429d12d98c823f86bc8cff6c7dc0706a2` |

Limits: the tail was exercised on a scratch copy of RUN-1, not on a fresh full run (a full run needs the daemon and about an hour of CPU
model time and belongs to the next scheduled run). `runs/` was not modified.
