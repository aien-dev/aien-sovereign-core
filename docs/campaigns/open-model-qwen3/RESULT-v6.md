# OPEN-MODEL-QWEN3 v6: result

```text
spec          = QUALIFICATION-v6.md, FROZEN, merged before the run (sc#409, f8d8c72); rows, tasks, limits and scorer = the v5 files
verdict       = FAIL (scoring-v5, oq3-v6-score.json): 334 declared rows, 245 PASS, 89 FAIL,
                none malformed, none NOT_APPLICABLE; 3 undeclared N1 extras listed outside the verdict, as in v5
failing rows  = 89 in all eleven launches: D1 24, D2 26, D6 25, D3 2, D4 2, D5 2, E1 2, E2 2, R1 2, N1 1, N2 1
classes       = (a) model output 70, (b) runtime/harness 17, (c) prompt or template 0, (d) budget 2, (e) scorer 0
public bundle = BLOCKED. Qwen3-4B-Instruct-2507 is not qualified; the model decision goes back to Drake (ruling 2, arch#190).
```

Owning issue: aien-spark/aien-sovereign-core#384. Nothing here changes QUALIFICATION-v6.md, any v5 file, any row, limit or goal.
No launch was repeated. Every number below is read from the files of Section 6.

## 1. Run

```text
product build   = sovereign-core de9c626ad9fd50652c1f767d273b4d63f118e342 (clean build ~/workspace/oq3-v6-build-de9c626), omega 6c6180c,
                  physics 6d7cf0d, aienos b84c0a6; pins recorded per part equal frozen-v6.json (row <id>-PIN PASS on every launch)
wrapper         = run-qwen3-v6.sh from a clean clone of sovereign-core main f8d8c72 (test-v6.sh 20 of 20 at that commit, records/test-v6.out)
model           = Qwen/Qwen3-4B-Instruct-2507 rev cdbee75f, model_sha256 17a78fbb...976b7 (index+shards), re-hashed 2026-10-11
environment     = AIEN_KV_CONTEXT_TOKENS=4096, AIEN_REQUIRE_BLACKWELL=1, AIEN_REQUIRE_CHECKPOINT=1, AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1
evidence type   = GPU under Linux (GB10, native Omega engine, no CUDA); R1 reference on CPU
gate G6 (dry)   = hold laneB-oq3-v6-dry-p1, 03:19:43Z to 03:27:03Z 2026-10-11, exit 0, G6 PASS (D1 to D3: PRE, OPR, BE, PIN all PASS)
part 1          = hold laneB-oq3-v6-p1, 03:27:03Z to 03:34:30Z, exit 0 (D1 D2 D3)
part 2          = hold laneB-oq3-v6-p2, to 03:42:29Z, exit 0 (D4 D5 D6)
part 3          = hold laneB-oq3-v6-p3, to 03:45:46Z, exit 0 (E1 E2 N1)
part 4          = hold laneB-oq3-v6-p4, to 03:49:53Z, exit 1 = the scorer's FAIL exit, not a crash (N2 R1 and the score)
GB10 time       = 30 min 10 s in all (dry 7:20, parts 7:27, 7:59, 3:17, 4:07); chip free from 03:49:53Z
NVRM kernel log = line count 20 before and after every hold (records/nvrm-*.txt): no driver event
```

Coordinator go: session 61b069, 2026-10-11, "run v6 on GB10 now", run from the pinned commit, not re-pinned.

## 2. Result per launch (attempts from `s3-reports/`)

| Launch | Attempts (finish, tokens, ms, outcome) | Committed | FAIL rows |
|---|---|---|---|
| D1 | eos 542 51968 refused "at least 22 non-empty lines, found 8"; eos 513 51040 same | no | 24 |
| D2 | eos 472 47921 refused "Wrap Up" not a heading; eos 455 50864 same | no | 26 |
| D3 | eos 399 36769 refused "18 lines, found 11"; eos 329 32697 parsed | yes | 2 |
| D4 | eos 308 40081 refused "13 lines, found 5"; eos 431 55490 parsed | yes | 2 |
| D5 | eos 139 15756 refused, headings missing; eos 185 21631 parsed | yes | 2 |
| D6 | eos 1069 113189 refused "Bringing Her Home" not a heading; attempt 2 timed out (56849 of 56810 ms left) | no | 25 |
| E1 | eos 71 9748 parsed | yes | 2 |
| E2 | eos 93 13127 parsed | yes | 2 |
| N1 | none (refused for its destination before any model call, as required) | no, as required | 1 |
| N2 | three attempts, each cut at the 16-token limit and refused, as required | no, as required | 1 |
| R1 | eos 21 3882 parsed; R1-X, R1-P, R1-T PASS (GB10 and CPU reference gave the same token ids) | yes | 2 |

