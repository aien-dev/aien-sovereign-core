# SMOLLM2-Q verdict: SmolLM2-1.7B-Instruct on the CAND-4 qualification suite

Acceptance: `ACCEPTANCE-SMOLLM2-Q.md`, frozen in commit 87dabe2 before any run (sc 2e8cef9 = the #246 merge,
omega c0369e6, aienos b84c0a6, physics 6d7cf0d). Binaries: the SMOL2Q-BUILD double build at 2e8cef9
(aien-cli ce2fff7c, 25 of 25 identical; `~/workspace/evidence-out/SMOL2Q-BUILD-A`), cpu-fault b1b4acb0
rebuilt from 2e8cef9. Receipts: `receipts/`, each named by the sha256 of its content; the console output of
every command is in `receipts/console/`. No launch was repeated; two commands were rerun after a refusal
or a harness defect, both disclosed below with the first output kept.

**VERDICT: FAIL** (digests PASS, hygiene PASS, **Q1 FAIL**, Q2 PASS, Q2w PASS).
**Section 9 class: every failing row is class U** (unrelated to memory sizing). **No class M event**
occurred in any SmolLM2 daemon start. Under decision D3 as written: SmolLM2 is not the next open-model
candidate, and **Qwen3-4B is the next model to evaluate** (not started here).

| part | result | evidence |
|------|--------|----------|
| digests | PASS | 19 of 19 pinned digests equal before each Q1 round (9 attempt records), at F0, and after the run (Q2w record) |
| hygiene H1, H1c, H2, H2c, H3, H4, H4c, H5 | PASS | `receipts/hygiene/hygiene-c6d657dd...json` |
| Q1 (T1, T2, T3 x 3, GPU) | **FAIL**: T1 3 of 3 PASS, T2 0 of 3, T3 0 of 3; both section 4.1 controls PASS | `receipts/scores/q1-score-dd8df5ac...json` (scorer exit 1) |
| Q2 (F0 on GPU, cases on cpu-fault, 3 reps) | PASS: 34 rows PASS, C6d NOT_APPLICABLE | `receipts/scores/q2-score-134c0156...json`; NP2 receipt 52c3fbd1 |
| Q2w (cases 2 and 5, harness side) | PASS: 3 controls, W2 x3, W5 (40 trials, 3 restarts advanced the mark, 0 inconsistent) | `receipts/scores/q2w-score-930faba3...json` |

## Q1 failures and their class (section 9)

As in CAND-4, every v5 receipt fails "Containment: workspace" and A1 because of the daemon's own record
mark; the frozen section 4.1 reading (`receipts/q1-a1/`) decides each launch. Under that reading:

| row | reps | what failed | class |
|-----|------|-------------|-------|
| T1 | 3 of 3 PASS | nothing | (none) |
| T2 | 0 of 3 | "Q1 Destination: proposed path" only: the task asks for `docs/CONTACT.txt`, the model proposed `CONTACT.txt` (same in all three runs) | **U** (task quality) |
| T3 | 0 of 3 | every attempt stopped with finish reason `max_tokens` at the 64-token limit (about 11.8 s and 13.2 s per attempt), "reply cut at the token limit"; the model wrote a heading list instead of the requested content | **U** (task quality) |

`max_tokens = 64` was frozen in SmolLM2 campaign v1 from the measured GB10 decode speed, before that run
(section 0); raising it after this result would be tuning after results and is not done. T3 also hit the
limit in v1, so this repeats a known weakness rather than a new one.

## Memory observations (section 8; closes issue #236)

