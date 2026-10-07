# OPEN-MODEL-QWEN3 campaign v3: acceptance criteria (FROZEN)

**Status: FROZEN** when this file is on main (merged before the run). Section 7 names the merged commits and the
binaries' sha256, the pre-run gate passed on that build (Section 9), and `run-qwen3-v3.sh` refuses unless the commit
arguments and the binaries match. Nothing below is a verdict; VERDICT-v3.md is written after the run. Prepared by
session 031756 for Drake, 2026-10-07; frozen 2026-10-07. **Read Section 12 first: it records what the merged code does
differently from what Section 2 and Section 3 assumed. Every `<id>-RQ` row is now outcome-based (decision recorded in
Section 12), which settles the G3-RQ problem it describes.**

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
| Small edit (the goal names an existing file) | 29 s | 96 tokens (`AIEN_COMPOSE_MAX_TOKENS`, set by the wrapper; the daemon default is 48, `server.rs:483-492`) |
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
exact wordings the runtime recognizes: "at least N lines", "at least N sections"; G3 also carries the six `titles` its goal names, used only by row G3-RQ), `min_code_blocks`, `tail_heading`
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
| `<id>-F` Complete document saved | G1 G2 G3 | the file is read; fence lines (a line starting with three backticks or three tildes) are even in number and at least 2 x `min_code_blocks`; when `min_code_blocks` > 0 the text after the last fence line has at least `min_tail_words` words and a line equal to `tail_heading`; the saved text equals the document an independent reading takes from the accepted reply by the merged parser rule (Section 12: the "filename:" line dropped; a backtick opener dropped and the outer fence closed by the CommonMark-style nesting rule, text after the close dropped; otherwise the body whole; trailing whitespace ignored) |
| `<id>-RQ` Stated requirements met | G1 G2 G3 | **outcome-based** (Section 12): the file saved at the destination meets every measurable requirement the task's goal states, computed by the campaign tooling from the task's declared `requirements` fields, whether or not the product recognized them: non-empty lines (fence lines included) at or above `n`, and for headings at least `n` markdown headings outside fenced code, each declared title (G3: Welcome, Accounts and access, Tools to install, Your first week, Who to ask, Glossary) among them. Nothing saved is FAIL. The product's `requirements_recognized` is recorded as information in the side-file (`value.recognized_by_runtime`), not as a condition |
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
   passed, as in v8. Checked at freeze against merged sovereign-core 8f3e8c8: the daemon does not print its effective
   limits (`server.rs:100-103` only refuses bad values), so no limits row is added and none can be.
5. **The scorer needs nothing.** It is driven by the declaration; `test-v3.sh` section 3 runs it on the declaration
   with made-up result lines (all PASS gives PASS; a missing, failing or duplicate line does not).

The row reading of a reply (`<id>-F`, `v3_doc_from_reply` in `rows-oq3-v3.jq`) was re-written at freeze from the merged
parser (`check_file_proposal` and `outer_fence_close`, `spine.rs`, sovereign-core 8f3e8c8): backtick openers only, the
nesting rule, and text after the outer close dropped (Section 12). Both sides still read the same rule, so a shared
misreading would pass both (Section 11).

## 7. Model, environment and build

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, weights unchanged, the digests of
ACCEPTANCE-v2 Section 2 (checked by the wrapper). Environment, checked by the wrapper: `AIEN_KV_CONTEXT_TOKENS=4096`,
`AIEN_REQUIRE_BLACKWELL=1`, `AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1`; unset: `AIEN_FORCE_CPU_STUB`, `AIEN_COMPOSE_MAX_TOKENS`,
`AIEN_COMPOSE_DOC_MAX_TOKENS`, `AIEN_COMPOSE_EDIT_BUDGET_MS`, `AIEN_COMPOSE_DOC_BUDGET_MS`, `AIEN_COMPOSE_BUDGET_MS`,
`AIEN_OMEGA_SPIN_US`, `AIEN_OMEGA_CTA_BUDGET` (the wrapper sets the compose limits per launch).

Build identity (frozen; also the `FROZEN_*` lines of `run-qwen3-v3.sh`):

