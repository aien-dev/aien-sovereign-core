# OPEN-MODEL-QWEN3 campaign v4: acceptance criteria (PREPARED)

**Status: PREPARED, NOT FROZEN, NOT RUN**

This is a draft written by session e3d035 for the orchestrator and Drake, 2026-10-07. It becomes FROZEN only when a later
change fills every "TO FILL AT FREEZE" entry below, changes this status line, and is merged before the run. Until then no
launch may run, `run-qwen3-v4.sh` refuses a real run while its build identity holds the placeholder, and nothing here is a
verdict. VERDICT-v4.md is written after the run and changes no row.

## 0. Read this first: what is and is not established

- **v1, v2 and v3 stand as recorded** (VERDICT-v1.md; VERDICT-v2.md = FAIL, 85 of 115 rows; VERDICT-v3.md = FAIL, 153 of
  156 rows, failing rows G2/Q2, G3/G3-L, G3/G3-RQ). Nothing here edits, reopens or reinterprets them, and no v1, v2 or v3
  file is changed by this campaign's files.
- **v4 asks a different question from v3.** v3 used goals written after seeing v2 and the diagnostic. v4 uses fresh goals
  with varied natural wording, every measurable requirement declared in the task file before any run, and outcome-based
  scoring on the exact saved bytes. The goals were written the way a user would phrase them, without reading the
  requirement-extraction code or any in-flight branch of it, so no wording was shaped to a particular extractor.
- **Three conclusions are kept apart and each is stated separately in the verdict:**
  1. **Document qualification**: do the saved documents and edits meet the requirements their goals state (this campaign's
     qualification rows).
  2. **GPU memory reliability**: omega#327 and omega#277 stay open and separate. Qwen3 on the GB10 stays opt-in. A run
     without a memory failure does not close either issue, and a memory failure is reported as its own finding, not as a
     document failure.
  3. **Release readiness**: not claimed by this campaign under any outcome.
- **Regression is separate from qualification.** Section 6 re-runs tasks that failed before. Those rows are reported and
  do NOT count toward the qualification verdict.
- Token-limit cut and wall-clock timeout are different reasons and are scored separately, as in v3 (rows CUT, D, N2-R).

## 1. Product limits under test

| Task kind | Wall budget (one deadline shared by all attempts of the task) | Output cap |
|---|---|---|
| Small edit (the goal names an existing file) | 29 s (`AIEN_COMPOSE_EDIT_BUDGET_MS`) | 96 tokens (`AIEN_COMPOSE_MAX_TOKENS`) |
| Full document (the goal creates a new file) | 120 s (`AIEN_COMPOSE_DOC_BUDGET_MS`) | 1024 tokens (`AIEN_COMPOSE_DOC_MAX_TOKENS`) |

These equal the v3 limits (`test-v4.sh` checks the limits object against v3). The wrapper sets both budgets and both caps for
every launch; the retired `AIEN_COMPOSE_BUDGET_MS` must be unset. N2 uses a document cap of 16 tokens (budget exhaustion),
R1 a document cap of 96 (bounded CPU reference replay), as in v3. **W4 deliberately runs with both caps at their product
values** (Section 2, "W4 and the product's classification"): no limit is changed to help it.

## 2. Launches

Ten qualification launches, once each, in this order: **W1, W2, W3, W4, W5, U1, U2, N1, N2, R1**, plus four regression
launches **RG2, RG3, RT4, RT6** after them (Section 6), fourteen launches in all. Machine copy: `tasks-oq3-v4.json` (each task has
`group`: qualification or regression).

| id | group | kind | cap | budget | pre-seed | expected completion |
|----|-------|------|-----|--------|----------|---------------------|
| W1 | qualification | document: line count, three named sections, two stated words | 1024 | 120 s | none | DONE |
| W2 | qualification | document: three fenced code examples, then a closing section with text | 1024 | 120 s | none | DONE |
| W3 | qualification | document: line count in number words, three topics | 1024 | 120 s | none | DONE |
| W4 | qualification | document created from a different existing source file | 1024 (edit cap 96 also at product value) | 120 s | `seed-v4/W4/notes/standup-log.txt` | DONE |
| W5 | qualification | document: minimum and maximum line count, item format, two stated words | 1024 | 120 s | none | DONE |
| U1 | qualification | small edit: add an item to one section | 96 | 29 s | `seed-v4/U1/notes/GARAGE.md` | DONE |
| U2 | qualification | small edit: add a numbered item, nested path | 96 | 29 s | `seed-v4/U2/todo/CHORES.md` | DONE |
| N1 | qualification | declared negative: destination outside the workspace | 1024 | 120 s | none | REFUSED |
| N2 | qualification | declared negative: budget exhaustion (cap 16) | 16 | 120 s | none | REFUSED |
| R1 | qualification | token-identity replay against the CPU reference | 96 | 120 s | none | DONE |
| RG2 | regression | v3 G2, verbatim | 1024 | 120 s | none | reported |
| RG3 | regression | v3 G3, verbatim | 1024 | 120 s | none | reported |
| RT4 | regression | v2 T4, verbatim | 1024 | 120 s | none | reported |
| RT6 | regression | v2 T6, verbatim | 1024 | 120 s | none | reported |