## 3. Why: two separate causes

**(a) and (d), the model, as in v5 (72 rows).** D1, D2 and D6 failed exactly as in v5: D1 wrote one long paragraph per section (8 lines
where 22 were asked), D2 wrote "Wrap Up" as a label instead of a heading, D6 wrote "Bringing Her Home" as plain text and its second
attempt ran out of the 170 s task budget. Greedy decoding reproduced the v5 behaviour on the newer product; each first failing row of
D1, D2 and D6 after the S1 row is a model row (`RQ`, `CUT`, `F`, `SB`). Classification method as V5-FAILURE-ANALYSIS.md.

**(b) a product change the frozen driver predates (17 rows).** On every launch the driver's step S1 (remember the project constraint)
was refused by the product: `ALLEN is not engaged on this machine, so there is no identity to attach memory to. Nothing was changed.
(not_engaged)` (verbatim, `steps/S1.json` of every launch; the run base is listed in `run-base-SHA256SUMS`). Since sc#386 (merged
2026-10-10 as #391, in de9c626 but not in the v5 build 30258fc) compose memory needs an engaged ALLEN identity; the WHOLE-SYSTEM-E2E
harness was updated to engage a fixture subject, the frozen next-phase-1 driver used by v5 and v6 was not. With no stored
constraint, `v1 Correctness: recall` fails on all nine launches that have the row, `v1 Task completion` (8 of 8 steps) fails on the
six launches that committed (D3, D4, D5, E1, E2, R1), and `N1-RH` and `N2-H` (the S1 constraint recalled after the restart) fail. These 17 rows passed in v5.
This is a campaign-tooling incompatibility with an intended product change, not a model result; by the binding rules it is a finding
for a later campaign and is not rescored here.

**Gate G6 did not catch it.** The dry run showed the same S1 refusal (dry D1 `v1 Correctness: recall` FAIL), but the G6 rule written in
QUALIFICATION-v6 Section 7 and the launcher checked only PRE, OPR, BE and PIN. That gate was too narrow: recorded as a finding of this
spec, not hidden. It does not change the verdict: without those 17 rows the result would still be FAIL on D1, D2 and D6 (UNVERIFIED as a
run outcome; it is the reading of rows that do not depend on S1).

## 4. Every failing row and its class

