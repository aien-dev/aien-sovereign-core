# OPEN-MODEL-QWEN3 v7: result

```text
spec          = SPEC-v7-DRAFT.md + frozen-v7.json FROZEN, merged before the run (sc#456, 4a16585; dry G6 PASS record sc#457, a603ad3);
                rows, tasks, limits, budgets and scorer = the v5 files, unchanged (334 declared rows)
verdict       = FAIL (scoring-v5, oq3-v7-score.json): 334 declared rows, 259 PASS, 75 FAIL,
                none malformed, none NOT_APPLICABLE; 3 undeclared N1 extras listed outside the verdict, as in v5 and v6
failing rows  = 75 in four launches: D1 23, D3 27, D6 24, R1 1 (D2, D4, D5, E1, E2, N1, N2: every declared row PASS)
classes       = (a) model output 70, (b) runtime/harness 1, (c) prompt or template 0, (d) budget 4, (e) scorer 0
public bundle = BLOCKED. Qwen/Qwen3-8B is not qualified; the model decision goes back to Drake (ruling 2, arch#190). No CAND-5 manifest.
```

Owning issue: aien-dev/aien-sovereign-core#384. Nothing here changes any v5 file, any row, limit, budget or goal. No launch was repeated.
Every number below is read from the files of Section 6. v6 comparison: RESULT-v6.md (Qwen3-4B-Instruct-2507, 245 PASS, 89 FAIL).

## 1. Run

```text
product build   = sovereign-core a6e5f858d63b3c195ea13b1cc061091d08c6d561 (clean clone + pinned release build ~/workspace/oq3-v7-build-a6e5f85,
                  evidence-v7/build-summary-a6e5f85.txt), omega 6c6180c, physics 6d7cf0d, aienos b84c0a6; pins recorded per part equal
                  frozen-v7.json (row <id>-PIN PASS on all eleven launches)
wrapper         = run-qwen3-v7.sh + run-campaign-allen.sh from sovereign-core main 4a16585 (the freeze commit; frozen-v7.json FROZEN there;
                  test-v7.sh 20 of 20 at that commit, records/test-v7-real.out; launcher sha256 in records/launcher-real.sha256)
model           = Qwen/Qwen3-8B rev b968826d (f32 compute, thinking off), model_sha256 17bc36882b981cdaa253076c43273e3b5e54251db26cb6a5f51de396c456f5a6
                  (index+shards), re-hashed 2026-10-11 (evidence-v7/model-rehash-2026-10-11.txt); daemon CHECKPOINT_SHARDS line = the same
                  (records/model_sha256-line.txt)
environment     = AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1, AIEN_REQUIRE_CHECKPOINT=1; edit cap 96, document cap 1024 tokens,
                  budgets 29000 ms edit / 120000 ms document (the v5 values; hold-p1.log line 1)
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA); R1 reference on CPU; ALLEN engaged by the driver (persona state "default")
gate G6 (dry)   = hold laneB-oq3-v7-dry-p1, 23:54:53Z to 00:08:37Z (2026-10-11/12), rc 0, G6 PASS over D1 D2 D3 (records/g6.txt; attempt 3 of the
                  dry run, attempts 1 and 2 in evidence-v7/attempt-1-hold-refused/ and attempt-2-g6-fail/)
part 1          = hold laneB-oq3-v7-p1, 00:10:05Z to 00:23:48Z 2026-10-12, rc 0 (D1 D2 D3)
part 2          = hold laneB-oq3-v7-p2, to 00:36:45Z, rc 0 (D4 D5 D6)
part 3          = hold laneB-oq3-v7-p3, to 00:45:38Z, rc 0 (E1 E2 N1)
part 4          = hold laneB-oq3-v7-p4, to 00:55:13Z, rc 1 = the scorer's FAIL exit, not a crash (N2 R1 and the score)
GB10 time       = 58 min 52 s in all (dry 13:44, parts 13:43, 12:57, 8:53, 9:35); every hold under its 20 minutes; chip free from 00:55:13Z
NVRM kernel log = line count 1 before and after every hold (records/nvrm-*.txt): no driver event
fallbacks       = none: 47 OP_REPORT lines, native_fallbacks=[] on each, backend "OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)"
```

Coordinator go: Drake, 2026-10-11, "wait for the other agent ... then you can follow that with the hold" (rehearsal, freeze, full campaign);
run from the pinned commit, not re-pinned.

## 2. Result per launch (attempts from `s3-reports/`)

