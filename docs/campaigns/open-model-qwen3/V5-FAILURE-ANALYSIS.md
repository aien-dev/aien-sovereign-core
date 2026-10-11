# OPEN-MODEL-QWEN3 v5: failure analysis (all 72 failing rows)

Written 2026-10-11 by Lane B (session 61b069 program, owning issue aien-sovereign-core#384, Drake ruling 2 on
aien-architecture#190). Read-only analysis of the frozen v5 record. It changes no v5 file, no row, no threshold and no
verdict: **v5 stays FAIL, 262 of 334 declared rows** (VERDICT-v5.md, `oq3-v5-score.json`, merged in sc#365 as f83de08).

Every claim below is tagged OBSERVED (read from a named file or re-counted from it), MEASURED (a new measurement made for
this analysis, with its command) or UNVERIFIED (judgement, with a confidence).

## 1. Result

| Class | Meaning | Rows |
|---|---|---|
| (a) model-output weakness | the model's own reply broke a rule the goal stated, so the product refused it before any write | **70** |
| (b) runtime defect | the product did something wrong | **0** |
| (c) prompt or chat-template limitation | the runtime formatted the prompt or the stop tokens wrongly | **0** |
| (d) resource or budget failure | token cap, time budget or memory | **2** (D6-DEC, D6-MEAS) |
| (e) verifier or scorer defect | the checker counted wrongly | **0** |
| total | | **72** |

All 72 rows sit in three launches: D1 (23), D2 (25), D6 (24) (OBSERVED, `oq3-v5-score.json`: 72 rows with verdict not PASS,
all with prefix `D1/`, `D2/` or `D6/`). Each launch has one root event; every other failing row of that launch follows
from it, because the rows read the saved file and the commit chain, and a refused reply leaves neither.

No class (b), (d)-harness or (e) product bug was demonstrated, so step 2 of the lane (regression test and fix PR per bug)
produced **no fix PR**. Section 4 lists what was checked to rule each class out.

## 2. Root event per launch (OBSERVED, `evidence-v5/s3-report-<id>.json`, `proposal_attempts`)

| Launch | Attempt | finish | tokens / cap | ms / budget left | Product refusal reason (verbatim, shortened only where marked) |
|---|---|---|---|---|---|
| D1 | 1 | eos | 542 / 1024 | 50 789 | `unmet requirement: at least 22 non-empty lines, found 8` |
| D1 | 2 | eos | 532 / 1024 | 53 470 | `unmet requirement: at least 22 non-empty lines, found 8` |
| D2 | 1 | eos | 443 / 1024 | 45 179 | `unmet requirement: at least 12 words of plain text in the section "Wrap Up", the section was not found; ... missing "Wrap Up" (each must be a markdown heading line)` |
| D2 | 2 | eos | 455 / 1024 | 46 901 | same as attempt 1 |
| D6 | 1 | eos | 1012 / 1536 | 103 819 | `unmet requirement: the headings "Bringing Her Home", missing "Bringing Her Home" (each must be a markdown heading line)` |
| D6 | 2 | none | none | 66 219 of 66 180 left | `model proposal exceeded 66180 ms` |

(Token counts are `decoding.greedy_tokens`; VERDICT-v5 quotes the receipt's `tokens`, one lower: 541, 531, 442, 454, 1011.)

What the model wrote (OBSERVED, re-counted from the attempt `text` with grep, independent of the product's checker):

- **D1**, both attempts: a title, the three required section headings, and **one long paragraph per section**: 8 non-empty
  content lines after the `filename:` line (9 counting it). The goal asked for at least 22 non-empty lines. The other D1
  rules were met by the refused replies (all three headings present; `whetstone` 3 times, `angle` 2 to 3 times). The retry
  prompt quoted the refusal reason; greedy decoding produced the same structure again.
- **D2**, both attempts: the three example sections are `##` headings in the right order with three complete fenced code
  blocks, but the closing section is written as a plain label line (`Wrap Up: This bash script ...` in attempt 1,
  `Wrap Up:` then a paragraph in attempt 2), not a heading. The goal asked for "a final section titled "Wrap Up"", and the
  model used headings for the other three titled sections in the same reply.
- **D6**, attempt 1: no markdown heading line at all; `Bringing Her Home` is the first content line as plain text, followed
  directly by the first paragraph (about 7 paragraph blocks, so the paragraph rule was close to met). Attempt 2 started with
  66 180 ms of the 170 000 ms task budget left and was cut by the wall clock after 66 219 ms with no tokens recorded.

## 3. Classification per row

Class (a) for 70 rows: the one root cause is the model's reply (rows `D1-RQ1`, `D2-RQ4`, `D6-RQ2` name the broken rule;
the other 67 need an accepted reply, a saved file, an approval, a commit or a generation record, none of which can exist
after a refusal). Class (d) for 2 rows: `D6-DEC` and `D6-MEAS` fail because attempt 2 timed out, and ACCEPTANCE-v5
Section 3.4 declares that a timed-out attempt (no `decoding`, no `tokens`, no `token_ids`) fails them. They would fail
with any outcome of attempt 1, once attempt 2 timed out.

| Launch | Row | Class | Why it failed |
|---|---|---|---|
| D1 | `v1 Task completion` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `v1 Correctness: committed content` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `v1 Human interventions: approvals` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `v1 Memory` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `Q1` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `Q2` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `Q3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `Q4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `A3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `A4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `A5` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-L` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-M` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-A` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-CM` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-SB` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-F` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-CUT` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-RQ1` | a | the stated rule the reply broke: 8 non-empty lines, at least 22 required |
| D1 | `D1-RQ2` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-RQ3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-D` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D1 | `D1-GR` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `v1 Task completion` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `v1 Correctness: committed content` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `v1 Human interventions: approvals` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `v1 Memory` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `Q1` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `Q2` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `Q3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `Q4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `A3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `A4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `A5` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-L` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-M` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-A` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-CM` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-SB` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-F` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-CUT` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-RQ1` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-RQ2` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-RQ3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-RQ4` | a | the stated rule the reply broke: "Wrap Up" written as a plain label line, not a heading |
| D2 | `D2-RQ5` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-D` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D2 | `D2-GR` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `v1 Task completion` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `v1 Correctness: committed content` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `v1 Human interventions: approvals` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `v1 Memory` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `Q1` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `Q2` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `Q3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `Q4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `A3` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `A4` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `A5` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-L` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-M` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-A` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-CM` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-SB` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-F` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-CUT` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-RQ1` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-RQ2` | a | the stated rule attempt 1 broke: "Bringing Her Home" written as a plain line, not a heading |
| D6 | `D6-D` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-DEC` | d | attempt 2 timed out (66 219 ms of the remaining 66 180 ms); a timed-out attempt has no `decoding` (ACCEPTANCE-v5 3.4) |
| D6 | `D6-GR` | a | no reply accepted, so no saved file, approval, commit or generation record exists to read |
| D6 | `D6-MEAS` | d | attempt 2 timed out; a timed-out attempt has no `tokens` or `token_ids` |

## 4. What was checked to rule out classes (b), (c), (d) and (e)

**(c) prompt and chat template: none.**
- The pinned template equals the upstream `chat_template` of Qwen/Qwen3-4B-Instruct-2507 at revision cdbee75f (sha256
  64f85b19... in `crates/aien-inference-abi/fixtures/chat-templates/qwen3-4b-instruct-2507/PROVENANCE.md` and ACCEPTANCE-v5
  Section 7), and the test `rendering_matches_the_reference_jinja_rendering` (`crates/aien-inference-abi/tests/qwen3_chat_template.rs`)
  compares the product's rendering with the reference jinja rendering (OBSERVED, files on main de9c626).
- The 2507 Instruct model has no thinking mode, and no reply contains a think tag (OBSERVED, attempt texts).
- All five attempts that produced tokens ended with `finish_reason` `eos` (OBSERVED, table in Section 2): the stop set
  (`generation_config.json` `eos_token_id` [151645, 151643]) was honoured and no reply ran on or stopped early.
- The prompt carries the goal verbatim (`spine::proposal_prompt`: `Goal: {goal}` plus the fixed answer format) and the retry
  carries the refusal reason verbatim (`spine::retry_prompt`) (OBSERVED, `crates/aien-runtime/src/spine.rs` on de9c626, both
  functions unchanged since the v5 build 30258fc).

**(b) runtime: none.**
- On D1, D2 and D6 the rows that test the product all PASS: `<id>-PRE` (no requirement refused before the model), `<id>-OPR`
  and `<id>-BE` (GB10 native path, no fallback), `<id>-PIN`; the refusal path committed nothing (VERDICT-v5 Section 2).
- R1: the GB10 run and the CPU reference produced the same 20 token ids for the same prompt (VERDICT-v5 Section 3).
- The refusal reasons agree with independent counts of the reply text (Section 2).

**(d) resources and budgets: 2 rows, no harness defect.**
- No token cut: every reply that finished stopped at `eos`, well under its cap (largest 1012 of 1536).
- No memory event: NVRM kernel log line count unchanged across all four parts; MemAvailable above 110 GB (VERDICT-v5 Section 3).
- D6 attempt 2 is a time-budget failure. Its cause is the product's documented retry rule, not a defect: attempt k > 1 starts
  when at least `COMPOSE_DOC_ATTEMPT_BUDGET` = 30 s is left (`spine.rs`, comment: "a retry that has to write a whole document
  needs about the shortest measured one: 28.6 s"), and it gets the rest of the task budget. D6's first attempt took 103.8 s
  for 1012 tokens, so a second essay of that length could not finish in the 66.2 s left. Changing this rule (skip a retry that
  cannot finish, or give it more time) is a product design choice for a later decision; it would change at most `D6-DEC`
  and `D6-MEAS`, never the D6 verdict, because attempt 1 was already refused for the missing heading. Recorded here, not changed.

**(e) verifier and scorer: none among the 72.**
- The product's counts (8 lines; no `Wrap Up` heading; no `Bringing Her Home` heading) equal independent grep counts of the
  same text (Section 2).
- The three extra N1 lines (`N1/N1-C`, `N1/N1-B`, `N1/N1-H`) are **not** among the 72. They are inherited rows that ACCEPTANCE-v5
  Section 3.3 replaced and declared as extras before the run; the scorer lists them under `extras` and does not count them,
  as declared. This is a cosmetic tooling note (the receipt still computes rows it no longer needs), not a scoring defect.

## 5. What this means for a rerun (input to QUALIFICATION-v6.md)

- No product change merged since v5 targets the model's formatting of line counts or headings (UNVERIFIED, confidence 0.8:
  read from the commit titles 30258fc..de9c626; the requirement reader gives the same result for all eleven goals on de9c626 as
  on b2cae6d, MEASURED, `evidence-v6/requirements-probe-de9c626.txt`). Decoding is greedy, so the same prompt gives the same
  reply. The prompt embeds the workspace path, so a new run base changes a few prompt tokens and the replies may differ
  slightly. Expected: D1, D2 and D6 fail again for the same reasons (UNVERIFIED, confidence 0.8).
- **A rerun on the CPU reference path cannot pass these rows** (MEASURED 2026-10-11 with the v5 build's `np1_reference`, D1
  goal, 16 and 80 output tokens: 51 852 ms and 140 771 ms, so about 1 389 ms per output token on `ReferenceCpuBackend`, against
  about 100 ms per token on GB10 in v5 D6). A 540-token D1 reply would need about 12.5 minutes against a 120 s budget, and the
  product refuses any budget above 599 s. In addition, row `<id>-BE` requires the `OmegaGb10` backend on every daemon start
  (`rows-oq3-v4.jq`), so every launch would fail it on CPU. A CPU-only qualification would need new rows and budgets, which is
  a new campaign and a Drake decision, never a silent change.
