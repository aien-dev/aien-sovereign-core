# Marker-wait spin window on GB10, Qwen3-4B through the daemon (2026-10-07 07:26-07:33Z, session d6d82f)

Build: aien-cli from this branch (sovereign-core 3c247ab + AIEN_OMEGA_SPIN_US), omega 94a75e2
(omega PR #329: process-wide spin window, default off), physics 6d7cf0d, aienos lock b84c0a6;
has_omega_gpu + has_omega_compose linked. sha256 in identity.txt (binary 61e1de2f...).
Model Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f (Apache-2.0), shard digests in identity.txt.
Script run-ab.sh in this directory. One quietlock hold (d6d82f-spin-ab) 07:26:02Z-07:33:19Z, exit 0.
Evidence type: GPU under Linux. Diagnostic, not a campaign run: the goals are made up and are not
NEXT-PHASE-1 tasks.

## Design
One binary, four daemon starts in one hold, alternating: A (AIEN_OMEGA_SPIN_US unset, omega default 0,
the 50 us sleep-poll), B (2000 us), A2 (unset), B2 (2000 us). Each start: warm-up, three
`compose propose` calls (fruits, colors, garden), shutdown. AIEN_COMPOSE_MAX_TOKENS=64,
AIEN_KV_CONTEXT_TOKENS=4096, AIEN_STEP_LOG=1, greedy. No cache drop, no setting change.
Each daemon log names the window omega reported back ("Omega marker spin: ...").

## Results
Replies are identical in all four runs (same text_sha256 per goal: 16364771..., e4cf5d65..., 726b2bdb...).

| | A (off) | B (2000 us) | A2 (off) | B2 (2000 us) |
|---|---|---|---|---|
| warm-up (1 token, 122 prompt tokens) | 30 871 ms | 28 139 ms | 30 462 ms | 28 081 ms |
| fruits, 9 tokens | 7 627 ms | 5 523 ms | 7 895 ms | 4 967 ms |
| colors, 27 tokens | 12 721 ms | 9 999 ms | 12 762 ms | 9 664 ms |
| garden, 64 tokens (max_tokens) | 23 945 ms | 20 264 ms | 24 350 ms | 19 229 ms |
| decode mean (99 steps, step log) | 309.7 ms/token | 268.7 ms/token | 310.1 ms/token | 267.6 ms/token |
| prompt read mean (3 proposals) | 4 503 ms | 3 014 ms | 4 717 ms | 2 414 ms |

Decode is 13.5% faster with the window, in both pairs; answers do not change.
Omega microbenchmark (omega#328 investigation, same omega commit): fixed cost of one launch 0.103 ms
(sleep-poll) vs 0.007 ms (spin 2000 us), 3/3 runs, every call correct.

## Limits
- Two pairs, one memory state (MemFree about 29-30 GB, Cached about 69 GB), one model.
- Spinning keeps one CPU core busy for up to 2 ms per marker wait (two waits per launch).
- 268 ms/token is still far above the about 90 ms/token a 256-token reply needs inside the 29 s
  skill budget; the matmul kernel (omega#328) remains the main lever.
- The A/B binary is the branch revision before the daemon default existed: there, unset meant omega's
  default 0. Runs B and B2 set 2000 us explicitly, the value the daemon now applies when unset
  (DAEMON_DEFAULT_SPIN_US). Set AIEN_OMEGA_SPIN_US=0 to get run A's behavior.
