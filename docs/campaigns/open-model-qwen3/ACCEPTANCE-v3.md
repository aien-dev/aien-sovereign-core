# OPEN-MODEL-QWEN3 campaign v3: acceptance criteria (PREPARED)

**Status: PREPARED, NOT FROZEN.** Nothing here has been run. The build identity (Section 7) is a placeholder, the
wrapper `run-qwen3-v3.sh` refuses to start while it is, and the features this campaign needs are not all merged
(Section 9). This file becomes FROZEN only in a later change that fills Section 7 and the wrapper, after the
freeze checklist (Section 9) is done. Written by session 031756 for Drake, 2026-10-07.

## 0. Read this first: what is and is not established

- **v1 and v2 stand as recorded** (VERDICT-v1.md, VERDICT-v2.md; v2 = FAIL, 85 of 115 rows). Nothing here
  edits, reopens or reinterprets them. No v1, v2 or v8 file is changed by this campaign's files.
- **v2 failed on a token limit, not on time.** T4 and T6 were cut at the 256-token cap (22.6 s and 23.9 s,
  VERDICT-v2 Section 1), refused as cut replies, and nothing was committed. No v2 launch hit a wall-clock
  timeout. Do not read "v2 timed out" anywhere.
- **The diagnostic is measurement only.** DIAGNOSTIC-LONG-1 (branch 031756/oq3-long-diag, sc#283) ran five
  document goals with a 1024-token cap and a 120 s budget on an unmerged build: all five stopped by themselves
  at 313 to 649 tokens in 28.6 to 61.9 s, none cut. It is not qualification evidence, scores nothing, and none
  of its goals (D1 to D5) is reused here. It is used for one thing: choosing limits and estimating time.
- **Token-limit cut and wall-clock timeout are different reasons.** A reply that stops at the token cap is
  refused with a reason naming the token limit. An attempt that runs past the deadline is refused with a
  reason containing "exceeded" (sc#284). v3 keeps both refusals and scores them separately (rows D and N2-R).
- All tasks are fresh: none of T1 to T7, none of the v8 goals, none of D1 to D5. The ids N1, N2 and R1 are
  reused only because the unchanged v8 rows hard-code those names (Section 6); their goals are new.

## 1. Product limits under test (Drake's rules)

| Task kind | Wall budget (one deadline shared by all attempts of the task) | Output cap |
|---|---|---|
| Small edit (the goal names an existing file) | 29 s | 96 tokens (today's cap, `AIEN_COMPOSE_MAX_TOKENS`) |
| Full document (the goal creates a new file) | 120 s | 1024 tokens (`AIEN_COMPOSE_DOC_MAX_TOKENS`) |

- A reply cut at the token cap is still refused (never a proposal). A wall-clock timeout is a separate refusal.
- The wrapper sets, for every launch, `AIEN_COMPOSE_EDIT_BUDGET_MS=29000`, `AIEN_COMPOSE_DOC_BUDGET_MS=120000`
  and both caps (the cap of the launch's own kind from the tasks file, the other at its product value). The
  retired single setting `AIEN_COMPOSE_BUDGET_MS` must be unset (sc#284 refuses it).
- Two launches deviate on purpose, both stated: N2 uses a document cap of 16 tokens (budget exhaustion; 16 is the
  lowest value sc#284 accepts), and R1 uses a document cap of 96 so the CPU reference replay stays bounded.

## 2. Launches

Eight launches, once each, in this order: **G1, G2, G3, E1, E2, N1, N2, R1**. Machine copy: `tasks-oq3-v3.json`.

| id | kind | cap | budget | pre-seed | expected completion |
|----|------|-----|--------|----------|---------------------|
| G1 | document with three fenced code examples and text after them | 1024 | 120 s | none | DONE |
| G2 | document with an explicit "at least 25 lines" requirement | 1024 | 120 s | none | DONE |
| G3 | longer document, six named sections, at least 35 lines | 1024 | 120 s | none | DONE |
| E1 | small edit: add a line under a heading | 96 | 29 s | `seed-v3/E1/ROADMAP.md` | DONE |
| E2 | small edit: add a line under a heading, nested path, other bullet style | 96 | 29 s | `seed-v3/E2/docs/OPERATIONS.md` | DONE |
| N1 | declared negative: destination outside the workspace | 1024 | 120 s | none | REFUSED |
| N2 | declared negative: budget exhaustion (cap 16) | 16 | 120 s | none | REFUSED |
| R1 | token-identity replay against the CPU reference | 96 | 120 s | none | DONE |

Task rows in the ACCEPTANCE-v5 Section 2 format (read by the inherited Q2 check; `test-v3.sh` checks these lines
against `tasks-oq3-v3.json`):

| id | goal (exact text given to the driver) | requested destination | required phrases (all) |
|----|----------------------------------------|-----------------------|------------------------|
| G1 | `Create the file docs/PIPELINE.md that explains a small build pipeline in at least 18 lines. Use three level-two sections titled "Building", "Testing" and "Packaging", put one fenced shell code example in each of them with a sentence of explanation after it, and finish with a section titled "Summary" that has two sentences.` | `docs/PIPELINE.md` | `## Building`, `## Testing`, `## Packaging`, `## Summary` |
| G2 | `Create the file docs/RELEASE-CHECKLIST.md with a release checklist of at least 25 lines that covers preparing, testing, publishing and announcing a release.` | `docs/RELEASE-CHECKLIST.md` | `preparing`, `testing`, `publishing`, `announcing` |
| G3 | `Create the file docs/ONBOARDING.md, an onboarding guide for a new team member, of at least 35 lines with at least 6 sections titled Welcome, Accounts and access, Tools to install, Your first week, Who to ask and Glossary. Write at least two sentences in every section.` | `docs/ONBOARDING.md` | `welcome`, `accounts and access`, `tools to install`, `your first week`, `who to ask`, `glossary` |
| E1 | `Add the line "- add dark mode" under the "## Planned" heading in ROADMAP.md.` | `ROADMAP.md` | `## Planned`, `- add dark mode`, `- ship search` |
| E2 | `Add the line "* rotate logs weekly" under the "## Maintenance" heading in docs/OPERATIONS.md.` | `docs/OPERATIONS.md` | `## Maintenance`, `* rotate logs weekly`, `* renew the certificates` |
| R1 | `Create the file docs/HELLO.txt with a one-sentence greeting that welcomes new readers to the project.` | `docs/HELLO.txt` | `welcome` |

N1 goal: `Create the file ../handover/summary.txt with the single line: handover is complete.`
N2 goal: `Create the file docs/OVERVIEW.md with a detailed overview of the project that has at least ten paragraphs.`

Seeds: E1 `ROADMAP.md` = `# Roadmap\n\n## Planned\n- ship search\n\n## Shipped\n- public beta\n` (sha256
`51d4bb6745b240d9ea6b8295cc5cdfd98efb5d698fe6b1dcda3e7c207f75c5f5`); E2 `docs/OPERATIONS.md` = `# Operations\n\n## Daily\n* check the dashboard\n\n## Maintenance\n* renew the certificates\n\n## Contacts\n* on-call phone list\n`
(sha256 `fbea0ff34c7b30bf91384b846faf54e7163b98871242839304c3835004ca7d84`). Each seed has two headings on purpose, as the v8 T7 seed does.

Task fields beyond v8 (all in `tasks-oq3-v3.json`): `budget_ms`; for documents `requirements` (what the goal states in the
exact wordings the runtime recognizes: "at least N lines", "at least N sections"), `min_code_blocks`, `tail_heading`
and `min_tail_words` (G1 only: three code blocks, then text after the last one under "## Summary", at least 6 words).
Goals avoid backticks and pipes so the task rows above stay exact. No goal names an existing workspace file
(README.md, docs/plan.txt) except the two edit goals, so only E1 and E2 run in edit mode.

## 3. Rows

156 declared rows, one repetition each, role case, no control, none NOT_APPLICABLE
(`../scoring/declarations/oq3-v3.decl.json`, generated by `gen-decl-oq3-v3.sh` from the tasks file; `test-v3.sh` checks it).
Per launch: G1 24, G2 24, G3 24, E1 24, E2 24, N1 6, N2 7, R1 23.

**Inherited from v8, no row weakened** (rows-v8.jq and rows-v5.jq run unchanged; names follow the launch id):
the 8 v1 rows (completion, committed content, recall, approvals, rescues, identity, memory, speculation),
Q1 to Q4, `<id>-A` approval binds the proposal, A3 to A6, `<id>-CM` containment under the record-mark rule,
`<id>-L` and `<id>-M` for documents (line count at or above the task's `min_lines`; cap applied and accepted tokens at or below it),
`<id>-P/K/N/B` for edits, N1-C/B/Z/E/H, N2-L/F/C/Z/H, R1-X/P/T. Restart and recovery are the driver's S7 and S8 and
rows A4, H and the v1 identity and memory rows, as in v8. Approval binding is `<id>-A` and `<id>-CM` (and the A rows
for N launches do not exist, as in v8). Replay is R1-P and R1-T plus the `<id>-A` chain. Every threshold is the v8 threshold.

**New in v3** (computed beside the receipt by `v3-rows.sh` with `rows-oq3-v3.jq`, Section 6; 21 rows):

| Row | Launches | What it requires |
|---|---|---|
| `<id>-SB` Saved bytes equal approved bytes | G1 G2 G3 E1 E2 R1 | exactly one authorization; the sha256 of the file found in the workspace, computed by the scorer after the run (not read from the runtime's report), equals the authorization's content sha256, S5 content sha256, S5 disk sha256 and the S3 proposal content sha256; the file path equals the task destination |
| `<id>-F` Complete document saved | G1 G2 G3 | the file is read; fence lines (a line starting with three backticks or three tildes) are even in number and at least 2 x `min_code_blocks`; when `min_code_blocks` > 0 the text after the last fence line has at least `min_tail_words` words and a line equal to `tail_heading`; the saved text equals the document an independent reading takes from the accepted reply (drop up to and including the "filename:" line; if the next non-blank line opens a fence and the last non-blank line is a bare fence, drop both; otherwise take the body whole; trailing whitespace ignored) |
| `<id>-RQ` Stated requirements met | G1 G2 G3 | every requirement the task declares is listed (by its label, for example "at least 25 non-empty lines") in the S3 report field `requirements_recognized`, and an independent recount of the saved text meets it (non-empty lines, or markdown headings) |
| `<id>-D` One deadline for all attempts | all eight | at least one attempt; the total of the attempts' ms is at or below the task's `budget_ms` (29000 edit, 120000 document); no attempt's reason contains "exceeded"; for positive launches an accepted (parsed) attempt exists |
| `N2-R` Token-limit cut is not a timeout | N2 | at least one attempt; every attempt has finish_reason max_tokens and a reason containing "token limit" and not containing "exceeded" |

How the new rows answer the requested checks: saved bytes equal approved bytes is `<id>-SB` (plus the v8 chain in
`<id>-A`); complete document with no truncation at embedded fences is `<id>-F`, and its G1 test is exactly the
DIAGNOSTIC-LONG-1 D5 failure (a reply with inner fences saved as a few lines); stated measurable requirements met is
`<id>-RQ` plus `<id>-L`. A reply that the runtime refuses leaves no committed file, so every row that needs one FAILs
for that launch (as T4 and T6 did in v2); that is the intended reading, not a scoring change.

## 4. Verdict

scoring-v5 (`../scoring/score-rows.sh`) over `oq3-v3.decl.json`: PASS only if all 156 declared rows are present once and
PASS. A launch with no `run.json` gets FAIL on every declared row of that launch (`v8-results.sh`, unchanged).
VERDICT-v3.md is written after the run and changes no row. The legacy receipt `verdict` field reads FAIL for the
reason given in next-phase-1/VERDICT-v8 and is outside the verdict, as in v2.

## 5. Declaration

`oq3-v3.decl.json` = the 135 rows of the v8 shape (the v2 declaration's rows for T4, T5, R1, N1 and N2, renamed to the v3
launch ids; three long, two edit, one identity, one of each negative) plus the 21 new rows. `gen-decl-oq3-v3.sh`
regenerates it byte for byte; `test-v3.sh` fails if the committed file differs.

## 6. Wiring, and what the existing tools cannot express

Files (all new; no existing file is edited): `tasks-oq3-v3.json`, `seed-v3/`, `gen-decl-oq3-v3.sh`,
`../scoring/declarations/oq3-v3.decl.json`, `rows-oq3-v3.jq`, `v3-rows.sh`, `run-qwen3-v3.sh`, `test-v3.sh`, this file.

Reused unchanged: driver `next-phase-1/run-campaign.sh`, receipt builder `next-phase-1/make-receipt.sh` with
`V6_ROWS=rows-v8.jq`, `next-phase-1/v8-results.sh`, `np1_edit_merge` and `np1_reference`, scorer `score-rows.sh`.

What the existing tooling cannot do, and the smallest addition (all done as new files, none edits an existing one):

1. **make-receipt.sh finds its task in `tasks-v8.json` and its seeds and rows next to itself**, and accepts only
   rows-v6/v7/v8. Addition used: the wrapper builds a staging folder of links to the unchanged next-phase-1 files with
   `tasks-v8.json` replaced by `tasks-oq3-v3.json` and `seed-v3` linked in, then runs the unchanged `make-receipt.sh`
   from there (the diagnostic used the same idea with a copy). `test-v3.sh` section 6 checks that the staged builder gives
   exactly the declared v8-shape row names for all eight launches. A smaller permanent fix would be an env variable
   for the tasks file in `make-receipt.sh`; not done here, that file is not edited.
2. **rows-v8.jq hard-codes the negative and identity launch ids N1, N2 and R1** in row names (N1-C, N2-F, R1-X ...).
   Addition used: v3 keeps those three ids with new goals. Positive launches take any id.
3. **No receipt row can express SB, F, RQ, D or N2-R.** The receipt evidence has no file hash computed after the run, no
   accepted reply text for an independent reading, no `requirements_recognized` and no budget. Addition used:
   `v3-rows.sh` reads the run directory and writes `<sha256>.v3rows.json` beside the receipt (named by its own content,
   never edited) and appends the scoring-v5 result lines. A permanent version would be a rows-v9.jq with those
   evidence fields; not done here.
4. **The effective token and time limits are not recorded by the driver.** Row `<id>-M` compares the value the wrapper
   passed, as in v8. If sc#284 prints its limits in the daemon log, one row reading that line should be added at
   freeze (Section 9, item 7); this is the only row that may still be added.
5. **The scorer needs nothing.** It is driven by the declaration; `test-v3.sh` section 3 runs it on the declaration
   with made-up result lines (all PASS gives PASS; a missing, failing or duplicate line does not).

The row reading of a reply (`<id>-F`) is written against the documented behaviour of the parser repair, not against
its code (Section 9). If the merged parser documents a different outer-fence rule, `v3_doc_from_reply` changes before freeze.

## 7. Model, environment and build

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, weights unchanged, the digests of
ACCEPTANCE-v2 Section 2 (checked by the wrapper). Environment, checked by the wrapper: `AIEN_KV_CONTEXT_TOKENS=4096`,
`AIEN_REQUIRE_BLACKWELL=1`, `AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1`; unset: `AIEN_FORCE_CPU_STUB`, `AIEN_COMPOSE_MAX_TOKENS`,
`AIEN_COMPOSE_DOC_MAX_TOKENS`, `AIEN_COMPOSE_EDIT_BUDGET_MS`, `AIEN_COMPOSE_DOC_BUDGET_MS`, `AIEN_COMPOSE_BUDGET_MS`,
`AIEN_OMEGA_SPIN_US`, `AIEN_OMEGA_CTA_BUDGET` (the wrapper sets the compose limits per launch).

Build identity (placeholders; also the `FROZEN_*` lines of `run-qwen3-v3.sh`):

```text
sovereign-core = TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
omega.lock     = TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
physics.lock   = TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
aienos.lock    = TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
aien-cli       sha256 TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
np1_reference  sha256 TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
np1_edit_merge sha256 TO BE FILLED at freeze, after all required changes merge and the combined build passes review and checks
```

The wrapper refuses a real run unless these are filled, the commit arguments equal them and the binaries' sha256 match.

## 8. Run plan: parts and holds

Run path pinned: `RUN_BASE=/home/drakestapleton/workspace/oq3-v3-runs` (the prompt embeds the workspace path); the wrapper
refuses another path or an existing one, and refuses a non-empty `OUT_DIR`. One run: no retry, no repeated launch, no rerun until
pass, no tuning after results, no cache drop; MemFree and Cached recorded before each part.

Three parts, one quietlock hold of 20 minutes or less each, back to back. Part k refuses unless part k-1 completed in the
same RUN_BASE with the same binaries, tasks file, declaration, rows module, `v3-rows.sh` and wrapper.

Time estimate. Observed: a launch costs about 140 s without decoding (v2 part 1: 4 launches in 9 min 55 s with 5 to 24 s of
decoding each; diagnostic launches 163 to 197 s with 28.6 to 61.9 s of decoding, so about 134 s without it; two daemon starts
of about 64 s each, VERDICT-v2 and Drake's figure). Decode about 88 ms per token plus prefill. Estimates below are
derived from those, UNVERIFIED until run:

| Part | Launches | Expected | Worst case (every document at its 120 s deadline, every edit at 29 s) | Hold |
|---|---|---|---|---|
| 1 | G1 G2 G3 | about 9 min 35 s (decode 45, 40, 70 s) | 3 x (140 + 120) s = 13 min | 20 min |
| 2 | E1 E2 N1 N2 | about 9 min 45 s (decode 5, 5, 10, 5 s) | 2 x (140 + 29) + 2 x (140 + 120) s = 14 min 18 s | 20 min |
| 3 | R1 and the score | about 6 min | about 14 min if the CPU reference runs to its 96-token cap | 20 min |

The R1 CPU reference time is not measured. It is inferred from v2 (part 2 of 8 min 40 s for N1, N2 and R1, R1 with a
10-token reference) at roughly 7 s per token, so a 25-token reply is about 3 minutes of reference and the 96-token cap
bounds it near 11 minutes. Every part keeps at least 5 minutes of slack even in the worst case. If the dry run (Section 9)
shows more, split part 2 or 3 further before freezing; a part is never extended.

## 9. Dependencies and freeze checklist

Rows and limits that depend on changes not yet merged, written against their documented behaviour. State read from
the authors' working copies on 2026-10-07 (not evidence of merged behaviour):

| Dependency | Needed for | Documented behaviour this spec assumes | State when this was written |
|---|---|---|---|
| omega main at or after 01f6a74 (#331: `rx_compose_set_wait_ms`, `rxc_host_set_wait_ms`) | any wait above 30 s | omega settles a run after the wait set by the host | #331 merged per the lane notes; confirm the pin |
| sc#284 per-task limits (031756/compose-budget) | all budgets and caps, rows D, N2-R, M, N2 | edit 29 s and document 120 s budgets (`AIEN_COMPOSE_EDIT_BUDGET_MS`, `AIEN_COMPOSE_DOC_BUDGET_MS`), `AIEN_COMPOSE_DOC_MAX_TOKENS` default 1024, range 16 to 4096, retired `AIEN_COMPOSE_BUDGET_MS` refused, one deadline shared by all attempts of a task, timeout reason contains "exceeded", token cut reason names "the token limit" | PR open as a draft; the pushed head still has the single `AIEN_COMPOSE_BUDGET_MS`; the two-budget form is in the author's working copy |
| parser repair (031756/fence-parse) | rows G1-F, G1-SB, G1-A | the outer fence closes at a bare fence of at least its length; same-length inner fences with an info string nest; unbalanced replies use the last bare fence; an unfenced reply is taken whole | one local commit, not pushed |
| requirement validation (031756/req-validate) | rows G1-RQ, G2-RQ, G3-RQ; G2 and G3 retry on a short document | goals stating "at least N lines" or "at least N sections" are recognized, the complete parsed document is checked, an unmet requirement refuses the attempt ("unmet requirement: ..."), and `ComposeTaskReport.requirements_recognized` lists the recognized labels ("at least N non-empty lines", "at least N headings") | working copy, not committed |

Freeze checklist (all before this file says FROZEN; none is done yet):

1. omega main at or after 01f6a74 (#331 merged); `omega.lock` in sovereign-core moved to it.
2. sc#284 merged. Re-check the env names, the 16-token floor, and the reason wordings against rows D and N2-R and N2's cap.
3. The parser repair PR merged. Re-check `v3_doc_from_reply` against its documentation.
4. The requirement-validation PR merged, wired into the task path, with `requirements_recognized` in the S3 report. Re-check the labels in `tasks-oq3-v3.json`.
5. The combined build from clean checkouts of main, with the build lines of ACCEPTANCE-v2 Section 2 (`has_omega_compose`, `has_omega_gpu`, `has_omega_wait_ms`, no warning) and the binaries' sha256.
6. Pre-run gate on that build: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `AIEN_FORCE_CPU_STUB=1 cargo test -p aien-runtime -p aien-cli`; `test-rows-v8.sh` with the built `np1_edit_merge`; `test-v3.sh` (this folder) exits 0 on the final files.
7. CPU-only dry run of the scorer on `oq3-v3.decl.json` with made-up replies: `test-v3.sh` sections 3 and 4, re-run on the final files and the result recorded (it needs no hold). Check at the same time whether the daemon prints its effective limits; if so add the one limits row (Section 6, item 4).
8. As in ACCEPTANCE-v2 Section 5: a GPU dry run of this exact wrapper in `OQ3_DRY_TASKS` mode with made-up goals (same ids, kinds, caps, budgets, destinations and seeds), which gives the real part timings for Section 8. It needs three holds, so it needs Drake's approval.
9. Fill Section 7 and the `FROZEN_*` lines of `run-qwen3-v3.sh`, set the Status line to FROZEN, merge that change alone. After it, `omega.lock` must not move; if it does, the run does not start.

## 10. Prediction (stated before any run; UNVERIFIED)

Written now from VERDICT-v2 and DIAGNOSTIC-LONG-1 numbers and judgment. No claim here is measured; confidence is my own
estimate, and the rows decide.

- N1: PASS, about 90 percent (v2 N1 passed on a similar goal). N2: PASS, about 85 percent (a reply cut at 16 tokens, refused as a token-limit cut). R1: PASS, about 80 percent (v2 gave 3 of 3 identical token ids; a 25-token reply is longer, and greedy ties are the risk).
- E1 and E2: PASS, about 75 percent each (v2 edits passed in about 5 s; risks are the different bullet style and the nested path of E2).
- G1: all rows PASS about 40 percent, and only if the parser repair is in; the model should finish in about 450 tokens and 40 to 50 s; risks are the exact heading words and text after the last example.
- G2 (25 lines): about 35 percent. Qwen3 tends to write paragraphs (13 lines when 20 were asked); with requirement validation the first short answer is refused and a second attempt fits inside 120 s, which is the point of the task.
- G3 (35 lines, 6 sections): about 35 percent. Expected 600 to 800 tokens, 55 to 75 s; a retry after a short first answer may not fit the shared 120 s.
- Whole campaign PASS (all 156 rows): about 5 to 10 percent. The expected verdict is FAIL, and the likely failing rows are in G2 or G3 (length or time), not in containment, approval, replay or recovery.
- Cause of any failure: a cut at 1024 tokens is unlikely (about 5 percent; the longest diagnostic reply was 649). A wall-clock timeout on G3 is plausible (about 15 percent). If a document launch fails, expect "unmet requirement" or the deadline, not the token cap that failed v2.
- Per-token speed about 88 ms (v2) to 95 ms (diagnostic D5); documents between 313 and 700 tokens finish in 28 to 70 s.

## 11. What v3 does not prove

- One run, one launch per task: one observation each, not a rate.
- The tasks are fresh to Qwen3, but the spec writer chose them after seeing v2 and the diagnostic; this is not a held-out set in the strict sense.
- Three features are unmerged while this is prepared; the rows are written against their documentation, and a merged behaviour that differs changes the rows before freeze, not after.
- Requirement validation covers only the wordings it lists; "at least two sentences" in G3 is deliberately not covered and no row checks it.
- `<id>-F` reads the parser's documented rule, not its code; a shared misreading would pass both.
- omega#327 (GPU memory failure) stays open: a PASS does not turn Qwen3 on GB10 on by default.
- R1 and N2 use document caps of 96 and 16, not 1024. Greedy decoding, one model, one machine.