Task rows in the ACCEPTANCE-v5 Section 2 format (read by the inherited Q2 check; `test-v4.sh` checks these lines against
`tasks-oq3-v4.json`). **Every phrase in the last column is a string the goal itself states**, so the inherited phrase check
(Q2, case-insensitive, whitespace collapsed) cannot require a word the goal did not name; `test-v4.sh` checks this for every
qualification task. (The v3 G2/Q2 failure was a phrase the goal implied but did not state. The four regression rows keep their
original phrases verbatim, on purpose, Section 6.)

| id | goal (exact text given to the driver) | requested destination | required phrases (all) |
|----|----------------------------------------|-----------------------|------------------------|
| W1 | `Create the file docs/COMPOST.md, a beginner's guide to composting at home, with no fewer than 22 lines. Give it three sections titled "Choosing a bin", "What to add" and "Common problems". Somewhere in the guide use the exact words greens and browns.` | `docs/COMPOST.md` | `Choosing a bin`, `What to add`, `Common problems`, `greens`, `browns` |
| W2 | `Write a new file called docs/BACKUPS.md that documents a nightly backup routine for a small file server. A minimum of 20 lines is needed. Include three separate fenced code examples, introduced by sections titled "Create the archive", "Copy it offsite" and "Verify the copy", and after the last example finish with a section titled "Restore notes" that holds at least 15 words of plain text.` | `docs/BACKUPS.md` | `Create the archive`, `Copy it offsite`, `Verify the copy`, `Restore notes` |
| W3 | `Please put together docs/SEED-SAVING.md, a short explainer on saving vegetable seeds from your own garden. Make it at least eighteen lines long. Cover three topics: drying, labelling and storage. A topic counts as covered when its word, or another form of it such as dried, labelled or stored, appears in the file.` | `docs/SEED-SAVING.md` | `seed` |
| W4 | `Create the file docs/WEEK-SUMMARY.md summarising notes/standup-log.txt in at least 14 lines. It needs a section titled "Highlights" and a section titled "Open questions", and it must name Priya, Tomas and Wen.` | `docs/WEEK-SUMMARY.md` | `Highlights`, `Open questions`, `Priya`, `Tomas`, `Wen` |
| W5 | `Make a new file called docs/CAMPING-LIST.md, a packing checklist for a weekend camping trip. It should run to a minimum of 16 lines and no more than 45 lines. Write every item as a line starting with "- [ ]" and include at least twelve of them. Use sections titled "Shelter", "Food and water" and "Safety", and use the exact words headlamp and first-aid somewhere in the list.` | `docs/CAMPING-LIST.md` | `Shelter`, `Food and water`, `Safety`, `headlamp`, `first-aid` |
| U1 | `Please add "- check tyre pressure" to the "## Before a trip" section of notes/GARAGE.md.` | `notes/GARAGE.md` | `## Before a trip`, `- check tyre pressure`, `- top up the washer fluid` |
| U2 | `Under the heading "## Weekend jobs" in todo/CHORES.md, add the item "3. archive the old photos".` | `todo/CHORES.md` | `## Weekend jobs`, `3. archive the old photos`, `2. clean the gutters` |
| R1 | `Create the file docs/THANKS.txt with a one-sentence thank-you note for everyone who tested the project.` | `docs/THANKS.txt` | `thank` |
| RG2 | `Create the file docs/RELEASE-CHECKLIST.md with a release checklist of at least 25 lines that covers preparing, testing, publishing and announcing a release.` | `docs/RELEASE-CHECKLIST.md` | `preparing`, `testing`, `publishing`, `announcing` |
| RG3 | `Create the file docs/ONBOARDING.md, an onboarding guide for a new team member, of at least 35 lines with at least 6 sections titled Welcome, Accounts and access, Tools to install, Your first week, Who to ask and Glossary. Write at least two sentences in every section.` | `docs/ONBOARDING.md` | `welcome`, `accounts and access`, `tools to install`, `your first week`, `who to ask`, `glossary` |
| RT4 | `Create the file docs/GUIDE.md with a user guide of at least 12 lines that covers installation, configuration, running tests and getting help.` | `docs/GUIDE.md` | `installation`, `configuration`, `running tests`, `getting help` |
| RT6 | `Create the file docs/TROUBLESHOOTING.md with a troubleshooting guide of at least 12 lines that covers error messages, log files, network problems and reporting a bug.` | `docs/TROUBLESHOOTING.md` | `error messages`, `log files`, `network problems`, `reporting a bug` |

N1 goal: `Create the file ../shared/readme-copy.txt with the single line: copy for the other team.`
N2 goal: `Create the file docs/HISTORY.md with a long account of the project history that has at least twelve paragraphs.`

Seeds (pre-existing files; each edit seed has two or more headings on purpose):

```text
seed-v4/U1/notes/GARAGE.md             sha256 949b53e7d9b4879ce1556053f1adb5fe85254c9dbe1128394ce7b75ed25008a0
seed-v4/U2/todo/CHORES.md              sha256 1120f45d25acbff22f773c1bfc46b8889387e65e30a305970691aab5f5d7b41d
seed-v4/W4/notes/standup-log.txt       sha256 0556f748cf22fb04f9440736cf6c2a6ae0682a0970c6c00360bc7f423b2ab630
```

Goals avoid backticks and pipes so the task rows above stay exact.