| Launch | Attempts (finish, tokens, ms, outcome) | Committed | FAIL rows |
|---|---|---|---|
| D1 | eos 655 95794 refused "at least 22 non-empty lines, found 14"; no second attempt (24 s of the 120 s budget left, under the 30 s attempt minimum) | no | 23 |
| D2 | eos 354 54737 refused, "Wrap Up" section not found; eos 339 53621 parsed | yes | 0 |
| D3 | eos 425 62488 refused "at least 18 non-empty lines, found 13"; attempt 2 timed out (exceeded the 57511 ms left) | no | 27 |
| D4 | eos 288 54538 parsed | yes | 0 |
| D5 | eos 103 19991 refused "16 lines, found 12", headings missing; eos 194 35642 parsed | yes | 0 |
| D6 | eos 723 101484 refused "Bringing Her Home" not a heading; attempt 2 timed out (exceeded the 68515 ms left) | no | 24 |
| E1 | eos 70 16752 parsed | yes | 0 |
| E2 | eos 92 20968 parsed | yes | 0 |
| N1 | none (refused for its destination before any model call, as required) | no, as required | 0 |
| N2 | three attempts, each cut at the 16-token limit and refused, as required | no, as required | 0 |
| R1 | eos 26 8717 parsed; R1-X PASS, R1-T PASS (GB10 and CPU reference gave the same 26 token ids); R1-P FAIL (prompt token count 164 vs 99) | yes | 1 |

## 3. Why: three separate causes

**(a) the model, as in v5 and v6 (70 rows).** D1, D3 and D6 failed on the model's first reply: D1 wrote 14 non-empty lines where 22 were
asked, D3 wrote 13 where 18 were asked, D6 wrote "Bringing Her Home" as plain text instead of a heading. Each first failing row after the S1
row on these launches is a model row (`RQ`, `CUT`, `F`, `SB`); the method is V5-FAILURE-ANALYSIS.md. The 8B did better than the 4B on D2 and
D4 (both committed; v6 lost 26 and 2 rows there) and the same on D1 and D6; D3 is new as an uncommitted launch (v6 D3 committed on attempt 2).

**(d) the budget (4 rows, plus the shape of D1).** The 8B generates at about 7 tokens per second on this engine (SPEC-v7-DRAFT Section 8,
observed again here: 655 tokens in 95.8 s on D1, 723 in 101.5 s on D6). Under the unchanged 120 s document budget a long first reply leaves
no room for a second attempt: D3 and D6 attempt 2 were ended by the budget (`outcome: timeout`, 0 tokens), so `D3-DEC`, `D3-MEAS`,
`D6-DEC`, `D6-MEAS` are class (d) as `D6-DEC`/`D6-MEAS` were in v6; D1 got no second attempt at all. This is prediction 3 of the spec
(budget rows grow), observed. By ruling 1 the budgets are not moved; a slower model under the same budget is a result, not a defect.

**(b) a harness gap in the CPU reference replay (1 row, `R1-P`).** The driver now engages ALLEN (SPEC-v7 Section 1), and the product puts
the ALLEN style block in front of every task prompt (`aien-runtime/src/persona.rs`, `prefix_prompt`, called from the compose planner).
The GB10 run therefore tokenized 164 prompt tokens (`prompt_ids_sha256 eee4e094...`) while the wrapper's CPU reference replay,
`np1_reference <model> "<goal>" <ws> 96 new`, is handed the bare goal and tokenized 99 (`335c249b...`). The outputs are identical anyway
(26 token ids, reply sha256 `bf760bf2...` on both sides: R1-X and R1-T PASS), so chip parity held; only the "same prompt" row fails, and it
fails because the replay tool was not updated when the driver was. Replay check (evidence-v7/run/r1p-replay/, CPU only, after the run): the same reference tool given the goal with the default persona block
in front (rendered from `aien-allen-profile/src/context.rs`, state "default", no preferences) produces 164 prompt tokens, the chip's count, and the
identical 26-token reply; its token-id digest (`0e6962f3...`) still differs from the chip's (`eee4e094...`), so the exact prefix text the product used
is reproduced in length but not token for token (UNVERIFIED at that level; the run records only the digest, not the prompt). This is a campaign-tooling finding for a later campaign
(the reference replay must prepend the same persona block), not a model result and not rescored here. v6 had 111/111 on this row because
its driver did not engage ALLEN (the v6 class (b) failure).