| model | record | KV pool plan line | VmHWM after warm-up | fatal / refusal lines |
|-------|--------|-------------------|---------------------|-----------------------|
| SmolLM2 | `receipts/mem/mem-dae1d9cb...json` | `KV pool: 3221225472 bytes (3.00 GiB) = 512 blocks x 16 tokens x 24 layers x 32 kv heads x 64 head_dim x 2 (K,V) x 4 bytes; context 8192 tokens` (equal to section 8) | 13909672 kB (about 13.9 GB; before #246: 61.1 GB) | none |
| Llama-3.2-1B control | `receipts/mem/mem-3cf3eda8...json` | `KV pool: 8589934592 bytes (8.00 GiB) = 8192 blocks x 16 tokens x 16 layers x 8 kv heads x 64 head_dim x 2 (K,V) x 4 bytes; context 131072 tokens` (equal to section 8) | 17386380 kB (about 17.4 GB; unchanged from v1) | none |

All 18 SmolLM2 daemon starts in Q1 (two per launch, nine launches) and the F0 start carry the same 3.00 GiB
plan line, no fatal line, no KV pool refusal, no driver status 0x51 and no out-of-memory kill; VmHWM about
13.9 GB in each. The memory fix holds for SmolLM2: peak memory fell from about 61 GB to about 14 GB.

## Limits and what is not claimed
- Everything listed under "Limits" in `cand4-qualification/VERDICT-CAND4.md` applies here unchanged
  (cases 1, 3, 6 out of scope; C6d NOT_APPLICABLE so case 4 not covered; case 2 only as W2; case 5 only
  non-deterministically; Q2 cases and Q2w on the CPU build).
- Q2 and Q2w passing says the recovery machinery works with SmolLM2 loaded; it says nothing about
  answer quality, which is what Q1 measures.
- Stated limit from v1: GB10 greedy output can differ from the Hugging Face reference on near-ties.
- NEXT-PHASE-1 v6/v7 not run (section 10).

## GPU holds (quietlock owner laneSmol2, UTC, 2026-10-07; flag released after every hold, exit 0)
- mem (first run): 00:43:38 to 00:44:54.
- Q1 round 1: 00:44:56 to 00:49:36. Round 2: 00:49:38 to 00:54:17. Round 3: 00:54:19 to 00:58:58.
- Q2 fixture F0: 00:59:00 to 00:59:46.
- mem (rerun, fixed harness): 01:26:46 to 01:28:03.
- Q2 cases and Q2w: CPU only, no hold.
The previous hold by another lane (np1v7) was released at 00:05:54, before the first laneSmol2 hold; no
other lane held the GPU between 00:43 and 01:29. (From `~/workspace/.spark-quiet.history`.)

## Disclosures (no receipt or frozen file edited)
1. **mem harness defect, fixed and rerun.** The first `mem` run recorded VmHWM 2716 kB for both models:
   `$!` was the pid of a subshell running the clean-environment function, not the daemon. Commit 79c6fe4
   makes that function `exec` so `$!` is the daemon, and records the measured process's command line in
   each record. This changes the harness after the freeze, for the `mem` command only (observations, not
   verdict rows); the acceptance file is unchanged. The first records (`mem-dee451e4...`, `mem-ac0e928f...`)
   are kept; their plan lines are correct, their VmHWM is not the daemon's. The rerun records
   (`mem-dae1d9cb...`, `mem-3cf3eda8...`) name `aien-cli daemon` as the measured process. Q1 records read
   VmHWM from the daemon's own `run.json` and were not affected.
2. **Q2w first call refused, rerun.** The first `q2w` call was given a separate run directory and refused
   before doing anything ("REFUSED: no F0 under ... (run q2 first)"; `receipts/console/q2w-refused-wrong-base.out`).
   It was rerun against the Q2 run directory and passed. Nothing ran in the refused call.
3. **GPU hold notices stayed local.** The harness's start/release whispers went to the campaign worktree's
   own crumb store, not a shared one, so other sessions did not see them. The quietlock flag itself (the
   real guard) was taken and released normally for every hold, as the history above shows.
4. **"session open attempt" count.** Each Q1 attempt record counts `session_open_attempt_lines = 1` per
   SmolLM2 start; that line is the success line "GPU session: open on attempt 1/3", not a retry. No start
   needed a second attempt.
5. **Q2 cases record carries no digest list**, as in CAND-4; the F0 record before it and the Q2w record
   after it both show 19 of 19.