**W4 and the product's classification.** W4 asks for a new file (`docs/WEEK-SUMMARY.md`) that summarises an existing one
(`notes/standup-log.txt`). The merged `classify_target` (`spine.rs`, sovereign-core main at the time of writing, read in
this session) treats the first existing path named in a goal as the target of an edit. W4's goal names an existing file, so the
product may run it as an edit of the source under the edit limits (96 tokens, 29 s), which cannot hold a 14-line summary.
UNVERIFIED until run: whether the product does that, and what it then produces. W4 is in the qualification set because a user
who writes "create X summarising Y" expects X to appear and Y to stay as it was. The rows measure exactly that outcome
(W4-SB, W4-RQ1 to W4-RQ5, W4-SRC) and the wrapper changes no limit for it. If W4 fails because of the classification, the
failure stands as a product finding, not a model finding, and the verdict says which.

## 3. Rows and exactly how each is counted

Row counts (machine-read by `test-v4.sh`, which fails if they drift from the generated declarations):

QUALIFICATION_ROWS=245
REGRESSION_ROWS=105

**Counting rules** (the same as ACCEPTANCE-v3 Section 12, plus the rules for the new requirement kinds; implemented in
`rows-oq3-v4.jq`, tested by `test-v4.sh` on made-up documents):

- **Lines**: every line with at least one non-whitespace character, code lines and fence marker lines included. Blank and
  whitespace-only lines are not counted.
- **Sections (headings)**: a markdown heading is 1 to 6 `#`, a space, then text; lines inside fenced code blocks (opened by three or
  more backticks or tildes, closed by the same marker at least as long with nothing after it; an unclosed fence runs to the
  end of the file) are skipped. A required section title matches a heading text after trimming, removing trailing closing `#`
  characters, and ASCII case-folding. Any heading level counts; the level is not scored unless the goal says so (none does).
- **Stated word** (`word`): the word occurs in the saved text as a whole word, ASCII case-insensitive. The character before
  and after it must not be a letter or a digit. `first-aid` matches `First-Aid kit`; `greens` does not match `evergreens`.
  Words contain only letters, digits and hyphen. The goal names each such word with the phrase "the exact words" or
  "name", so the row checks only what the goal states.
- **Covers topic X** (`topic`): the goal names the topic word and says that "its word, or another form of it such as dried,
  labelled or stored" counts. The row passes when the saved text contains, as a whole word (same rule as a stated word), at
  least one of the topic's accepted forms. The accepted forms are fixed here and in the task file, before any run:

  | Topic | Accepted forms (whole word, case-insensitive) |
  |---|---|
  | drying | `dry`, `dries`, `dried`, `drying` |
  | labelling | `label`, `labels`, `labelled`, `labeled`, `labelling`, `labeling` |
  | storage | `store`, `stores`, `stored`, `storing`, `storage` |

  No other form counts, and no form outside this table is used by any row. A longer word that merely contains a form
  (`restored`, `dryer`) does not count.
- **Code blocks** (`code_blocks`): the number of complete fenced blocks (an opening fence with its closing fence). An unclosed
  fence counts zero. Which section a block sits under is not scored.
- **Closing section after the last example** (`tail`): after the closing fence of the last complete code block there is a heading
  whose text equals the declared heading (same title rule as sections); the words (whitespace-separated tokens) of the non-empty
  lines after that heading, up to the next heading or the end of the file, number at least the declared minimum (W2: 15).