```text
sovereign-core = 8f3e8c8b879e10dd83883cee150f16508284d643  (main: sc#285 a667276, sc#284 94c8c20, sc#287 8f3e8c8; binaries built here)
omega.lock     = 01f6a74636b8383b010cdb95597839582c415c27  (omega main, omega#331 rx_compose_set_wait_ms)
physics.lock   = 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf  (omega/physics.lock at 01f6a74)     aienos.lock = b84c0a67590a934f3f3e001b12ec85ebc086a9eb
aien-cli       sha256 689027ea9ac09f52ec130a2d0b0310995b7539113ccd3a63b06f1037ad2cf4de
np1_reference  sha256 df91eede1223f454b7c6afd47589aac6406c8e69e393018fdbe3377b2103f686
np1_edit_merge sha256 abc345a47e872728e8b59d050ccf41794c78266e19da882ad6e3d1db681c6377
```

Built from clean checkouts (no local changes, `git status` empty) in `/home/drakestapleton/workspace/oq3-v3-build`:
`sc` at 8f3e8c8 (the main parent of the campaign branch's merge commit; the campaign branch only adds files under
`docs/campaigns/`, so the binaries' source is main's), `omega` at 01f6a74 (compose and GPU libraries both built from it),
`physics` at 6d7cf0d, aienos lock repo `/home/drakestapleton/workspace/aienos-repo` (contains b84c0a6). Environment:
`AIEN_OMEGA_DIR`, `AIEN_PHYSICS_DIR`, `AIEN_AIENOS_LOCK_REPO` set; no `AIEN_OMEGA_GPU_LIB` / `AIEN_OMEGA_COMPOSE_LIB` override.
Commands: `cargo build -vv --release -p aien-cli` and `cargo build -vv --release -p aien-runtime --example np1_reference
--example np1_edit_merge` (`evidence-v3/build.sh`). Both build logs show `cargo:rustc-cfg=has_omega_compose`,
`cargo:rustc-link-lib=static=rx_compose`, `cargo:rustc-cfg=has_omega_gpu`, `cargo:rustc-link-lib=static=omega_gpu` and
`cargo:rustc-cfg=has_omega_wait_ms`, and no `cargo:warning` line (`evidence-v3/build-summary.txt`; the only compiler
warnings are dead-code and lifetime lints inside the third-party crates memchr, rustix and the workspace's own
existing lints, the same kind v2 had).

The wrapper refuses a real run unless the commit arguments equal these, and `chk` compares the three binaries' sha256
to them before the first launch. The wrapper, tasks file, declaration and rows the run uses are the ones on main once this
file is merged; that merge changes only files under `docs/campaigns/`, so the binaries' source is unchanged. If
`omega.lock` on main moves before the run, the run does not start.

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

## 9. Dependencies and freeze record

All four required changes are merged; the rows and limits were re-checked against the merged code (Section 12).