| Launch | Row | Class |
|---|---|---|
| D1 | `A3` | a |
| D1 | `A4` | a |
| D1 | `A5` | a |
| D1 | `D1-A` | a |
| D1 | `D1-CM` | a |
| D1 | `D1-CUT` | a |
| D1 | `D1-D` | a |
| D1 | `D1-F` | a |
| D1 | `D1-GR` | a |
| D1 | `D1-L` | a |
| D1 | `D1-M` | a |
| D1 | `D1-RQ1` | a |
| D1 | `D1-RQ2` | a |
| D1 | `D1-RQ3` | a |
| D1 | `D1-SB` | a |
| D1 | `Q1` | a |
| D1 | `Q2` | a |
| D1 | `Q3` | a |
| D1 | `Q4` | a |
| D1 | `v1 Correctness: committed content` | a |
| D1 | `v1 Correctness: recall` | b |
| D1 | `v1 Human interventions: approvals` | a |
| D1 | `v1 Memory` | a |
| D1 | `v1 Task completion` | a |
| D2 | `A3` | a |
| D2 | `A4` | a |
| D2 | `A5` | a |
| D2 | `D2-A` | a |
| D2 | `D2-CM` | a |
| D2 | `D2-CUT` | a |
| D2 | `D2-D` | a |
| D2 | `D2-F` | a |
| D2 | `D2-GR` | a |
| D2 | `D2-L` | a |
| D2 | `D2-M` | a |
| D2 | `D2-RQ1` | a |
| D2 | `D2-RQ2` | a |
| D2 | `D2-RQ3` | a |
| D2 | `D2-RQ4` | a |
| D2 | `D2-RQ5` | a |
| D2 | `D2-SB` | a |
| D2 | `Q1` | a |
| D2 | `Q2` | a |
| D2 | `Q3` | a |
| D2 | `Q4` | a |
| D2 | `v1 Correctness: committed content` | a |
| D2 | `v1 Correctness: recall` | b |
| D2 | `v1 Human interventions: approvals` | a |
| D2 | `v1 Memory` | a |
| D2 | `v1 Task completion` | a |
| D3 | `v1 Correctness: recall` | b |
| D3 | `v1 Task completion` | b |
| D4 | `v1 Correctness: recall` | b |
| D4 | `v1 Task completion` | b |
| D5 | `v1 Correctness: recall` | b |
| D5 | `v1 Task completion` | b |
| D6 | `A3` | a |
| D6 | `A4` | a |
| D6 | `A5` | a |
| D6 | `D6-A` | a |
| D6 | `D6-CM` | a |
| D6 | `D6-CUT` | a |
| D6 | `D6-D` | a |
| D6 | `D6-DEC` | d |
| D6 | `D6-F` | a |
| D6 | `D6-GR` | a |
| D6 | `D6-L` | a |
| D6 | `D6-M` | a |
| D6 | `D6-MEAS` | d |
| D6 | `D6-RQ1` | a |
| D6 | `D6-RQ2` | a |
| D6 | `D6-SB` | a |
| D6 | `Q1` | a |
| D6 | `Q2` | a |
| D6 | `Q3` | a |
| D6 | `Q4` | a |
| D6 | `v1 Correctness: committed content` | a |
| D6 | `v1 Correctness: recall` | b |
| D6 | `v1 Human interventions: approvals` | a |
| D6 | `v1 Memory` | a |
| D6 | `v1 Task completion` | a |
| E1 | `v1 Correctness: recall` | b |
| E1 | `v1 Task completion` | b |
| E2 | `v1 Correctness: recall` | b |
| E2 | `v1 Task completion` | b |
| N1 | `N1-RH` | b |
| N2 | `N2-H` | b |
| R1 | `v1 Correctness: recall` | b |
| R1 | `v1 Task completion` | b |

## 5. What this means

- **Qwen3-4B-Instruct-2507 is not qualified. The public bundle stays blocked** pending Drake's model decision (ruling 2: a FAIL blocks the
  bundle; no silent substitution). No CAND-5 manifest is drafted.
- Model side (unchanged from v5): on long or strictly structured documents this model, under this product's prompt and greedy decoding,
  does not follow line-count and heading-form instructions (3 of 6 document tasks fail on both attempts; the other 3 pass only on
  their second attempt). Options for a later campaign, each a new frozen spec and a Drake call: a different model, a prompt or retry
  that shows the model its own refused reply, sampling, or a larger model.
- Runtime side: the next-phase-1 driver must engage an ALLEN fixture subject before S1, as the WHOLE-SYSTEM-E2E harness does since
  sc#386, before any later campaign reuses it; and a later G6 must check every product-side row of the dry launches, not four.
- Containment held: no refused reply was written; N1 was refused for its destination, N2 for the token cut; R1 GPU and CPU token
  identity held; no fallback; no NVRM event.

## 6. Evidence (`evidence-v6/run/`)

`out/` (receipts `<sha>.json`, `.summary.txt`, `.v5rows.json`, `oq3-v6-results.jsonl` 337 lines = 334 declared + 3 extras,
`oq3-v6-score.json`, committed `replies/`, `INDEX.md`), `dry-out/` (G6, never scored), `s3-reports/` (one per launch),
`records/` (hold start, end, exit and log per hold, memory and NVRM counts per hold, launch records `<id>.launch-v5.json`, pins,
identity and meminfo per part, `g6.txt`, `test-v6.out`, `launcher.sha256`), `run-base-SHA256SUMS` (all 486 files of
`~/workspace/oq3-v6-runs`; daemon logs and run directories stay there).

## 7. Limits

One run, one launch per task, greedy decoding, one model, one machine, GB10 at the pins. The legacy receipt `verdict` field is outside
the verdict. Release readiness is not claimed. omega#327, omega#277 and sc#277 stay open and separate.