- **Item lines** (`prefixed_lines`): lines whose text, after leading whitespace, starts with the declared prefix (W5: `- [ ]`).
- **Maximum lines** (`max_lines`): non-empty lines, at most the declared number.
- **Number words**: the task file records each requirement as written in the goal (`as_written`, for example "at least
  eighteen") next to the numeric `n`; `test-v4.sh` checks that the goal contains that wording and that the number matches.

**Declared requirements per task** (the structured `requirements` field of `tasks-oq3-v4.json`, in row order `RQ1`, `RQ2` ...):

| Task | Row | Kind | Requirement as written in the goal | Rule |
|---|---|---|---|---|
| W1 | W1-RQ1 | min_lines | no fewer than 22 | at least 22 non-empty lines |
| W1 | W1-RQ2 | headings | three sections titled | the three named sections are headings |
| W1 | W1-RQ3 | word | greens | the word greens appears |
| W1 | W1-RQ4 | word | browns | the word browns appears |
| W2 | W2-RQ1 | min_lines | A minimum of 20 | at least 20 non-empty lines |
| W2 | W2-RQ2 | headings | introduced by sections titled | the four named sections are headings |
| W2 | W2-RQ3 | code_blocks | three separate fenced code examples | at least 3 complete fenced code blocks |
| W2 | W2-RQ4 | tail | at least 15 words | after the last code block, a Restore notes heading followed by at least 15 words |
| W3 | W3-RQ1 | min_lines | at least eighteen | at least 18 non-empty lines |
| W3 | W3-RQ2 | topic | drying | covers the topic drying |
| W3 | W3-RQ3 | topic | labelling | covers the topic labelling |
| W3 | W3-RQ4 | topic | storage | covers the topic storage |
| W4 | W4-RQ1 | min_lines | at least 14 | at least 14 non-empty lines |
| W4 | W4-RQ2 | headings | a section titled | the two named sections are headings |
| W4 | W4-RQ3 | word | Priya | the name Priya appears |
| W4 | W4-RQ4 | word | Tomas | the name Tomas appears |
| W4 | W4-RQ5 | word | Wen | the name Wen appears |
| W5 | W5-RQ1 | min_lines | a minimum of 16 | at least 16 non-empty lines |
| W5 | W5-RQ2 | max_lines | no more than 45 | at most 45 non-empty lines |
| W5 | W5-RQ3 | prefixed_lines | at least twelve | at least 12 lines starting with - [ ] |
| W5 | W5-RQ4 | headings | sections titled | the three named sections are headings |
| W5 | W5-RQ5 | word | headlamp | the word headlamp appears |
| W5 | W5-RQ6 | word | first-aid | the word first-aid appears |

Regression tasks declare their requirements the same way (RG2: RQ1 at least 25 lines; RG3: RQ1 at least 35 lines, RQ2 the six titles Welcome, Accounts and access, Tools to install, Your first week, Who to ask, Glossary; RT4 and RT6: RQ1 at least 12 lines).

**Inherited from v8, no row weakened** (rows-v8.jq and rows-v5.jq run unchanged; row names follow the launch id): the 8 v1 rows,
Q1 to Q4, `<id>-A` approval binds the proposal, A3 to A6, `<id>-CM` containment, `<id>-L` and `<id>-M` for documents,
`<id>-P/K/N/B` for edits, N1-C/B/Z/E/H, N2-L/F/C/Z/H, R1-X/P/T. `<id>-Z` and `N1-Z`/`N2-Z` (zero effects) and `N1-C`/`N2-C`
(REFUSED) cover "nothing committed on refusal" in the receipt; `<id>-A` and `<id>-CM` cover approval binding.

**New in v4** (computed beside the receipt by `v4-rows.sh` with `rows-oq3-v4.jq`, which includes `rows-oq3-v3.jq` unchanged):

| Row | Launches | What it requires |
|---|---|---|
| `<id>-SB` Saved bytes equal approved bytes | documents, edits, R1 | exactly one authorization; the sha256 of the file found in the workspace (computed by the scorer after the run, not the runtime) equals the authorization's content sha256, S5 content sha256, S5 disk sha256 and the S3 proposal content sha256; the file path equals the task destination |
| `<id>-F` Complete document saved | documents | fence lines even in number and at least 2 x `min_code_blocks`; the saved text equals the document an independent reading takes from the accepted reply by the merged parser rule (ACCEPTANCE-v3 Section 12) |
| `<id>-CUT` Accepted reply not cut | documents, edits, R1 | an accepted (parsed) attempt exists and its finish_reason is not max_tokens (no cut reply was accepted) |
| `<id>-RQ<k>` Requirement k met | documents | the saved file meets the k-th declared requirement by the counting rules above. Nothing saved is FAIL |
| `W4-SRC` Source unchanged | W4 | the sha256 of `notes/standup-log.txt` in the workspace after the run equals the declared sha256, and the source path differs from the destination |
| `<id>-EO` Edit outcome | U1, U2 | every non-empty seed line is a line of the saved text; a line equal to the declared new line follows the declared heading before the next level-one or level-two heading; the saved text has exactly one more non-empty line than the seed |
| `<id>-D` One deadline | all | at least one attempt; the attempts' total ms is at or below the task's `budget_ms`; no attempt's reason contains "exceeded"; for positive launches an accepted attempt exists |
| `<id>-NC` Nothing committed on refusal | N1, N2 | no authorization record, no S5 path and no file found at any S5 path |
| `<id>-BE` No CPU, stub or fallback backend | every launch | `run.json` `daemon` (one entry per daemon start, `backend` read from the daemon log by the driver) is non-empty, every backend contains `OmegaGb10` and none contains cpu, stub, reference or fallback (case-insensitive); no entry, an empty backend or a missing `run.json` FAILs. This is the only backend field a receipt carries: it is per daemon start, not per operation |
| `N2-R` Token-limit cut is not a timeout | N2 | every attempt has finish_reason max_tokens and a reason containing "token limit" and not containing "exceeded" |

Whether the product recognized a requirement (`requirements_recognized` in the S3 report) is not a row condition. A reply the
runtime refuses leaves no committed file, so every row that needs one FAILs for that launch; that is the intended reading.

## 4. Declaration

`gen-decl-oq3-v4.sh` writes two declarations from the v8 row shapes of the v2 declaration plus the v4 rows:
`../scoring/declarations/oq3-v4.decl.json` (qualification, 235 rows) and `../scoring/declarations/oq3-v4-regression.decl.json`
(regression, 101 rows). One repetition each, role case, no control, none NOT_APPLICABLE. `test-v4.sh` fails if either committed
file differs from the generated one. Per launch, qualification: N1 8, N2 9, R1 25, U1 27, U2 27, W1 29, W2 29, W3 29, W4 31, W5 31.

## 5. Verdict (qualification)

scoring-v5 (`../scoring/score-rows.sh`) over `oq3-v4.decl.json` and the qualification result file only: **PASS only if every one of
the 235 qualification rows is present once and PASS.** A launch with no `run.json` gets FAIL on every declared row of that
launch. Rules, all binding:

- A failed campaign stays failed: no rerun of a failure, no repeated launch, no retry after seeing a result, no rerun until
  pass.
- no task edit after seeing answers: goals, requirements, accepted forms, limits, rows and declarations are the ones merged at
  the freeze. A defect found in the spec after the run is recorded in the verdict as a finding and fixes only a new
  campaign (v5), never this one.
- All attempts, outputs, exit codes, timings, receipts, side files and the sha256 of every file in the run base are retained
  (`evidence-v4/`, as v3's). Nothing is deleted, trimmed or regenerated.
- The legacy receipt `verdict` field is outside the verdict, as in v2 and v3 (VERDICT-v3 explains why).
- The verdict reports the three conclusions of Section 0 in three separate lines.

## 6. Regression section (reported, NOT part of the verdict)

Four launches re-run, verbatim, tasks that failed in an earlier campaign. Their rows are declared in
`oq3-v4-regression.decl.json`, scored by their own scorer call into `oq3-v4-regression-score.json`, run last (part 5, after the
qualification score exists) so they cannot affect it, and **they do NOT count toward the qualification verdict**. The verdict
file reports them in their own table.

| Regression id | Origin | Why it failed before | Notes |
|---|---|---|---|
| RG2 | v3 G2 | G2/Q2: the reply had 26 lines but lacked the words preparing, publishing, announcing | phrases kept verbatim, so the same hidden-word check applies; reported for comparison |
| RG3 | v3 G3 | G3-L and G3-RQ: 13 of 35 non-empty lines | requirements: 35 lines and six named sections |
| RT4 | v2 T4 | v2 passed everything except a phrase check made impossible by a made-up goal; at a 256-token cap | goal verbatim, current limits |
| RT6 | v2 T6 | v2: reply cut at the 256-token cap, nothing committed | goal verbatim, current limits (1024 tokens, 120 s) |

v1: its three tasks failed on a containment row of the legacy receipt (VERDICT-v1.md), a receipt-reading problem fixed in v8, not on
task content, so no v1 task is re-run here. The regression launches use the v3 document limits, not the v2 cap of 256, because
the v2 cap was the reason T4 and T6 failed and a product that can now finish them is what is being checked.

## 7. Model, environment and build (freeze checklist)

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, weights unchanged. Every "TO FILL AT FREEZE" entry
below is empty now and is filled and checked at the freeze; the wrapper compares what it can.

```text
sovereign-core commit (exact)            = 6bbe2ec269768c7c9b94b9484c757ca45f55f564 (main; contains sc#311, #313, #315 to #319; v4 previously drafted at 4d4dfd4)
omega.lock commit                        = 6c6180cf378075b61291f4565d226eba38b4decd (omega.lock at 6bbe2ec2; changed from 01f6a74636b8383b010cdb95597839582c415c27, v3 used that)
physics commit                           = 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf (unchanged) (v3 used 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf)
aienos.lock commit                       = b84c0a67590a934f3f3e001b12ec85ebc086a9eb (unchanged; build log "matches") (v3 used b84c0a67590a934f3f3e001b12ec85ebc086a9eb)
Cargo.lock sha256                        = 49d97bf30113b1727fcfc0e33be79d9446ae13651a08afc32bba889b77fca265
aien-cli sha256                          = 152c0aecce662f618bf683c8854d6de56a7075e0461c2433570f4c15b68571a5
np1_reference sha256                     = ea1e22f114192cbb94afddef8026239252246a03390ef6986eb4b2c00ad9c046
np1_edit_merge sha256                    = 1815cd51b2975afc3ef841123a47cf6d48b381cd14c42afa3dbf0fecaa43a07d
build logs show has_omega_compose, has_omega_gpu, has_omega_wait_ms = confirmed (evidence-v4/build-lines.txt)
```

Model files (values recorded from the v3 wrapper and re-hashed on this machine on 2026-10-07; TO FILL AT FREEZE means re-hash
and confirm at the freeze), directory `/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/`:

```text
model.safetensors.index.json             d6c42883a895dfef5b0080ed2116a1bcd764f558406b98923d675978a1abf29c   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
model-00001-of-00003.safetensors         75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
model-00002-of-00003.safetensors         0b48adbb1f60e901153d91907ba11ce63bd4b8b584482e730f48808d055dfba1   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
model-00003-of-00003.safetensors         7dd39ccca5e4de123c74c14af44c9bf2eb75df33b4614382af0134528e060d5d   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
tokenizer.json                           aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
tokenizer_config.json (holds the chat template; checked by the v4 wrapper, not by v3) a62ff0a2472a0fa1b8eaabcb57c59b58afa42a22831dc141400b6e0cf2b65ce3   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
chat template string alone (jq -j .chat_template | sha256sum)   64f85b198065d0fba2a81f37e10ed68161ce2c19a754c7100e67e0ca2ee9c326   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
config.json                              5beea1a4a34c62782bfb2f911c606741a3bab8f92d80a118fa053c28af12e8ba   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
generation_config.json                   835fffe355c9438e7a25be099b3fccaa98350b83451f9fd2d99512e74f1ade48   (re-hashed 2026-10-08, identical to 2026-10-07; evidence-v4/model-sha256.txt)
```

Environment, checked by the wrapper (values are what v3 used; confirm at the freeze):

```text
AIEN_KV_CONTEXT_TOKENS                   = 4096                       (TO FILL AT FREEZE: confirm)
AIEN_REQUIRE_BLACKWELL                   = 1                          (TO FILL AT FREEZE: confirm)
AIEN_GB10_QWEN3_DECLARED_ATTEMPT         = 1                          (TO FILL AT FREEZE: confirm, opt-in flag; the daemon warns about omega#327 once per model call)
AIEN_COMPOSE_EDIT_BUDGET_MS              = 29000                      (set by the wrapper from the tasks file; TO FILL AT FREEZE: confirm)
AIEN_COMPOSE_DOC_BUDGET_MS               = 120000                     (set by the wrapper; TO FILL AT FREEZE: confirm)
AIEN_COMPOSE_MAX_TOKENS                  = 96 for edits               (set per launch by the wrapper; TO FILL AT FREEZE: confirm)
AIEN_COMPOSE_DOC_MAX_TOKENS              = 1024 for documents, 16 for N2, 96 for R1   (set per launch; TO FILL AT FREEZE: confirm)
unset: AIEN_FORCE_CPU_STUB, AIEN_COMPOSE_BUDGET_MS, AIEN_OMEGA_SPIN_US, AIEN_OMEGA_CTA_BUDGET   (TO FILL AT FREEZE: confirm)
AIEN_OMEGA_DIR / AIEN_PHYSICS_DIR / AIEN_AIENOS_LOCK_REPO = TO FILL AT FREEZE
```

Campaign files (sha256 of each as merged at the freeze):

```text
run-qwen3-v4.sh                          = TO FILL AT FREEZE
tasks-oq3-v4.json                        = TO FILL AT FREEZE
oq3-v4.decl.json                         = TO FILL AT FREEZE
oq3-v4-regression.decl.json              = TO FILL AT FREEZE
rows-oq3-v3.jq                           = TO FILL AT FREEZE   (unchanged from v3, b81788df... in v3 evidence)
rows-oq3-v4.jq                           = TO FILL AT FREEZE
v4-rows.sh                               = TO FILL AT FREEZE
gen-decl-oq3-v4.sh                       = TO FILL AT FREEZE
test-v4.sh                               = TO FILL AT FREEZE
seed-v4/ (three files, hashes in Section 2) = TO FILL AT FREEZE: confirm unchanged
next-phase-1 driver run-campaign.sh, make-receipt.sh, v8-results.sh, rows-v8.jq, rows-v5.jq = TO FILL AT FREEZE (sha256 of each)
```

GPU hold names (quietlock), one per part, 20 minutes or less each, back to back:

```text
part 1 hold name                         = TO FILL AT FREEZE
part 2 hold name                         = TO FILL AT FREEZE
part 3 hold name                         = TO FILL AT FREEZE
part 4 hold name                         = TO FILL AT FREEZE
part 5 hold name                         = TO FILL AT FREEZE
```

The wrapper refuses a real run unless the commit arguments equal the frozen commits and the three binaries' sha256 equal
the frozen values (it also refuses while any of them is still the placeholder), and run path and output directory are fresh.

## 8. Run plan: parts and holds

Run path pinned: `RUN_BASE=/home/drakestapleton/workspace/oq3-v4-runs` (to be created by the wrapper; the prompt embeds the
workspace path). One run: no retry, no repeated launch, no cache drop; MemFree and Cached recorded before each part. Five parts, one
quietlock hold of 20 minutes or less each, back to back. Part k refuses unless part k-1 completed in the same RUN_BASE with the
same binaries, tasks file, declarations, rows modules and wrapper. No GPU work happens before the freeze and Drake's go-ahead.

Time estimate. UNVERIFIED, derived from v3 (a launch costs about 140 s without decoding; documents decoded 20 to 43 s; the
three v3 parts took 8 min 42 s, 9 min 51 s and 4 min 23 s):

| Part | Launches | Expected | Worst case (documents at their 120 s deadline, edits at 29 s) | Hold |
|---|---|---|---|---|
| 1 | W1 W2 W3 | about 9 to 10 min | 3 x (140 + 120) s = 13 min | 20 min |
| 2 | W4 W5 U1 U2 | about 11 min | 2 x (140 + 120) + 2 x (140 + 29) s = 14 min 18 s | 20 min |
| 3 | N1 N2 | about 6 min | 2 x (140 + 120) s = 8 min 40 s | 20 min |
| 4 | R1 and the QUALIFICATION score | about 5 min | about 14 min if the CPU reference runs to its 96-token cap | 20 min |
| 5 | RG2 RG3 RT4 RT6 (regression) | about 12 min | 4 x (140 + 120) s = 17 min 20 s | 20 min |

If the GPU dry run (Section 9, item 8) shows more, a part is split before freezing; a part is never extended.

## 9. Self-test and freeze checklist

`test-v4.sh` is CPU only (no model, no GPU, no hold) and is part of the pre-run gate. It checks: both declarations equal the
generated ones and the row counts above; task freshness against every earlier tasks file, acceptance file, verdict and the
diagnostic (goal text, destinations, distinctive topic words); every phrase and every requirement wording appears in its goal;
regression goals are verbatim earlier goals; the scorer on made-up result lines for both declarations; one passing and at
least one failing synthetic reply per qualification task, run through `v4-rows.sh` (each failing reply must fail exactly the
intended requirement rows); negatives, identity, approval-binding, cut and deadline cases; direct tests of each counting
rule; wrapper refusals; and the unchanged receipt builder staged as the wrapper stages it.

Result when this draft was prepared (CPU only): test-v4.sh: 170 passed, 0 failed (CPU only, no GPU, no hold; run from the repository root of the branch)

Freeze checklist (all TO FILL AT FREEZE unless marked done):

1. Pick the sovereign-core commit to freeze on (main, with whatever product changes are then merged) and the matching
   omega.lock and physics commits: 6bbe2ec2, omega 6c6180c, physics 6d7cf0d (Section 7; moved from 4d4dfd4 on 2026-10-08; if main moves again before freeze, repeat items 2 to 5).
2. Re-read this file's Section 12 against that commit. TO FILL AT FREEZE.
3. Combined build from clean checkouts, binaries' sha256 and build-line evidence into `evidence-v4/`: done 2026-10-08 on
   6bbe2ec2, `evidence-v4/build-summary.txt` and `build-lines.txt`.
4. Pre-run gate on that build: `cargo fmt --all --check`, clippy with `-D warnings`, the runtime and CLI tests in stub and
   linked builds reported separately, `test-rows-v8.sh`, `test-v3.sh` and `test-v4.sh`: first run 2026-10-07 on 4d4dfd4, re-run in part on 6bbe2ec2 on 2026-10-08 (all but the linked tests and test-v3.sh), results in
   `evidence-v4/pre-run-gate.txt` (4d4dfd4: stub 289/1/65, linked 350/0/5, test-v4 164/0; 6bbe2ec2: stub 365/0/107, linked NOT RUN, test-v4 170/0).
   Repeat at freeze if the frozen commit changes.
5. Scorer dry run on both declarations with made-up lines (`test-v4.sh` section 3): done in this draft; repeat at freeze.
6. A GPU dry run of this exact wrapper in `OQ3_DRY_TASKS` mode: NOT DONE (needs five holds and Drake's approval). Until it runs,
   the part timings of Section 8, the live reading of `requirements_recognized` and W4's classification are unmeasured.
7. Fill every "TO FILL AT FREEZE" line of Section 7 and the `FROZEN_*` lines of `run-qwen3-v4.sh`; change the Status line;
   merge that change alone, with Drake's go-ahead. TO FILL AT FREEZE.

## 10. Prediction (stated before any run; UNVERIFIED)

Written now from VERDICT-v1, VERDICT-v2, VERDICT-v3 and DIAGNOSTIC-LONG-1 numbers and my own judgment. Nothing here is
measured; the rows decide. Probabilities are for "every row of that launch passes".

- N1 about 85 percent, N2 about 85 percent, R1 about 80 percent (all three passed in v3 on similar goals).
- U1 about 70 percent, U2 about 60 percent (v3 edits passed in about 5 s; risks are the numbered-list item in U2 and the
  extra non-empty-line rule of EO).
- W1 about 40 percent. The model must write 22 non-empty lines (v3 G2 gave 26 against 25 asked, v3 G3 gave 13 against 35), and
  name three headings exactly and use the words greens and browns. Likely failing row: RQ1 (length).
- W2 about 30 percent. Three complete code blocks plus a closing section of at least 15 words; v3 G1 passed a similar structure
  at 20 lines. Likely failing rows: RQ1 (20 lines), RQ4 (words in the closing section) or RQ2 (a heading title).
- W3 about 35 percent. Topics are easy to hit; the 18-line count is the risk.
- W4 about 10 percent. Low because of the classification described in Section 2 (the product may run it as an edit of the
  source file under 96 tokens and 29 s), not because of the model. If it is run as a document with the document limits, about 40 percent.
- W5 about 35 percent. Needs at least 16 and at most 45 non-empty lines, twelve item lines in the stated form and two stated words.
- Whole qualification PASS: about 2 to 3 percent (written before #295 merged; Section 10.1 now makes it 0 percent, because W3, W5 and R1 cannot commit). The expected verdict is FAIL. Launch results are correlated through the model's
  habit of writing short paragraphs, so the product of the figures above (below 1 percent) is too low; I round up.
- Regression, reported only: RG2 about 30 percent, RG3 about 25 percent, RT4 about 70 percent, RT6 about 70 percent.
- GPU memory failure during the run (omega#327 or #277): about 5 percent. If it happens it is reported as the second conclusion
  of Section 0 and the affected launches fail their rows without being read as document failures.
- Per-token speed about 88 to 95 ms; documents between 300 and 700 tokens finish in 28 to 70 s; no token-limit cut at 1024 (about 5 percent).


### 10.1 Requirement analyser outcome per task (stated before any run, from the product, not from any reply)

Sovereign-core #295 (in the pinned 6bbe2ec2, with #319 FirstLineHeading) reads each goal's stated requirements and, when any span is uncertain, refuses the
task before any model call (`spine.rs`, "refuse before any model call"; the verify step refuses again as a backstop). Such a
launch therefore records no attempt at all. The table is what `crates/aien-runtime/tests/requirements_usability_test.rs`
(lists SILENT, RECOGNIZED, REFUSED over `tests/fixtures/goal_corpus.tsv`) asserts for each v4 goal on that commit. It is a
product fact, checked on CPU; it is not a model result. Regenerated mechanically on 2026-10-08 at 6bbe2ec2 by calling `requirements::analyze` on each goal text of `tasks-oq3-v4.json` (no model, no daemon, no GPU; output and probe source in `evidence-v4/requirements-probe-6bbe2ec2.txt`). The only difference from the 4d4dfd4 table is the longer span reported for R1; every outcome is unchanged. The goals were written before #295 merged and are kept verbatim:
rewording a goal so the analyser accepts it would tailor the tasks to the product.

| id | analyser | span reported (REFUSED only) | consequence for the rows |
|----|----------|------------------------------|--------------------------|
| W1 | RECOGNIZED | | requirements enforced at verify; the rows decide |
| W2 | RECOGNIZED | | as W1 |
| W3 | REFUSED (uncertain) | "three topics" | no model call, no commit: completion is not DONE, so W3 FAILS |
| W4 | RECOGNIZED | | as W1 |
| W5 | REFUSED (uncertain) | "twelve of them" | no model call, no commit: W5 FAILS |
| U1 | SILENT | | nothing to enforce; the rows decide |
| U2 | SILENT | | as U1 |
| N1 | RECOGNIZED | | declared negative; expected REFUSED on the destination, the rows decide |
| N2 | REFUSED (uncertain) | "at least twelve paragraphs" | refused, but before the model: no attempt, so N2-F and N2-R ("at least one attempt", rows-v8.jq and rows-oq3-v3.jq) FAIL; the budget limit is never exercised |
| R1 | REFUSED (uncertain) | "one-sentence thank-you note for everyone who tested the project" | no model call and no commit: R1-X needs an accepted attempt, so R1 FAILS; token identity is not measured |
| RG2, RG3, RT4, RT6 | RECOGNIZED (RG2, RG3 are v3 G2, G3 verbatim, listed under tasks-oq3-v3) | | regression, reported only |

Consequence: the qualification verdict is predicted FAIL with certainty, independent of the model (W3, W5, N2 and R1 make no
model call). The run still measures the model on W1, W2, W4, U1 and U2, N1's destination refusal, the four regression goals,
and GPU memory over a long session. A refusal of W3, W5, N2 or R1 is a product limit, reported as such; it is not rerun and the goals are not changed afterwards.

## 11. What v4 does not prove

- One run, one launch per task: one observation each, not a rate. Greedy decoding is the intended mode (documented: `server.rs` model_proposer "Greedy"; `np1_reference` temperature 0.0), one model, one machine. **GAP, not closed:** no receipt, run.json or attempt field records the temperature or sampling used per launch (checked 2026-10-08 on 6bbe2ec2: the attempt carries tokens, token_ids, finish_reason, prompt_ids_sha256, text_sha256; the daemon entry carries pid, backend, model, vmhwm_kb, warm_up_ms). A "greedy proven per launch" row would need a new product field and is not added; the verdict reports greedy as DOCUMENTED, not OBSERVED.
- The tasks are fresh and were written without reading the extraction code, but the author knows v1 to v3 results and chose
  requirement kinds that earlier runs found hard (line counts, stated words, structure after code blocks). This is a held-out set
  in the sense that no earlier goal or topic is reused (`test-v4.sh` proves it against the repository files), not in the
  strict sense of unseen difficulty.
- The rows were not checked against a live run: the GPU dry run has not happened.
- `<id>-F` reads the merged parser's rule restated in jq (ACCEPTANCE-v3 Section 12); a shared misreading would pass both.
- The topic rule is a stated list of word forms. It does not judge whether a section really covers a topic well, and a document
  that mentions "dry" once passes the drying topic. That is the declared, fair, narrow reading.
- "At least two sentences" style requirements are not measured anywhere in v4; no goal asks for them.
- A PASS would show the saved documents meet the declared requirements on these tasks once. It would not close omega#327 or
  omega#277, would not turn Qwen3 on GB10 on by default, and would not claim release readiness.

## 12. Independence and findings recorded while preparing

- This spec was written without reading `crates/aien-runtime/src/requirements.rs` or any in-flight requirement-extraction
  branch. The counting rules in Section 3 are the v3 rules (ACCEPTANCE-v3 Section 12), which v3 recorded from the merged documentation of
  the counting rules, plus the new kinds defined here. The only product code read for v4 is `classify_target` in `spine.rs`, to
  learn how a goal that names an existing file is classified (W4).
- The v3 verdict's lesson, G2/Q2, is applied: no phrase check requires a word the goal does not state (`test-v4.sh`).
- The v3 verdict's lesson, G3, is applied by declaring every requirement in the task file and scoring the saved bytes, as v3
  did after its decision, now with one row per requirement so a failure names the requirement.
- Not done, by design: no task, limit or row was tuned to a guess of Qwen3's behaviour, and nothing was run on a GPU.
- **Receipt builder limit, found while preparing.** The unchanged `make-receipt.sh` reads `seed/<destination>` for any task that
  has a seed, which fits edits only. W4 is a document whose seed holds a different file. The wrapper therefore stages the tasks
  file for the receipt builder with W4's seed set to null (`STAGE_FILTER` in `run-qwen3-v4.sh`), while the driver still receives
  the real seed, and `v4-rows.sh` reads the source file itself (row W4-SRC). `test-v4.sh` section 6 stages it the same way and
  checks that the builder gives exactly the declared v8-shape rows for all fourteen launches.
- `v4-rows.sh` includes the unchanged `rows-oq3-v3.jq` through `jq -L`; the identity files of each part hash both row modules, so a
  change to either stops the next part.
- The row names `<id>-RQ<k>` replace v3's single `<id>-RQ` so a failing requirement is named in the declared row.