| Dependency | Merged as | Re-checked against the merged code |
|---|---|---|
| omega `rx_compose_set_wait_ms` (#331) | omega main 01f6a74 = `omega.lock` | the build log shows `has_omega_wait_ms` (Section 7) |
| two budgets and caps (sc#284) | sovereign-core 94c8c20 | env names, defaults, ranges and refusal wording confirmed (Section 12) |
| parser repair (sc#285) | sovereign-core a667276 | outer-fence rule read; `v3_doc_from_reply` rewritten to match (Section 6, Section 12) |
| requirement validation (sc#287) | sovereign-core 8f3e8c8 | extraction rules read and run on every goal; one goal not recognized (Section 12) |

Freeze checklist, as done on 2026-10-07 (logs in `evidence-v3/`):

1. omega main 01f6a74 (#331 merged); `omega.lock` in sovereign-core is 01f6a74. DONE.
2. sc#284 merged; env names, 16-token floor, reason wordings re-checked. DONE, with the findings of Section 12.
3. Parser repair merged; `v3_doc_from_reply` re-checked and rewritten. DONE (`test-v3.sh` has cases for each branch).
4. Requirement validation merged and wired (`requirements_recognized` on the S3 report, `control.rs:371`). DONE; labels re-checked: G1 and G2 recognized, G3 not; RQ rows are outcome-based so this does not decide any row (Section 12).
5. Combined build from clean checkouts with the v2 Section 2 build lines plus `has_omega_wait_ms`, binaries' sha256. DONE (Section 7, `evidence-v3/build-summary.txt`).
6. Pre-run gate on 8f3e8c8: `cargo fmt --all --check` OK; `cargo clippy --workspace --all-targets -- -D warnings` OK;
   `AIEN_FORCE_CPU_STUB=1 cargo test -p aien-runtime -p aien-cli` 216 passed, 0 failed; `test-rows-v8.sh` with `NP1_EDIT_MERGE` =
   the frozen binary 142 passed, 0 failed; `test-v3.sh` 67 passed, 0 failed (re-run after the outcome-based RQ change, Section 12) (`evidence-v3/gate-summary.txt`). DONE.
7. CPU-only dry run of the scorer on `oq3-v3.decl.json` with made-up result lines: `test-v3.sh` section 3 (156 made-up PASS lines
   give PASS; a missing, a failing and a duplicate line do not) plus an explicit run of all 156 rows in `evidence-v3/scorer-dryrun.txt`.
   The daemon does not print its effective limits, so no limits row exists (Section 6, item 4). DONE.
8. A GPU dry run of this exact wrapper in `OQ3_DRY_TASKS` mode (as ACCEPTANCE-v2 Section 5). NOT DONE: it needs three quietlock holds
   and Drake's approval. Until it runs, the part timings of Section 8, the `<id>-M` cap row for document launches and the live reading of
   `requirements_recognized` are unmeasured.
9. Section 7 and the `FROZEN_*` lines of `run-qwen3-v3.sh` filled; Status FROZEN; merge this change alone. DONE here; the merge is Drake's.

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
- The rows were re-checked against the merged code at freeze (Section 12), not against a live run: the GPU dry run (Section 9, item 8) has not happened, so how the daemon reports its caps and attempts live is read from code, not observed.
- Requirement validation covers only the wordings it lists; "at least two sentences" in G3 is deliberately not covered and no row checks it. Its wording rules also leave the G3 goal unrecognized (Section 12, finding 1).
- `<id>-F` reads the merged parser's rule, restated in jq; a shared misreading would pass both.
- omega#327 (GPU memory failure) stays open: a PASS does not turn Qwen3 on GB10 on by default.
- R1 and N2 use document caps of 96 and 16, not 1024. Greedy decoding, one model, one machine.

## 12. Findings from re-checking against the merged code (sovereign-core 8f3e8c8, 2026-10-07)

Quoted file:line are in sovereign-core at 8f3e8c8. Nothing in the tasks file was changed to avoid a product weakness.

**Confirmed as the spec assumed**

- Env names and defaults: `AIEN_COMPOSE_EDIT_BUDGET_MS` (default 29 s, `spine.rs:841,845`), `AIEN_COMPOSE_DOC_BUDGET_MS` (default 120 s,
  `spine.rs:843,847`), `AIEN_COMPOSE_DOC_MAX_TOKENS` (default 1024, range 16..=4096, `spine.rs:850,854,856`); budgets accepted in
  1000..=599000 ms (`spine.rs:858`); edit cap `AIEN_COMPOSE_MAX_TOKENS` read by the daemon, default 48 (`server.rs:483-492`).
  N2's cap of 16 is the lowest accepted value. A stale `AIEN_COMPOSE_BUDGET_MS` is refused: `"AIEN_COMPOSE_BUDGET_MS is retired; set ..."`
  (`spine.rs:852,970-973`); bad values stop the daemon at start (`server.rs:100-103`).
- Reasons: a token-limit cut is `"reply cut at the token limit after N tokens (finish_reason max_tokens); a cut reply is never a proposal"`
  (`spine.rs:1306-1312`), which contains "token limit" and not "exceeded" (rows D, N2-R). A wall-clock timeout starts `"model proposal exceeded "`
  (`spine.rs:997`) and is classed "timeout".
- Report fields: `proposal_content_sha256` (`control.rs:358`), `proposal_attempts` (`control.rs:367`), `requirements_recognized` (`control.rs:371`,
  filled from the goal at `spine.rs:1869`), per-attempt `unmet_requirements` (`control.rs:410`, reason `"unmet requirement: ..."`, `requirements.rs:189`),
  `finish_reason` and `text` per attempt (`control.rs`). Labels are `"at least N non-empty lines"` and `"at least N headings"` (`requirements.rs:78-81`, `61`).
- Kind by goal (`classify_target`, `spine.rs:1091`, first existing path named in the goal, issue #288): run over all eight goals with the workspace the driver
  builds (README.md, docs/plan.txt, plus the launch's seed): E1 finds ROADMAP.md and E2 finds docs/OPERATIONS.md (edit, 29 s, cap 96); G1, G2, G3, N1, N2, R1 find no
  existing file (new document, 120 s). No task's kind differs from Section 2. N1's path is refused by the path check (`..`), not classified.

**Differs from what the spec assumed**

1. **G3's requirements are not recognized, so G3-RQ cannot pass as written.** `requirements::extract` (`requirements.rs:273`) only takes a count whose noun ends its
   clause (`clause_ends_after`, `requirements.rs:248`; "of/per/each/in/with ..." after the noun disqualifies it). The G3 goal reads "at least 35 lines **with** at least 6
   sections **titled** ...", so neither "at least 35 lines" nor "at least 6 sections" is extracted. Run on all eight goals with the merged module itself (`requirements::extract`):
   G1 gives `["at least 18 non-empty lines"]`, G2 gives `["at least 25 non-empty lines"]`, G3 gives `[]`, the other five give `[]`. Consequences: the runtime would not refuse a short G3
   reply and would not retry it; `requirements_recognized` is empty for G3. As first written, row G3-RQ demanded recognition (both labels in the report), so it would FAIL
   whatever the model wrote. G3-L (line count) and G3-F still run. This is a real weakness of requirement validation (a plain sentence like G3's is not covered). The goal
   was NOT rewritten, and sovereign-core was not changed.
   **Decision (orchestrator, 2026-10-07; replaces the open question to Drake about options a, b, c):** every RQ row (G1-RQ, G2-RQ, G3-RQ) is OUTCOME-BASED. It passes iff
   the bytes actually saved at the destination meet every measurable requirement the goal states, computed by the campaign tooling (`rows-oq3-v3.jq`, `v3_row_rq`) from the
   task file's declared requirement fields, independent of whether the product recognized them. Counting follows the `requirements.rs` doc comments: non-empty lines count
   every line with a non-whitespace character, code and fence marker lines included (`requirements.rs:36-37`, "Counting"); sections are markdown headings, 1 to 6 `#`, a space,
   then text (`requirements.rs:24`), skipping fenced code blocks and the fence lines themselves (CommonMark close rule, an unclosed fence runs to the end, `requirements.rs:37-41`),
   so a heading inside a code fence does not count. G3 therefore passes iff the saved ONBOARDING.md has at least 35 non-empty lines and the six named sections present as
   headings outside code fences (the six titles are a new `titles` field on G3's declared headings requirement in `tasks-oq3-v3.json`; goal text, ids and budgets are unchanged). If
   nothing was saved, RQ is FAIL. Why: the row used to measure the product's wording coverage, which `<id>-L` and the prediction already treat separately, and made G3-RQ unable
   to pass; the campaign question is whether the document the user would get meets the stated requirements. Whether the product recognized each requirement
   (`requirements_recognized`) is kept as information in the receipt side-file (`value.recognized_by_runtime`), not as a row. Row count is unchanged: 156 declared rows,
   21 new, because RQ stays one row per document launch and the declaration is byte-identical. Not changed: goals, task ids, budgets, the prediction section.
2. **Outer-fence rule differs from the prepared reading** (`check_file_proposal`, `outer_fence_close`, `spine.rs:733-830`): only a backtick opener is an outer fence
   (tilde is not); the opener line is always dropped; the close is found by the nesting rule (info-string fences nest; a bare fence at depth 0 with more fences after it opens
   a bare inner block; none balanced uses the last bare fence); text after the close is dropped, not kept; no close at all keeps the whole body. `v3_doc_from_reply` now
   follows this exactly (the earlier version kept an unclosed opener and text after the close, which would have failed honest replies). Known limit stated by the parser:
   prose holding a fence after the real close is read as part of the file.
3. **The daemon does not print its effective limits**, so no limits row (Section 6, item 4).
4. **Build warnings:** the build logs contain compiler warnings from third-party crates (dead-code lints) but no `cargo:warning` line from any build script.
