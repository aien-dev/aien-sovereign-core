# OPEN-MODEL-QWEN3 campaign v5: acceptance criteria (DRAFT)

**Status: DRAFT, NOT FROZEN, NOT RUN**

Written by session fb5693 on 2026-10-08 under Drake's decision (c) (`~/handoffs/2026-10-08-drake-decisions.md`): do not run
v4, write v5 first; keep v4 unchanged as a diagnostic spec and regression evidence; fix the product causes of legitimate
tasks being refused, never the scoring; v5 = fresh tasks, real output-byte checks, verified decoding mode, backend provenance,
no silent fallback; review, freeze, one chip slot.

This file becomes FROZEN only when a later change fills every "TO FILL AT FREEZE" entry, passes every freeze gate of
Section 9, changes this status line, and is merged before the run with Drake's go-ahead. Until then no launch may run and
nothing here is a verdict.

## 0. What is and is not established

- **v1, v2, v3 stand as recorded** (VERDICT-v1.md; VERDICT-v2.md FAIL 85 of 115; VERDICT-v3.md FAIL 153 of 156). Nothing here
  edits or reinterprets them.
- **v4 stays exactly as written** (sc#294, branch `e3d035/oq3-v4-campaign`, ACCEPTANCE-v4.md). It is not run (decision (c)). It is
  kept as a diagnostic spec and as regression evidence. Its Section 10.1 finding stands as negative evidence: on the product of
  2026-10-08, four of its ten qualification goals (W3, W5, N2, R1) are refused before any model call. v5 changes no v4 file.
- **The v4 finding is wider than four tasks.** The v5 goals below were written by an independent author who did not open any
  repository. On origin/main 9b5e6e8 (which contains #295, #319, #293 and #329) the product refuses **8 of the 11** before any
  model call (`evidence-v5/requirements-probe-v5-goals.txt`). On the same commit the v4 table is reproduced exactly
  (`evidence-v5/requirements-probe-v4-goals.txt`). The cause is the requirement reader, not the model: Section 1 lists each cause
  with its issue. v5 cannot freeze until those are fixed (Section 9, gate G1); **the goals are never reworded to suit the reader**.
- **Three conclusions stay separate in the verdict**, as in v4: (1) document qualification (this campaign's rows); (2) GPU memory
  reliability (omega#327, omega#277, sc#277 stay open and separate; Qwen3 on GB10 stays opt-in; a memory failure is reported as
  its own finding); (3) release readiness, not claimed under any outcome.
- **Token-limit cut and wall-clock timeout** are scored separately, as in v3 and v4.

## 1. Product fixes v5 depends on

A legitimate goal refused by the product is a product failure. Each cause is fixed in the product; no row, count or goal is
changed to make a task pass. Probe evidence for every line is in `evidence-v5/`.

| # | Cause (product) | Goals affected | Fix |
|---|---|---|---|
| P1 | Count wordings refused before the model: `between 16 and 30 lines`, `three sections titled ...`, `22 non-empty lines and ...`, `one section must be titled "T"` | v5 D1, D5, D6 | issue sc#331 |
| P2 | `cover N topics: a, b and c` refused before the model; and the topic check rejects forms the goal accepts (`dried`, `store`, `stored`, `Label`, `withdrew`), so a correct document would be refused at verify | v4 W3; v5 D3 | issue sc#332 |
| P3 | Required-word wordings refused (`make sure the words X and Y appear`, `the words X and Y show up`) or silently not read (`use the exact words`, `must name A, B and C`, `mention ... by name`) | v5 D1, D5 (refused); v4 W1, W4, W5, v5 D4 (not read) | issue sc#333 |
| P4 | Item counts that refer back (`twelve of them`, `12 of those items`) refused before the model | v4 W5; v5 D5 | issue sc#334 |
| P5 | Paragraph counts (`at least 7 paragraphs`) refused; no paragraph requirement kind | v4 N2; v5 D6, N2 | issue sc#335 |
| P6 | One-sentence and one-line files refused before the model (`a one-sentence reminder`, `just one line that says "..."`); N1 may be refused by the reader before the destination check, so a refusal row could pass for the wrong reason | v4 R1; v5 R1, N1 | issue sc#336 |
| P7 | No positive fallback evidence: the daemon records neither strict mode nor op accounting (`OP_REPORT`) | every launch (row `<id>-OPR`) | issue sc#337 |
| P8 | The generation record binds only the index file of a sharded checkpoint (`model.safetensors.index.json`), not the three weight shards | every committed launch (row `<id>-GR`) | issue sc#338 |

Already merged and used by v5: sc#329 (the generation record and every compose attempt state the decoding actually taken,
row `<id>-DEC`), sc#293 (the destination is the path the goal names to create, which D4 needs), sc#295 and sc#319
(requirement enforcement end to end).

Not a freeze dependency, reported separately: sc#277 / omega#327 / omega#277 (GPU memory). sc#294 (v4) should merge as the
diagnostic record so the v4 scripts v5 reuses (Section 4) are on main.

## 2. Launches and limits

Eleven qualification launches, once each, in this order: **D1, D2, D3, D4, D5, D6, E1, E2, N1, N2, R1**. Machine copy:
`tasks-oq3-v5.json`. No chip regression launches: the scope stays narrow and fits one chip slot. Regression evidence is the
CPU probe of all fourteen v4 goals on the frozen commit (Section 6), reported, not counted.

Limits are the v3 and v4 product limits: small edit 29 s and 96 tokens; new document 120 s and 1024 tokens; N2 a document cap of
16 tokens (budget exhaustion); R1 a document cap of 96 (bounded CPU reference replay). The wrapper sets both budgets and both caps
for every launch; the retired `AIEN_COMPOSE_BUDGET_MS` must be unset.

| id | kind | cap | budget | seed | expected completion |
|----|------|-----|--------|------|---------------------|
| D1 | document: line count, three titled sections, two stated words | 1024 | 120 s | none | DONE |
| D2 | document: three fenced code examples with titled sections, a closing section of at least 12 words | 1024 | 120 s | none | DONE |
| D3 | document: line count, four named topics with stated word forms | 1024 | 120 s | none | DONE |
| D4 | document created from a different existing file; titled sections; three names | 1024 | 120 s | `seed-v5/D4/notes/choir-rehearsal.txt` | DONE |
| D5 | document: line range, item prefix and item count, titled sections, two stated words | 1024 | 120 s | none | DONE |
| D6 | document: paragraph count, one titled section | 1024 | 120 s | none | DONE |
| E1 | small edit: add a bullet to one section | 96 | 29 s | `seed-v5/E1/repairs/tap-repair.md` | DONE |
| E2 | small edit, nested path: add a numbered item at the end of a list | 96 | 29 s | `seed-v5/E2/crafts/yarn/scarf.md` | DONE |
| N1 | declared negative: destination outside the workspace | 1024 | 120 s | none | REFUSED, for the destination |
| N2 | declared negative: budget exhaustion | 16 | 120 s | none | REFUSED, token limit |
| R1 | token-identity replay against the CPU reference; one sentence | 96 | 120 s | none | DONE |

Goals (exact text given to the driver; written by the independent author, unchanged):

| id | goal | destination |
|----|------|-------------|
| D1 | `Please create the file docs/knife-sharpening.md, a short how-to guide on sharpening a kitchen knife by hand. It should have at least 22 non-empty lines and three sections titled "Gather Your Tools", "Sharpen the Edge" and "Test and Store". Make sure the words whetstone and angle both appear somewhere in it.` | `docs/knife-sharpening.md` |
| D2 | `I need a technical how-to saved as docs/rename-photos.md that shows how to rename a folder of holiday photos with a bash loop. Write at least 18 lines in total and include 3 fenced code examples, where each example is introduced by its own section titled "Example One: Date Prefix", "Example Two: Lowercase Names" and "Example Three: Number the Files" in that order. After the last code example, add a final section titled "Wrap Up" containing at least 12 words of plain text and no code.` | `docs/rename-photos.md` |
| D3 | `Write an explainer for beginners called docs/savings-basics.md about how a savings account works. It must cover four topics: interest, deposit, withdraw and balance, and the file needs at least 18 lines. A topic counts as covered when its word or another form of that word appears, for example interests or interested for the first one, deposits or deposited for the second, withdrew or withdrawing for the third, and balances or balancing for the last.` | `docs/savings-basics.md` |
| D4 | `There is a file at notes/choir-rehearsal.txt with my rough notes from choir practice. Please create a new file called docs/choir-summary.md that summarises it, and leave the original alone. The summary needs at least 13 lines, two sections titled "Key Points" and "Next Steps", and must mention Marta, Owen and Priya by name.` | `docs/choir-summary.md` |
| D5 | `Make me a carry-on packing checklist in docs/packing-carry-on.md. Keep it between 16 and 30 lines long. Every item must be its own line starting with "- [ ]" and I want at least 12 of those items. Organise it under three sections titled "Documents", "Clothes" and "Electronics", and be sure the words passport and charger show up.` | `docs/packing-carry-on.md` |
| D6 | `Please write a warm, longer essay about adopting a rescue cat and save it as docs/rescue-cat-essay.md. It should run to at least 7 paragraphs, and one section must be titled "Bringing Her Home".` | `docs/rescue-cat-essay.md` |
| E1 | `In the existing file repairs/tap-repair.md, add the bullet line "- Thread seal tape for the spout" under the section called "Parts" and leave everything else as it is.` | `repairs/tap-repair.md` |
| E2 | `Open crafts/yarn/scarf.md and put one more numbered item, "4. Cast off loosely and weave in the ends.", at the end of the list under the heading "Steps". Don't change the other sections.` | `crafts/yarn/scarf.md` |
| N1 | `Create a file at ../shared-stuff/reminder.txt containing just one line that says "Renew the library card on Friday."` | refused |
| N2 | `I would like a long, detailed history of the marathon as a race, from the legend of ancient Greece to modern city events, saved as docs/marathon-story.md. Please make it at least twelve paragraphs long.` | refused |
| R1 | `Save a one-sentence reminder about stretching the shoulders before swimming into notes/swim-tip.txt.` | `notes/swim-tip.txt` |

Seeds (each edit seed has three level-two headings; the D4 source names the three people):

```text
seed-v5/D4/notes/choir-rehearsal.txt     sha256 aa8780bbc1c587cc6f980e44d838b1dfc39e11d06365d3471126008c9b99b7b7
seed-v5/E1/repairs/tap-repair.md         sha256 4766451b85a3cabde953959bb75c990fbf9f82e1caf41554c9a695e8937b91b9
seed-v5/E2/crafts/yarn/scarf.md          sha256 3a5a8494fbe60908c9a9431ac4839f17e4962a5a51f22399662087073cf00909
```

## 3. Rows and exactly how each is counted

Every row is computed by the scorer from the files the run leaves, never from a runtime verdict. The scorer computes every
sha256 itself. A launch with no `run.json` FAILs every declared row of that launch. Absent evidence is FAIL, never PASS.

### 3.1 Counting rules (saved bytes)

The v4 rules (ACCEPTANCE-v4 Section 3) apply unchanged: lines, sections (headings, fenced code skipped), stated word (whole
word, ASCII case-insensitive, neighbours not a letter or digit), topic (accepted forms only), code blocks (complete fenced
blocks), closing section after the last example (`tail`), item lines (`prefixed_lines`), maximum lines. New in v5:

- **Paragraph** (`paragraphs`): outside fenced code blocks, a maximal run of consecutive non-empty lines none of which is a
  heading line. Blank lines and heading lines separate paragraphs. A list block counts as one paragraph (no v5 goal that counts
  paragraphs asks for a list). `at least N paragraphs` passes when the count is N or more.
- **One sentence** (`sentences_exact`, R1): the saved text, trimmed, is one non-empty line that ends with `.`, `!` or `?`
  and contains no `.`, `!` or `?` followed by whitespace before that last character. Abbreviations are not special-cased; the
  goal asks for a reminder, not a citation, and this narrow reading is declared here before any run.
- **Edit position** (E2 only, `end_of_section`): in addition to the v4 edit-outcome rule, the new line is the last non-empty
  line before the next heading after `## Steps`.

Topic forms (fixed now, before any run; whole word, case-insensitive; a longer word that only contains a form does not count):

| Topic (D3) | Accepted forms |
|---|---|
| interest | `interest`, `interests`, `interested` |
| deposit | `deposit`, `deposits`, `deposited`, `depositing` |
| withdraw | `withdraw`, `withdraws`, `withdrew`, `withdrawn`, `withdrawing`, `withdrawal`, `withdrawals` |
| balance | `balance`, `balances`, `balanced`, `balancing` |

The independent author also listed `interesting` for interest; the spec author removed it before any run (it is not a form of
the banking sense of the word) and added `withdrawal`, `withdrawals` (the noun of withdraw). Both changes are recorded here and in
`tasks-oq3-v5.json`.

### 3.2 Declared requirements per task (rows `<id>-RQ<k>`, in the order of `tasks-oq3-v5.json`)

| Task | Rows |
|---|---|
| D1 | RQ1 at least 22 non-empty lines; RQ2 the three titled sections are headings; RQ3 the words whetstone and angle appear |
| D2 | RQ1 at least 18 non-empty lines; RQ2 at least 3 complete fenced code blocks; RQ3 the three example titles are headings; RQ4 after the last code block a Wrap Up heading followed by at least 12 words |
| D3 | RQ1 at least 18 non-empty lines; RQ2 to RQ5 the topics interest, deposit, withdraw, balance |
| D4 | RQ1 at least 13 non-empty lines; RQ2 Key Points and Next Steps are headings; RQ3 Marta, Owen and Priya appear |
| D5 | RQ1 at least 16 and RQ2 at most 30 non-empty lines; RQ3 at least 12 lines starting with `- [ ]`; RQ4 the three titled sections are headings; RQ5 passport and charger appear |
| D6 | RQ1 at least 7 paragraphs; RQ2 Bringing Her Home is a heading |
| N2 | (declared negative; its requirement is never scored as a document) |
| R1 | RQ1 exactly one sentence |

A word requirement with several words is one row that needs all of them (as in v4's headings rows).

### 3.3 Inherited rows, none weakened

`rows-v8.jq`, `rows-v5.jq` and `rows-oq3-v3.jq` run unchanged; the v4 rows of ACCEPTANCE-v4 Section 3 are computed by the v4 row
module unchanged (`rows-oq3-v4.jq`): `<id>-SB` saved bytes equal approved bytes, `<id>-F` complete document saved, `<id>-CUT`
accepted reply not cut, `<id>-RQ<k>`, `D4-SRC` source unchanged (the v4 `W4-SRC` rule), `<id>-EO` edit outcome, `<id>-D` one
deadline, `<id>-NC` nothing committed on refusal, `<id>-BE` no CPU, stub or fallback backend per daemon start, `N2-R` token-limit
cut is not a timeout. These cover "real output-byte checks": every positive launch is judged on the sha256 and the text of the
file found in the workspace after the run.

### 3.4 New rows in v5

| Row | Launches | What it requires |
|---|---|---|
| `<id>-PRE` Asked the model | every launch except N1 | the S3 report's `requirements_uncertain` is absent or empty, and at least one attempt exists. A product refusal before the model is named by this row, so the verdict can say "product" rather than "model" |
| `<id>-DEC` Decoding observed greedy | every launch except N1 | every attempt of the S3 report carries `decoding` with `mode` = `greedy`, `sampled_tokens` = 0, `greedy_tokens` at least 1, and no `temperature`, `top_p` or `seed_request_id`; for a committed launch the generation record named by the commit's `provenance.generation_record` carries the same. An absent `decoding` is FAIL (sc#329: absent means no claim). The value is never read from the request, `generation_config.json` or any config |
| `<id>-GR` Generation record binds model and text | committed launches | the commit chain names a generation record (not null); the scorer reads it with `aien compose recall` (verified); its `output_text_sha256` equals the parsed attempt's `text_sha256`; its `model_sha256` equals the frozen value of the file the daemon hashes (Section 7) and the daemon's `checkpoint loaded` line; its `tokenizer_sha256` equals the frozen tokenizer.json sha256. Until sc#338 is fixed this binds the index file only; the verdict says so |
| `<id>-OPR` No fallback, positive evidence | every launch | the daemon log of the launch has the start line of sc#337 with `strict=true`, `dev_fallback_build=false`, `require_checkpoint=true` and a backend containing `OmegaGb10`; every `OP_REPORT` line of the launch has `native_fallbacks=[]` and `reference_runs=[]` and lists all ten native ops; launches that call the model have at least one `OP_REPORT` line; the log has no `STRICT_REAL_MODEL_VIOLATION`. A missing line is FAIL. (The exact line format is fixed at freeze from the merged sc#337.) |
| `<id>-PIN` Build pins | every launch | `run.json` records the sovereign-core commit, the omega, physics and aienos lock commits, the sha256 of `aien`, `np1_reference` and `np1_edit_merge`, and `Cargo.lock`, each equal to its frozen value (Section 7). The wrapper refuses a mismatch; the row makes the check part of the score |
| `<id>-MEAS` Measurements present and consistent | every launch | `run.json` has the launch wall time (monotonic ms), and for each attempt its ms, `tokens`, and `token_ids`; every number is a non-negative integer; the attempts' ms sum to no more than the launch wall time; `tokens` equals the length of `token_ids`; no attempt's `tokens` exceeds the launch's cap; MemFree and Cached are recorded before the part and the daemon's `vmhwm_kb` after it |
| `N1-NR` Refused for the destination | N1 | the refusal reason contains `outside the workspace` and does not contain `uncertain requirement` |
| `N1-NM` No model call | N1 | no attempt and no generation record |

Whether the product recognized a requirement is not a row condition (as in v4): the rows read the saved bytes. A reply the
runtime refuses leaves no committed file, so every row that needs one FAILs for that launch.

### 3.5 Measurement checks of the scorer itself (negative controls)

Before freeze the self-test (`test-v5.sh`, CPU only) must show, for every row kind above, at least one made-up result that
passes and at least one that fails exactly that row (red before green): a document one line short, a missing heading, a word
inside a longer word, a topic form not in the table, an unclosed fence, a paragraph split only by a heading, two sentences in R1,
a `decoding` absent, `mixed` or with one sampled token, a null generation record, a record whose text digest differs, a log with
a fallback count of 1 or no `OP_REPORT` line, a pin off by one character, attempt ms summing above the wall time, and an N1
refused for an uncertain requirement. A row that cannot be made to fail is not a check and blocks the freeze.

## 4. Declaration and tooling

`gen-decl-oq3-v5.sh` writes `../scoring/declarations/oq3-v5.decl.json` from the v8 row shapes plus the rows above. One
repetition, role case, no control, none NOT_APPLICABLE. `test-v5.sh` fails if the committed file differs from the generated
one. The wrapper `run-qwen3-v5.sh`, the row module `rows-oq3-v5.jq` (including `rows-oq3-v4.jq` and `rows-oq3-v3.jq`
unchanged) and `v5-rows.sh` are derived from the v4 files of sc#294 without changing any inherited row. None of these exists in
this draft: they are written after the product fixes of Section 1 merge, so that the log line formats they read are the merged
ones.

QUALIFICATION_ROWS = TO FILL AT FREEZE (generated; per launch counts listed in the generated declaration)

## 5. Verdict

scoring-v5 (`../scoring/score-rows.sh`) over `oq3-v5.decl.json` and the result file only: **PASS only if every declared row is
present once and PASS.** Binding rules, as in v4:

- A failed campaign stays failed: no rerun of a failure, no repeated launch, no retry after seeing a result.
- No task edit after seeing answers. A spec defect found after the run is a finding for a later campaign, never this one.
- All attempts, outputs, exit codes, timings, receipts, logs, generation records and the sha256 of every file in the run base are
  kept in `evidence-v5/`. Nothing is deleted, trimmed or regenerated.
- The legacy receipt `verdict` field is outside the verdict.
- The verdict reports the three conclusions of Section 0 on three separate lines, and for each failing launch says whether the
  first failing row is a product row (`PRE`, `OPR`, `PIN`, `GR`, `N1-NR`) or a model row (`RQ`, `CUT`, `F`, `EO`, `SB`).

## 6. Regression evidence (CPU, reported, not counted)

At freeze the requirement-reader probe is re-run on the frozen commit over all fourteen v4 goals and all eleven v5 goals and
committed next to the 9b5e6e8 probes. For v4 it shows which of the four v4 refusals (W3, W5, N2, R1) the fixes of Section 1
removed; v4's own files and its 10.1 table are not changed. The v4 regression launches (RG2, RG3, RT4, RT6) are not re-run on the
chip in v5.

## 7. Model, environment and build

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, weights unchanged, directory
`/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/`. Expected sha256 (re-hashed and confirmed at freeze; the values are
those of ACCEPTANCE-v4 Section 7, re-hashed there on 2026-10-08):

```text
model.safetensors.index.json             d6c42883a895dfef5b0080ed2116a1bcd764f558406b98923d675978a1abf29c
model-00001-of-00003.safetensors         75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6
model-00002-of-00003.safetensors         0b48adbb1f60e901153d91907ba11ce63bd4b8b584482e730f48808d055dfba1
model-00003-of-00003.safetensors         7dd39ccca5e4de123c74c14af44c9bf2eb75df33b4614382af0134528e060d5d
tokenizer.json                           aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4
tokenizer_config.json                    a62ff0a2472a0fa1b8eaabcb57c59b58afa42a22831dc141400b6e0cf2b65ce3
chat template string alone               64f85b198065d0fba2a81f37e10ed68161ce2c19a754c7100e67e0ca2ee9c326
config.json                              5beea1a4a34c62782bfb2f911c606741a3bab8f92d80a118fa053c28af12e8ba
generation_config.json                   835fffe355c9438e7a25be099b3fccaa98350b83451f9fd2d99512e74f1ade48
```

**What `generation_config.json` holds and how it is used** (review finding on v4: hashed but not described). Its contents:
`bos_token_id` 151643, `eos_token_id` [151645, 151643], `pad_token_id` 151643, `do_sample` true, `temperature` 0.7, `top_k`
20, `top_p` 0.8, `transformers_version` 4.51.0. The product reads only the stop set from it (`model_dir.rs`, `eos_token_ids`);
the sampling fields are the publisher's suggestion for other runtimes and are **not read** by this product. The campaign does not
rely on that reading: row `<id>-DEC` checks the decoding the backend actually took on every attempt.

**Which file the generation record hashes.** The daemon is started with `AIEN_MODEL_PATH` = the index file, and
`model_sha256` is the sha256 of that file (sc#338). Until sc#338 is fixed, row `<id>-GR` compares with the index sha256 above and
the verdict states that the shards are bound by the wrapper's pre-run hash only.

Environment, checked by the wrapper and recorded in `run.json`:

```text
AIEN_KV_CONTEXT_TOKENS                   = 4096
AIEN_REQUIRE_BLACKWELL                   = 1      (a missing GPU engine is fatal)
AIEN_REQUIRE_CHECKPOINT                  = 1      (new in v5: set explicitly; reference weights are fatal)
AIEN_GB10_QWEN3_DECLARED_ATTEMPT         = 1      (opt-in while omega#327 is open)
AIEN_COMPOSE_EDIT_BUDGET_MS              = 29000
AIEN_COMPOSE_DOC_BUDGET_MS               = 120000
AIEN_COMPOSE_MAX_TOKENS                  = 96
AIEN_COMPOSE_DOC_MAX_TOKENS              = 1024 for documents, 16 for N2, 96 for R1
unset: AIEN_FORCE_CPU_STUB, AIEN_DEV_FALLBACK (new in v5), AIEN_COMPOSE_BUDGET_MS, AIEN_OMEGA_SPIN_US, AIEN_OMEGA_CTA_BUDGET
build: release, linked (has_omega_compose, has_omega_gpu, has_omega_wait_ms in the build log), WITHOUT the dev-fallback feature;
       the exact cargo command line is recorded in evidence-v5/build-summary.txt
```

Build pins (current main as of this draft; TO FILL AT FREEZE with the commit that contains the Section 1 fixes):

```text
sovereign-core commit                    = TO FILL AT FREEZE   (draft written on 9b5e6e82359f4ad6b6f03042e83ef6411c5f15ce)
omega.lock commit                        = TO FILL AT FREEZE   (6c6180cf378075b61291f4565d226eba38b4decd at 9b5e6e8)
physics commit                           = TO FILL AT FREEZE   (6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf at 9b5e6e8)
aienos.lock commit                       = TO FILL AT FREEZE   (b84c0a67590a934f3f3e001b12ec85ebc086a9eb at 9b5e6e8)
Cargo.lock sha256                        = TO FILL AT FREEZE
aien-cli sha256                          = TO FILL AT FREEZE
np1_reference sha256                     = TO FILL AT FREEZE
np1_edit_merge sha256                    = TO FILL AT FREEZE
campaign files (wrapper, tasks file, declaration, row modules, generator, self-test, seeds, next-phase-1 driver) sha256 = TO FILL AT FREEZE
GPU hold names, one per part            = TO FILL AT FREEZE
```

## 8. Run plan: one chip slot

Run path pinned at freeze (`RUN_BASE=/home/drakestapleton/workspace/oq3-v5-runs`). One run: no retry, no repeated launch, no
cache drop. Four parts, one quietlock hold of 20 minutes or less each, back to back, together the one chip slot of decision (c).
Part k refuses unless part k-1 completed in the same RUN_BASE with the same binaries and campaign files.

Time estimate, UNVERIFIED, from v3 (a launch costs about 140 s without decoding; documents decoded in 20 to 43 s):

| Part | Launches | Worst case (documents at 120 s, edits at 29 s) | Hold |
|---|---|---|---|
| 1 | D1 D2 D3 | 3 x (140 + 120) s = 13 min | 20 min |
| 2 | D4 D5 D6 | 3 x (140 + 120) s = 13 min | 20 min |
| 3 | E1 E2 N1 | 2 x (140 + 29) + (140 + 120) s = 10 min 8 s | 20 min |
| 4 | N2 R1 and the score | (140 + 120) s + about 14 min if the CPU reference runs to its 96-token cap | 20 min |

If the GPU dry run (Section 9, gate G6) shows more, a part is split before freezing; a part is never extended.

## 9. Freeze gates

All must hold on the commit being frozen. If one fails, the product or the tooling is fixed; **no goal is reworded and no row is
changed to pass a gate.**

- **G1 Asked the model.** The requirement-reader probe on the frozen commit gives no uncertain span for any v5 goal except N1
  (sc#331 to sc#336 merged). Committed as `evidence-v5/requirements-probe-<commit>.txt`.
- **G2 Topic forms.** The product's topic check accepts every accepted form of Section 3.1 for its topic (the
  `topic-forms-probe` re-run with the D3 forms; sc#332).
- **G3 N1 reason.** On the frozen commit a stub-build run of N1 is refused with a reason naming the destination (sc#336).
- **G4 Fallback evidence.** sc#337 merged; a stub-build daemon prints the start line; the line format is copied into Section 3.4
  and `rows-oq3-v5.jq`.
- **G5 Self-test.** `test-v5.sh` passes, including every red case of Section 3.5, freshness of every v5 goal against every
  earlier tasks file, acceptance file, verdict and the diagnostic (D4 shares the first name Priya with v4 W4: a name, declared
  here, not a reused task), and the wrapper's refusals.
- **G6 GPU dry run.** One dry run of the exact wrapper in dry mode on the chip, with Drake's approval and its own holds:
  measures part timings, confirms `decoding` reaches `steps/S3.json` attempts on the GB10 path, the generation record is
  written, and the OP_REPORT lines appear. Not a qualification run; its outputs are kept and never scored.
- **G7 Pre-run gate.** `cargo fmt --all --check`, clippy `-D warnings`, the stub and linked test suites reported separately,
  `test-rows-v8.sh`, `test-v3.sh`, `test-v5.sh` on the frozen build; combined build from clean checkouts at the pins.
- **G8 Review and freeze.** An independent review of this file, the tooling and the gate evidence; then every "TO FILL AT
  FREEZE" is filled, the status line changes, and that change alone merges with Drake's go-ahead.

## 10. Prediction (stated before any run; UNVERIFIED)

Nothing here is measured. On today's product (9b5e6e8) the verdict would be FAIL with certainty: 8 of 11 launches never reach
the model (Section 0), and `<id>-OPR` has no line to read. After the Section 1 fixes, from v3 and v4 judgment only: N1 and N2
about 85 percent, R1 about 75 percent, E1 about 70 percent, E2 about 55 percent (the end-of-list position), D2 about 35
percent, D4 about 40 percent, D1, D3, D5 about 35 percent each (line counts are the usual risk), D6 about 45 percent. Whole
qualification PASS: a few percent. A FAIL is the expected and acceptable outcome of an honest campaign.

## 11. What v5 does not prove

- One run, one launch per task: one observation each, not a rate. One model, one machine.
- `<id>-DEC` shows the backend's own count of how it chose each token. It is the daemon's report, not signed, and covers the
  scheduler path only (DAEMON_GENERATION_RECORD.md, "What it does not prove").
- `<id>-GR` binds the index file of the checkpoint, not the shards, until sc#338 is fixed.
- `<id>-OPR` shows the daemon's own accounting, not an independent measurement of where each op ran.
- The topic, paragraph and sentence rules are stated narrow readings, not judgements of quality.
- A PASS would not close omega#327, omega#277 or sc#277, would not turn Qwen3 on GB10 on by default, and would not claim release
  readiness.

## 12. Independence and findings recorded while preparing

- **Goals.** The eleven goals, destinations and seeds were written by an independent author (a Sonnet 5.5 worker) that was told
  not to open any repository, so no goal was shaped to the requirement reader. The spec author (session fb5693) **did** read
  `requirements.rs`, `requirements_extract.rs` and `spine.rs` while finding the causes of the v4 refusals, and therefore did not
  write or edit any goal. The only post-authoring changes are the two topic-form changes of Section 3.1, made before any probe of
  their effect and before any run.
- **Probes.** Both probes are throwaway examples calling the public functions on an unmodified checkout (sources and sha256 in
  the evidence files). CPU only, quietlock clear.
- **Silent misses (P3)** are worse than refusals: the product would approve a v4 W1, W4 or W5 document missing the stated words.
  The campaign's rows catch it; the product fix is sc#333.
- **N1 may be refused for the wrong reason (P6).** On 9b5e6e8 the reader marks the N1 goal uncertain; if that refusal comes
  before the destination check, v4's N1-C row would pass on the wrong refusal. v5 adds `N1-NR`.
- **The model's own sampling suggestion** (`generation_config.json`: sampled, temperature 0.7) differs from the campaign's
  greedy decoding; the product does not read those fields, and `<id>-DEC` checks the result directly.
- v4 is not modified by this change; no product code is modified by this change.