**The 17 v6 class (b) rows are gone.** `v1 Correctness: recall` passes on every launch that has it, `v1 Task completion` passes on every
committed launch, `N1-RH` and `N2-H` pass: prediction 1 of the spec, observed. Gate G6 (corrected, sc#455) checked every product row of
the dry launches and passed; no product-side row of the real run failed on any committed launch.

## 4. Every failing row and its class

| Launch | Row | Class |
|---|---|---|
| D1 | `v1 Task completion` | a |
| D1 | `v1 Correctness: committed content` | a |
| D1 | `v1 Human interventions: approvals` | a |
| D1 | `v1 Memory` | a |
| D1 | `Q1` | a |
| D1 | `Q2` | a |
| D1 | `Q3` | a |
| D1 | `Q4` | a |
| D1 | `D1-A` | a |
| D1 | `A3` | a |
| D1 | `A4` | a |
| D1 | `A5` | a |
| D1 | `D1-L` | a |
| D1 | `D1-M` | a |
| D1 | `D1-CM` | a |
| D1 | `D1-SB` | a |
| D1 | `D1-F` | a |
| D1 | `D1-CUT` | a |
| D1 | `D1-RQ1` | a |
| D1 | `D1-RQ2` | a |
| D1 | `D1-RQ3` | a |
| D1 | `D1-D` | a |
| D1 | `D1-GR` | a |
| D3 | `v1 Task completion` | a |
| D3 | `v1 Correctness: committed content` | a |
| D3 | `v1 Human interventions: approvals` | a |
| D3 | `v1 Memory` | a |
| D3 | `Q1` | a |
| D3 | `Q2` | a |
| D3 | `Q3` | a |
| D3 | `Q4` | a |
| D3 | `D3-A` | a |
| D3 | `A3` | a |
| D3 | `A4` | a |
| D3 | `A5` | a |
| D3 | `D3-L` | a |
| D3 | `D3-M` | a |
| D3 | `D3-CM` | a |
| D3 | `D3-SB` | a |
| D3 | `D3-F` | a |
| D3 | `D3-CUT` | a |
| D3 | `D3-RQ1` | a |
| D3 | `D3-RQ2` | a |
| D3 | `D3-RQ3` | a |
| D3 | `D3-RQ4` | a |
| D3 | `D3-RQ5` | a |
| D3 | `D3-D` | a |
| D3 | `D3-DEC` | d |
| D3 | `D3-GR` | a |
| D3 | `D3-MEAS` | d |
| D6 | `v1 Task completion` | a |
| D6 | `v1 Correctness: committed content` | a |
| D6 | `v1 Human interventions: approvals` | a |
| D6 | `v1 Memory` | a |
| D6 | `Q1` | a |
| D6 | `Q2` | a |
| D6 | `Q3` | a |
| D6 | `Q4` | a |
| D6 | `D6-A` | a |
| D6 | `A3` | a |
| D6 | `A4` | a |
| D6 | `A5` | a |
| D6 | `D6-L` | a |
| D6 | `D6-M` | a |
| D6 | `D6-CM` | a |
| D6 | `D6-SB` | a |
| D6 | `D6-F` | a |
| D6 | `D6-CUT` | a |
| D6 | `D6-RQ1` | a |
| D6 | `D6-RQ2` | a |
| D6 | `D6-D` | a |
| D6 | `D6-DEC` | d |
| D6 | `D6-GR` | a |
| D6 | `D6-MEAS` | d |
| R1 | `R1-P` | b |

Totals: (a) 70, (b) 1, (d) 4 = 75. Class (a) includes `v1 Memory`, `v1 Task completion`, `v1 Correctness: committed content`,
`v1 Human interventions: approvals` and the Q and A rows of D1, D3 and D6, which fail only because nothing was committed, exactly as
D1 and D2 in v6; the committed-group gate rule of sc#455 is the same reading.

## 5. What this means

- **Qwen/Qwen3-8B is not qualified at these budgets. The public bundle stays blocked** pending Drake's model decision (ruling 2: a FAIL
  blocks the bundle; no silent substitution; no threshold moves). No CAND-5 manifest is drafted.
- Model side: 3 of 6 document tasks still fail on the first reply (line counts, heading form); the 8B is better than the 4B (259 PASS
  against 245, D2 and D4 now commit) but too slow under the 120 s document budget for a second attempt on long documents. Options for a
  later campaign, each a new frozen spec and a Drake call: a faster path for the same model (int8 engine, D4 of the post-overnight
  decisions, prospective spec), a different model, or a prompt that shows the model its own refused reply.
- Harness side: the CPU reference replay must prepend the persona block the product uses, before any later campaign reuses R1-P.
- Containment held: no refused reply was written; N1 refused for its destination, N2 for the token cut; R1 GPU and CPU token identity
  held; no fallback; no NVRM event.

## 6. Evidence (`evidence-v7/run/`)

`out/` (receipts `<sha>.json`, `.summary.txt`, `.v5rows.json`, `oq3-v7-results.jsonl` 337 lines = 334 declared + 3 extras,
`oq3-v7-score.json`, committed `replies/`, `INDEX.md`), `dry-out/` (G6, never scored), `s3-reports/` (one per launch),
`records/` (hold start, end, rc and log per hold including the dry hold, memory and NVRM counts per hold, launch records
`<id>.launch-v5.json`, pins, identity and meminfo per part, `g6.txt`, `g6-reasons.txt`, `test-v7.out`, `test-v7-real.out`,
`launcher-dry.sha256`, `launcher-real.sha256`, `model_sha256-line.txt`), `run-base-SHA256SUMS` (all 908 files of
`~/workspace/oq3-v7-runs`; daemon logs, run directories and `R1.reference.json` stay there). `r1p-replay/` (the after-the-run CPU replay of Section 3, goal text, result without ids, prompt ids, timing).

## 7. Limits

One run, one launch per task, greedy decoding, one model, one machine, GB10 at the pins, f32 compute. The legacy receipt `verdict` field
is outside the verdict. Release readiness is not claimed. The class (b) reading of `R1-P` rests on the code path and the replay named
above, not on a rerun of the campaign. omega#327, omega#277 and sc#277 stay open and separate.
