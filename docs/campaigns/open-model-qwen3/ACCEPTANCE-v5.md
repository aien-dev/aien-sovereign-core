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
- **The v4 finding is wider than four tasks.** The wording of the v5 goals below is by an independent author who did not open
  any repository (brief: `evidence-v5/task-author-brief.md`). On origin/main 9b5e6e8 (which contains #295, #319, #293 and #329)
  the requirement reader marks **7 of the 11** goals uncertain (`evidence-v5/requirements-probe-v5-goals.txt`): **5 of the 9
  goals that should reach the model** (D1, D3, D5, D6, R1) are refused before any model call; N2, which should be stopped by its
  16-token cap, is stopped by the reader instead (the wrong reason); N1 is also marked, but the product refuses it earlier for
  its destination (Section 3.3), so that mark has no effect. On the same commit the v4 table is reproduced exactly
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
| P6 | One-sentence and one-line files refused before the model (`a one-sentence reminder`, `just one line that says "..."`). The second wording only matters for a goal whose destination is valid; on 9b5e6e8 v5 N1 is refused for its destination first (`spine.rs`, `task_decision` runs before `analyze`), and gate G3 keeps that order checked | v4 R1; v5 R1 (N1 wording) | issue sc#336 |
| P7 | No positive fallback evidence: the daemon records neither strict mode nor op accounting (`OP_REPORT`) | every launch (row `<id>-OPR`) | issue sc#337 |
| P8 | The generation record binds only the index file of a sharded checkpoint (`model.safetensors.index.json`), not the three weight shards | every committed launch (row `<id>-GR`) | issue sc#338 |

**All eight causes are fixed on main** (gate G1, `evidence-v5/requirements-probe-b2cae6d.txt`): P1 sc#331 by #343, P2 sc#332
by #347, P3 sc#333 by #345, P4 sc#334 by #340, P5 sc#335 by #341, P6 sc#336 by #348, P7 sc#337 by #350, P8 sc#338 by #351;
b2cae6d holds all eight. The P2 follow-up (the topic check refused three accepted forms of withdraw, gate G2) is fixed by sc#354 (Section 9).
Since sc#342 (b0cee16) the approval desk is required: the next-phase-1 driver creates the desk key and signs with `--desk 1`, and
the v5 wrapper refuses the dev-only opt-out `AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK`.

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
16 tokens (budget exhaustion); R1 a document cap of 96 (bounded CPU reference replay). One declared adjustment: D6 runs at 170 s
and 1536 tokens (Section 2.1). The wrapper sets both budgets and both caps for every launch from the task file (the task's own
`max_tokens` and `budget_ms` for its kind, the product values for the other kind); the retired `AIEN_COMPOSE_BUDGET_MS` must be unset.

| id | kind | cap | budget | seed | expected completion |
|----|------|-----|--------|------|---------------------|
| D1 | document: line count, three titled sections, two stated words | 1024 | 120 s | none | DONE |
| D2 | document: three fenced code examples with titled sections, a closing section of at least 12 words | 1024 | 120 s | none | DONE |
| D3 | document: line count, four named topics with stated word forms | 1024 | 120 s | none | DONE |
| D4 | document created from a different existing file; titled sections; three names | 1024 | 120 s | `seed-v5/D4/notes/choir-rehearsal.txt` | DONE |
| D5 | document: line range, item prefix and item count, titled sections, two stated words | 1024 | 120 s | none | DONE |
| D6 | document: paragraph count, one titled section | 1536 | 170 s | none | DONE |
| E1 | small edit: add a bullet to one section | 96 | 29 s | `seed-v5/E1/repairs/tap-repair.md` | DONE |
| E2 | small edit, nested path: add a numbered item at the end of a list | 96 | 29 s | `seed-v5/E2/crafts/yarn/scarf.md` | DONE |
| N1 | declared negative: destination outside the workspace | 1024 | 120 s | none | REFUSED, for the destination |
| N2 | declared negative: budget exhaustion | 16 | 120 s | none | REFUSED, token limit |
| R1 | token-identity replay against the CPU reference; one sentence | 96 | 120 s | `seed-v5/R1/notes/README.txt` (the `notes/` folder only) | DONE |

Task rows in the format the inherited Q2 check reads (`make-receipt.sh` builds this exact line from the task file and requires it
verbatim in the acceptance file; generated from `tasks-oq3-v5.json` with the same jq). Phrases are matched as case-insensitive
substrings after whitespace is collapsed (`rows-v5.jq`, Q2), so `shoulder` matches "shoulders" and `saving` matches "savings".

| id | goal (exact text given to the driver; written by the independent author, unchanged) | requested destination | required phrases (all) |
|----|------|------|------|
| D1 | `Please create the file docs/knife-sharpening.md, a short how-to guide on sharpening a kitchen knife by hand. It should have at least 22 non-empty lines and three sections titled "Gather Your Tools", "Sharpen the Edge" and "Test and Store". Make sure the words whetstone and angle both appear somewhere in it.` | `docs/knife-sharpening.md` | `Gather Your Tools`, `Sharpen the Edge`, `Test and Store`, `whetstone`, `angle` |
| D2 | `I need a technical how-to saved as docs/rename-photos.md that shows how to rename a folder of holiday photos with a bash loop. Write at least 18 lines in total and include 3 fenced code examples, where each example is introduced by its own section titled "Example One: Date Prefix", "Example Two: Lowercase Names" and "Example Three: Number the Files" in that order. After the last code example, add a final section titled "Wrap Up" containing at least 12 words of plain text and no code.` | `docs/rename-photos.md` | `Example One: Date Prefix`, `Example Two: Lowercase Names`, `Example Three: Number the Files`, `Wrap Up` |
| D3 | `Write an explainer for beginners called docs/savings-basics.md about how a savings account works. It must cover four topics: interest, deposit, withdraw and balance, and the file needs at least 18 lines. A topic counts as covered when its word or another form of that word appears, for example interests or interested for the first one, deposits or deposited for the second, withdrew or withdrawing for the third, and balances or balancing for the last.` | `docs/savings-basics.md` | `saving` |
| D4 | `There is a file at notes/choir-rehearsal.txt with my rough notes from choir practice. Please create a new file called docs/choir-summary.md that summarises it, and leave the original alone. The summary needs at least 13 lines, two sections titled "Key Points" and "Next Steps", and must mention Marta, Owen and Priya by name.` | `docs/choir-summary.md` | `Key Points`, `Next Steps` |
| D5 | `Make me a carry-on packing checklist in docs/packing-carry-on.md. Keep it between 16 and 30 lines long. Every item must be its own line starting with "- [ ]" and I want at least 12 of those items. Organise it under three sections titled "Documents", "Clothes" and "Electronics", and be sure the words passport and charger show up.` | `docs/packing-carry-on.md` | `- [ ]`, `Documents`, `Clothes`, `Electronics`, `passport`, `charger` |
| D6 | `Please write a warm, longer essay about adopting a rescue cat and save it as docs/rescue-cat-essay.md. It should run to at least 7 paragraphs, and one section must be titled "Bringing Her Home".` | `docs/rescue-cat-essay.md` | `Bringing Her Home` |
| E1 | `In the existing file repairs/tap-repair.md, add the bullet line "- Thread seal tape for the spout" under the section called "Parts" and leave everything else as it is.` | `repairs/tap-repair.md` | `- Thread seal tape for the spout` |
| E2 | `Open crafts/yarn/scarf.md and put one more numbered item, "4. Cast off loosely and weave in the ends.", at the end of the list under the heading "Steps". Don't change the other sections.` | `crafts/yarn/scarf.md` | `4. Cast off loosely and weave in the ends.` |
| R1 | `Save a one-sentence reminder about stretching the shoulders before swimming into notes/swim-tip.txt.` | `notes/swim-tip.txt` | `shoulder` |

N1 goal (declared negative, not a Q2 row): `Create a file at ../shared-stuff/reminder.txt containing just one line that says "Renew the library card on Friday."`
N2 goal (declared negative, not a Q2 row): `I would like a long, detailed history of the marathon as a race, from the legend of ancient Greece to modern city events, saved as docs/marathon-story.md. Please make it at least twelve paragraphs long.`

Seeds (each edit seed has three level-two headings; the D4 source names the three people; the R1 seed only provides the
`notes/` folder its destination needs, see the known limit below):

```text
seed-v5/D4/notes/choir-rehearsal.txt     sha256 aa8780bbc1c587cc6f980e44d838b1dfc39e11d06365d3471126008c9b99b7b7
seed-v5/E1/repairs/tap-repair.md         sha256 4766451b85a3cabde953959bb75c990fbf9f82e1caf41554c9a695e8937b91b9
seed-v5/E2/crafts/yarn/scarf.md          sha256 3a5a8494fbe60908c9a9431ac4839f17e4962a5a51f22399662087073cf00909
seed-v5/R1/notes/README.txt              sha256 d376553dc23c5cc230cfa60ec0f11cc0f0671b7f8bcbb3fc6f8bc46232ab6b0d
```

Known product limit (found by the extra hold `a7c5d201-oq3-v5-extra-p1`, 2026-10-09): the product refuses to write into a
folder that does not exist. The compose step accepts such a destination as a new document (`spine::classify_destination`
treats a missing path whose nearest existing parent is inside the workspace as `New`), but authorize then refuses it
(`EFFECT_REFUSED OutsideWorkspace: target directory of <ws>/notes/swim-tip.txt ... (missing)`). v5 does not test or
change this behaviour. Every positive destination's folder therefore exists before its launch: the root and `docs/` come
from the default workspace that `run-campaign.sh` builds, `repairs/` and `crafts/yarn/` from the E1 and E2 seeds, and
`notes/` from the R1 seed (one placeholder file; the R1 goal text is unchanged and the prompt carries no file listing, so
the identity rows are unaffected). `test-v5.sh` checks this for every positive task. The receipt builder stages every
non-edit seed (D4, R1) as null, as it did for D4 (the wrapper's `STAGE_FILTER`).

### 2.1 Declared adjustment: D6 answer budget (Drake, 2026-10-09T00:15:14Z)

D6 runs at **1536 output tokens and 170 s** (170000 ms) instead of the document product values of 1024 tokens and 120 s.
Decided by Drake at 2026-10-09T00:15:14Z, before the freeze, in his words: "Option 1 approved. Raise D6's limit to 1,536
output tokens and 170 seconds. Apply the change before the evaluation package is locked. A task should be judged on the
model's ability to complete it, not on an insufficient execution budget. The additional runtime is justified. Keep all
other evaluation conditions unchanged, document the adjustment, and apply the same limits to every model evaluated on D6
to preserve fairness."

| field (`tasks-oq3-v5.json`, D6) | old | new |
|---|---|---|
| `max_tokens` | 1024 | 1536 |
| `budget_ms` | 120000 | 170000 |

- D6 only. These are the only two fields changed for this adjustment; every other task's limits are byte-identical.
- The same limits for every leg that scores D6: the wrapper passes D6's own `max_tokens` and `budget_ms` as
  `AIEN_COMPOSE_DOC_MAX_TOKENS` and `AIEN_COMPOSE_DOC_BUDGET_MS` for the launch, and row `D6-D` scores against the same
  `budget_ms`. There is no per-backend or per-model override; any other model evaluated on D6 uses 1536 tokens and 170 s.
  D6 has no CPU reference leg (only R1 does).
- Within the product's limits (`spine.rs`: document tokens 16 to 4096, budget 1000 to 599000 ms). D6's prompt is 148
  tokens (G6 dry run), so 148 + 1536 fits the 4096-token KV context. Part 2 stays inside its 20-minute hold (Section 8).

## 3. Rows and exactly how each is counted

Every row is computed by the scorer from the files the run leaves, never from a runtime verdict. The scorer computes every
sha256 itself. A launch with no `run.json` FAILs every declared row of that launch. Absent evidence is FAIL, never PASS.

### 3.1 Counting rules (saved bytes)

The v4 rules (ACCEPTANCE-v4 Section 3) apply unchanged: lines, sections (headings, fenced code skipped), stated word (whole
word, ASCII case-insensitive, neighbours not a letter or digit), topic (accepted forms only), code blocks (complete fenced
blocks), closing section after the last example (`tail`), item lines (`prefixed_lines`), maximum lines. New in v5:

- **Paragraph** (`paragraphs`): outside fenced code blocks and fence lines, a maximal run of consecutive prose lines. A prose
  line is a non-empty line (a line with at least one non-whitespace character) that is not a heading line and not a list item
  (after leading whitespace it does not start with `- `, `* `, `+ ` or digits followed by `.` or `)` and a space). Blank lines
  (including whitespace-only lines), heading lines and list-item lines end a run and are not part of any paragraph. This is the
  rule asked of the product in sc#335. `at least N paragraphs` passes when the count is N or more.
- **Heading order** (`heading_order`, D2): for each listed title take the first heading (same title rule as sections) that
  matches it; all are present and their line numbers increase in the listed order.
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
| D2 | RQ1 at least 18 non-empty lines; RQ2 at least 3 complete fenced code blocks; RQ3 the three example titles are headings; RQ4 after the last code block a Wrap Up heading followed by at least 12 words; RQ5 the three example headings are in the stated order |
| D3 | RQ1 at least 18 non-empty lines; RQ2 to RQ5 the topics interest, deposit, withdraw, balance |
| D4 | RQ1 at least 13 non-empty lines; RQ2 Key Points and Next Steps are headings; RQ3 Marta, Owen and Priya appear |
| D5 | RQ1 at least 16 and RQ2 at most 30 non-empty lines; RQ3 at least 12 lines starting with `- [ ]`; RQ4 the three titled sections are headings; RQ5 passport and charger appear |
| D6 | RQ1 at least 7 paragraphs; RQ2 Bringing Her Home is a heading |
| N2 | none: the paragraph requirement is recorded in the task file for the CPU probe (gate G1) and is never scored, because N2 is a declared negative |
| R1 | RQ1 exactly one sentence (the subject is checked only by the inherited Q2 phrase `shoulder`) |

A word requirement with several words is one row that needs all of them (as in v4's headings rows). Two parts of the D2 goal are
not separate rows: "no code" in Wrap Up holds by construction for fenced code (RQ4 reads only text after the last complete
block), and inline backticks are not scored; "its own section" for each example is not scored beyond RQ3 and RQ5 (which section
holds a block is not scored, as in v4).

### 3.3 Inherited rows: none weakened; one declared exception for N1, other N1 rows confirmed at gate G3

`rows-v9.jq`, `rows-v5.jq` and `rows-oq3-v3.jq` run with one declared change: the compose-dir allowlist of the containment rows
(`v1 Containment: speculation` in `make-receipt.sh`, `<id>-CM`, `N1-Z`, `N2-Z` through `v6_stray`, and `A1`) also accepts the exact
name `./approval-desk.key`, the approval-desk key the harness itself creates since sc#342 (`run-campaign.sh`), not the model. Any
other extra file, including a second `.key` name, is still stray. The allowlist also matches the harness names exactly
(`./machine.id`, `./cortex.cx`, `./jspace`, `./jspace/jspace.data`, `./jspace/jspace.meta`, `./approval-desk.key`); before, a name
that only started like one (`./machine.idX`, `./jspace/other`) passed. A leftover `./jspace/jspace.meta.tmp` (an interrupted
checkpoint write) is stray. The GPU dry run (gate G6) found that without this the key made
20 declared rows FAIL (every containment row of every launch). `rows-v9.jq` is `rows-v8.jq` with only that line changed; `rows-v8.jq` stays byte-identical
because completed runs pin its sha256. Otherwise the rows run unchanged; the v4 rows of ACCEPTANCE-v4 Section 3 are computed by the v4 row
module unchanged (`rows-oq3-v4.jq`): `<id>-SB` saved bytes equal approved bytes, `<id>-F` complete document saved, `<id>-CUT`
accepted reply not cut, `<id>-RQ<k>`, `D4-SRC` source unchanged (the v4 `W4-SRC` rule), `<id>-EO` edit outcome, `<id>-D` one
deadline, `<id>-NC` nothing committed on refusal, `<id>-BE` no CPU, stub or fallback backend per daemon start, `N2-R` token-limit
cut is not a timeout. These cover "real output-byte checks": every positive launch is judged on the sha256 and the text of the
file found in the workspace after the run.

**One declared exception: where N1 is refused.** In v3 the product refused an outside-the-workspace goal at a model attempt, and
v8's `N1-B` ("at least one attempt refused with a reason naming outside the workspace", `rows-v8.jq`) and v4's `<id>-D` ("at least
one attempt") were written for that. On 9b5e6e8 the product refuses earlier: `task_decision` (`spine.rs`) runs before any
requirement reading or model call and returns `RunComposeTask: destination ../shared-stuff/reminder.txt cannot be written safely
(not a plain relative path inside the workspace); refusing` (reproduced on CPU by the reviewer of this draft). A correct refusal
made before the model can never pass `N1-B` or `N1-D`. For N1 only, those two rows are replaced by `N1-NR` and `N1-NM`
(Section 3.4), which test the same property (refused at the workspace boundary, nothing committed) where the product now refuses,
and are stricter on one point (no model call at all). This is a scorer change caused by a product change, declared here before
any run, with red and green controls (Section 3.5); it is not a relaxation and is flagged for the reviewer and Drake at freeze.
Which of the other inherited N1 rows (`N1-C`, `N1-Z`, `N1-E`, `N1-H`, v4 `N1-NC`) can read a refusal that leaves no S3 report is
confirmed at gate G3 by a stub-build run; any that cannot is listed and replaced the same declared way, never dropped silently.
**Gate G3 result** (CPU stub run through the unchanged driver, `evidence-v5/g3-n1-stub-run.txt`). The refusal lands in
`steps/S3.json`, field `error` (`ok` false, step `propose`, S3 exit 1), verbatim: `RunComposeTask: destination
../shared-stuff/reminder.txt cannot be written safely (not a plain relative path inside the workspace); refusing`. No S3 report is
written, S4 to S6 exit non-zero, the workspace is unchanged. On that output `N1-Z` and `N1-E` PASS and stay declared. Three more
v8 rows cannot read a refusal that leaves no S3 report and are replaced the declared way: `N1-B` by `N1-NR`, `N1-C` (completion
state from the S3 report) by `N1-RC` (completion state from the step exit codes), `N1-H` (machine id from the S3 report) by
`N1-RH` (machine id from the pre-restart and S8 recalls); v4's `N1-D` is replaced by `N1-NM` and v4's `N1-NC` is kept. The
unchanged receipt still computes `N1-B`, `N1-C` and `N1-H`; they are not declared, so the scorer lists them as extras, outside
the verdict.

The same product change means v4's N1 would now fail `N1-B` on 9b5e6e8; v4 is not edited, and this is recorded as a v4 finding
(Section 12).

### 3.4 New rows in v5

| Row | Launches | What it requires |
|---|---|---|
| `<id>-PRE` Asked the model | every launch except N1 | the S3 report has the field `requirements_uncertain` and it is an empty list (the field is always written, so an absent field is FAIL), and at least one attempt exists. A product refusal before the model is named by this row, so the verdict can say "product" rather than "model" |
| `<id>-DEC` Decoding observed greedy | every launch except N1 | **On every attempt** of the S3 report: `decoding` is present and not null; `decoding.greedy_tokens` is at least 1; `decoding.sampled_tokens` is 0; `temperature`, `top_p` and `seed_request_id` are null or absent (an attempt's `decoding` is the plain `DecodeObservation`, which has no `mode` field). An attempt with no `decoding` (timed out, errored, or not observed) FAILs the row. **On the generation record** named by a committed launch's `provenance.generation_record`: `decoding.mode` is `greedy` and `decoding.sampled_tokens` is 0. sc#329: absent means no claim, so absent is FAIL. The value is never read from the request, `generation_config.json` or any config |
| `<id>-GR` Generation record binds model and text | committed launches | the S3 report names a generation record (not null), and the commit record (`compose_commit`) and every authorization carry `provenance.generation_record` equal to it; the record is in the S8 recall (`aien compose recall`), verified, `generation` 1, `origin` `compose_proposal`, its `task` and `attempt` those of the accepted (parsed) attempt; its `output_text_sha256` equals that attempt's `text_sha256`; `model_digest_kind` is `index+shards` and `model_sha256` equals the frozen manifest digest (Section 7, `frozen-v5.json`); `tokenizer_sha256` equals the frozen tokenizer.json sha256; every daemon start's `checkpoint loaded` line carries the same `model_sha256` and `tokenizer_sha256`, and its log has exactly one line `CHECKPOINT_SHARDS model_sha256=<m> model_digest_kind=index+shards index_sha256=<i> shards=[<name>:<sha256>,...]` equal to the frozen one (the three shards of Section 7, in order) |
| `<id>-OPR` No fallback, positive evidence | every launch | for **every daemon start** of the launch (one log per entry of `run.json` `daemon`: `daemon-1.log`, `daemon-2.log`, as `<id>-BE` counts them), read after removing colour codes and leading spaces: the log has exactly one start line `STRICT strict=<b> dev_fallback_build=<b> require_checkpoint=<b> backend=<name>` and it reads `strict=true dev_fallback_build=false require_checkpoint=true` with a backend containing `OmegaGb10` (the GB10 backend names itself `OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)`; the CPU stub prints `ReferenceCpuBackend`, gate G3), and no `STRICT_REAL_MODEL_VIOLATION` line; every line `OP_REPORT native=[a,b,..] reference=[..] native_fallbacks=[name:count,..] reference_runs=[..] backend=<name>` (one per model call) lists all ten native ops in `native` (`rmsnorm`, `apply_rope`, `matmul_vec`, `matmul_batch`, `swiglu`, `gqa_attention`, `paged_attention`, `paged_attention_batch`, `compute_logits`, `rmsnorm_heads`), has `native_fallbacks=[]` and `reference_runs=[]` and a backend containing `OmegaGb10`; a launch that calls the model (every launch except N1) has at least one `OP_REPORT` line. A missing line or log is FAIL. Formats copied from the merged sc#337 (#350, `server.rs`; gate G4) |
| `<id>-PIN` Build pins | every launch | the wrapper's launch record `<id>.launch-v5.json` (the driver `run-campaign.sh` is unchanged and records no pins) holds `pins`: the sovereign-core commit, the omega, physics and aienos lock commits, and the sha256 of `Cargo.lock`, `aien`, `np1_reference` and `np1_edit_merge`; each of the eight frozen values in `frozen-v5.json` is filled (40 or 64 lowercase hex, not the placeholder) and equals the recorded one. The wrapper refuses a mismatch before any launch; the row makes the check part of the score |
| `<id>-MEAS` Measurements present and consistent | every launch | the launch record `<id>.launch-v5.json` has `wall_ms` (launch wall time on CLOCK_BOOTTIME from `/proc/uptime`, ms) and `meminfo_before` `mem_free_kb` and `cached_kb` (`/proc/meminfo` before the part); every daemon start in `run.json` has `vmhwm_kb`; every attempt of the S3 report has `ms`, `tokens` and `token_ids`; every number is a non-negative integer; `tokens` equals the length of `token_ids` and is at most the launch's `max_tokens`; the attempts' ms sum to no more than `wall_ms` |
| `E2-EP` Edit position | E2 | in the saved text the last non-empty line after `## Steps` and before the next heading equals the new line (Section 3.1, Edit position) |
| `N1-NR` Refused for the destination | N1 | the propose step failed (S3 exit code in `run.json` recorded as a number other than 0, `steps/S3.json` `ok` false) and `steps/S3.json` `error` equals exactly the string recorded at gate G3: `RunComposeTask: destination ../shared-stuff/reminder.txt cannot be written safely (not a plain relative path inside the workspace); refusing`. Any other text, for example an uncertain-requirement refusal, is FAIL. Replaces `N1-B` for N1 (Section 3.3) |
| `N1-NM` No model call, nothing committed | N1 | no attempt, no generation record named in an S3 report or present in the S8 recall (`steps/S8.json` must exist; a missing recall is FAIL), no authorization record, no S5 path, and nothing at `../shared-stuff/reminder.txt` resolved against the workspace. Replaces v4 `N1-D` for N1 (Section 3.3) |
| `N1-RC` Completion state REFUSED, read from the steps | N1 | S3, S4 and S5 are recorded in `run.json` and each has a recorded exit code other than 0 (a missing exit code is FAIL); no authorization record; no S5 path; `containment.workspace_changed` is empty. Replaces v8 `N1-C` (Section 3.3) |
| `N1-RH` Daemon healthy after the refusal | N1 | the pre-restart recall (`steps/pre-restart-recall.json`) and the S8 recall after the restart are both `ok` and return the S1 constraint (`run.json` `constraint_text`) byte-identical and verified, with the same `machine_id`. Replaces v8 `N1-H` (Section 3.3) |

Whether the product recognized a requirement is not a row condition (as in v4): the rows read the saved bytes. A reply the
runtime refuses leaves no committed file, so every row that needs one FAILs for that launch.

### 3.5 Measurement checks of the scorer itself (negative controls)

Before freeze the self-test (`test-v5.sh`, CPU only) must show, for every row kind above, at least one made-up result that
passes and at least one that fails exactly that row (red before green): a document one line short, a missing heading, a word
inside a longer word, a topic form not in the table, an unclosed fence, a paragraph split only by a heading, two sentences in R1,
an attempt whose `decoding` is null or has one sampled token, a record whose `decoding.mode` is `mixed`, swapped example headings in D2, a missing `requirements_uncertain` field, a null generation record, a record whose text digest differs, a log with
a fallback count of 1 or no `OP_REPORT` line, a pin off by one character, attempt ms summing above the wall time, and an N1
refused for an uncertain requirement or after a model attempt. A row that cannot be made to fail is not a check and blocks the freeze.
`test-v5.sh` holds these cases: each product-row red case changes one thing in a passing synthetic launch and checks that exactly
the named rows fail; each document fixture in `selftest-v5/` names the rows it must fail.

## 4. Declaration and tooling

`gen-decl-oq3-v5.sh` writes `../scoring/declarations/oq3-v5.decl.json` from the v8 row shapes plus the rows above. One
repetition, role case, no control, none NOT_APPLICABLE. `test-v5.sh` fails if the committed file differs from the generated
one. The wrapper `run-qwen3-v5.sh`, the row module `rows-oq3-v5.jq` (including `rows-oq3-v4.jq` and `rows-oq3-v3.jq`
unchanged) and `v5-rows.sh` are derived from the v4 files of sc#294 without changing any inherited row. They were written after
the product fixes of Section 1 merged and read the merged line formats. `v5-rows.sh` computes the extra rows of a launch from the
run directory, the daemon logs and the wrapper's launch record `<id>.launch-v5.json`; the frozen model values and pins come from
`frozen-v5.json`. `tasks-oq3-v5.json` carries the machine fields the inherited row modules read (Section 12).

QUALIFICATION_ROWS=334

Per launch: D1 34, D2 36, D3 36, D4 35, D5 36, D6 33, E1 33, E2 34, R1 32, N1 11, N2 14 (generated; `test-v5.sh` checks both).

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
`/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/`. Expected sha256 (the values of ACCEPTANCE-v4 Section 7, re-hashed there on 2026-10-08; re-hashed again for v5 on
2026-10-08T21:39Z with no GPU hold, all nine identical: `evidence-v5/model-rehash-2026-10-08.txt`):

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

**What the generation record binds** (sc#338, fixed by #351). The daemon is started with `AIEN_MODEL_PATH` = the index file.
For a sharded checkpoint `model_sha256` is the manifest digest over the index and every shard (`docs/DAEMON_GENERATION_RECORD.md`),
the record says `model_digest_kind=index+shards`, and the daemon logs `CHECKPOINT_SHARDS model_sha256=<m>
model_digest_kind=index+shards index_sha256=<i> shards=[<name>:<sha256>,...]`. Expected manifest digest:

```text
model_sha256 (index+shards)              17a78fbba447a4e66a3d886c0998fbcf2f9201d46e5e0c5bfec2d57c975976b7
```

COMPUTED FROM LISTED SHA256s, NOT RE-HASHED: the value is the manifest built from the index and shard sha256 values listed
above, not from the files. Re-hash: DONE 2026-10-08T21:39Z (`evidence-v5/model-rehash-2026-10-08.txt`;
the four files equal the listed values and the manifest rebuilt from the re-hash is this value). Confirming it against the
daemon's own `CHECKPOINT_SHARDS` line = DONE in the G6 GPU dry run 2026-10-08T22:08-22:30Z: all 22 daemon starts logged a `CHECKPOINT_SHARDS` line equal to these values. G6 ran the 4770703 build; from 4770703 to the re-pin e27bda8 the only product source change is `crates/aien-runtime/src/destination.rs` (plus tests), so the code that logs the line (`aien-cli` `commands.rs`) and the locks are unchanged. Row `<id>-GR` binds the index and all three shards through it (`frozen-v5.json`).

Environment, checked by the wrapper and recorded in `run.json`:

```text
AIEN_KV_CONTEXT_TOKENS                   = 4096
AIEN_REQUIRE_BLACKWELL                   = 1      (a missing GPU engine is fatal)
AIEN_REQUIRE_CHECKPOINT                  = 1      (already exported by the next-phase-1 driver, run-campaign.sh; the v5 wrapper also checks it; reference weights are fatal)
AIEN_GB10_QWEN3_DECLARED_ATTEMPT         = 1      (opt-in while omega#327 is open)
AIEN_COMPOSE_EDIT_BUDGET_MS              = 29000
AIEN_COMPOSE_DOC_BUDGET_MS               = the task budget_ms: 120000, D6 170000 (set per launch)
AIEN_COMPOSE_MAX_TOKENS                  = 96
AIEN_COMPOSE_DOC_MAX_TOKENS              = the task max_tokens: 1024 for documents, D6 1536, 16 for N2, 96 for R1
unset: AIEN_FORCE_CPU_STUB, AIEN_DEV_FALLBACK (new in v5), AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK (sc#342 dev-only opt-out),
       AIEN_COMPOSE_BUDGET_MS, AIEN_OMEGA_SPIN_US, AIEN_OMEGA_CTA_BUDGET
build: release, linked (has_omega_compose, has_omega_gpu, has_omega_wait_ms in the build log), WITHOUT the dev-fallback feature;
       the exact cargo command line is recorded in evidence-v5/build-summary.txt
```

Build pins (FILLED IN A DRAFT, NOT FROZEN: main 30258fc holds the Section 1 fixes, sc#353, sc#354, the G6 fixes sc#357, sc#358, sc#359 and the
extra-hold fix sc#360 (CPU reference prompt, example only: no product source changed since e27bda8, aien-cli bytes equal); built from clean
clones by `evidence-v5/build.sh`, output `evidence-v5/build-summary.txt`; the same values are in `frozen-v5.json`, and G7
rebuilds at these pins):

```text
sovereign-core commit                    = 30258fce8d373bbd01ede59076dddce6e8a64901
omega.lock commit                        = 6c6180cf378075b61291f4565d226eba38b4decd   (unchanged since v4)
physics commit                           = 6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf   (physics.lock at omega 6c6180c)
aienos.lock commit                       = b84c0a67590a934f3f3e001b12ec85ebc086a9eb   (aienos.lock at omega 6c6180c)
Cargo.lock sha256                        = 49d97bf30113b1727fcfc0e33be79d9446ae13651a08afc32bba889b77fca265
aien-cli sha256                          = 62d98e0b92372862c4a1f7d762a92ec8cddcf398dced8048f75e086b2d66bc55 (unchanged at 30258fc)
np1_reference sha256                     = a684b55af6efb1cea91ee119352552e7462c50026f2c91d38010b40c4e3fb7b9   (new at 30258fc: sc#360)
np1_edit_merge sha256                    = 66be8da4d3c5c7c32089e851acdbed9c050afa38f2d7db9e8fbced3811949e80
campaign files sha256                    = evidence-v5/campaign-files.sha256 (wrapper, tasks file, declaration, row modules,
                                           generator, self-test, seeds, frozen values, next-phase-1 driver, receipt builder with rows-v5.jq and rows-v9.jq, scorer)
GPU hold names, one per part            = TO FILL AT FREEZE   (proposed <runner session>-oq3-v5-p1 to -p4, 20 minutes each)
```

A real run refuses while any pin holds the placeholder and also while the `frozen-v5.json` status does not say FROZEN
(`run-qwen3-v5.sh`; filled pins alone are not a freeze). The pins describe the product build (sovereign-core 30258fc and its
locks); the campaign files are bound separately by `evidence-v5/campaign-files.sha256` and the wrapper's identity file, so a
later campaign-file commit on top of 30258fc does not change a pin.

## 8. Run plan: one chip slot

Run path pinned at freeze (`RUN_BASE=/home/drakestapleton/workspace/oq3-v5-runs`). One run: no retry, no repeated launch, no
cache drop. Four parts, one quietlock hold of 20 minutes or less each, back to back, together the one chip slot of decision (c).
Part k refuses unless part k-1 completed in the same RUN_BASE with the same binaries and campaign files. The parts below are
the ones `run-qwen3-v5.sh` runs (`OQ3_PART` 1 to 4). Before each part the wrapper writes the build pins and `/proc/meminfo`, and
for each launch `<id>.launch-v5.json` (wall time, driver exit, memory before the part, pins).

Time estimate, UNVERIFIED, from v3 (a launch costs about 140 s without decoding; documents decoded in 20 to 43 s):

| Part | Launches | Worst case (documents at 120 s, D6 at 170 s, edits at 29 s) | Hold |
|---|---|---|---|
| 1 | D1 D2 D3 | 3 x (140 + 120) s = 13 min | 20 min |
| 2 | D4 D5 D6 | 2 x (140 + 120) + (140 + 170) s = 13 min 50 s | 20 min |
| 3 | E1 E2 N1 | 2 x (140 + 29) + (140 + 120) s = 9 min 58 s | 20 min |
| 4 | N2 R1 and the score | (140 + 120) s + about 14 min if the CPU reference runs to its 96-token cap | 20 min |

If the GPU dry run (Section 9, gate G6) shows more, a part is split before freezing; a part is never extended.

## 9. Freeze gates

All must hold on the commit being frozen. If one fails, the product or the tooling is fixed; **no goal is reworded and no row is
changed to pass a gate.**

- **G1 Asked the model.** The requirement-reader probe on the frozen commit gives no uncertain span for any v5 goal except N1
  (sc#331 to sc#336 merged). Committed as `evidence-v5/requirements-probe-<commit>.txt`. **MET on b2cae6d**
  (`evidence-v5/requirements-probe-b2cae6d.txt`: no uncertain span for any v5 or v4 goal); re-run on the frozen commit.
- **G2 Topic forms.** The product's topic check accepts every accepted form of Section 3.1 for its topic (the
  `topic-forms-probe` re-run with the D3 forms; sc#332). The re-run uses each topic as the product reads it from the goal, so
  the third topic is checked as `withdraw (or withdrew)`, not as the bare name. **MET on sc#354** (2026-10-08, 37ad5ac,
  `evidence-v5/topic-forms-probe-v5-fix-37ad5ac.txt`): 18 of 18 forms MET, and the G1 reader gives the same result for every v5
  and v4 goal. It was OPEN on b0cee16 (`evidence-v5/topic-forms-probe-v5-b0cee16.txt`: 15 of 18; `withdrawn`, `withdrawal` and
  `withdrawals` refused by the product). Product cause (a sc#332 follow-up), fixed in the product by an explicit `-n`/`-al`/`-als`
  word-form rule; no form is removed and no goal is reworded. Re-checked on the frozen commit.
- **G3 N1 reason.** On the frozen commit a stub-build run of N1 through the driver is refused before any model call with the
  reason of `N1-NR`; the exact error string (verbatim) and the file and field it lands in are recorded in `evidence-v5/` and copied into `N1-NR`; each inherited N1 row is run
  on that output and any that cannot read it is listed and replaced as Section 3.3 says (sc#336 keeps the order tested).
  **DONE on b2cae6d** (`evidence-v5/g3-n1-stub-run.txt`); re-checked on the frozen commit.
- **G4 Fallback evidence.** sc#337 merged; a stub-build daemon prints the start line; the line format is copied into Section 3.4
  and `rows-oq3-v5.jq`. **DONE**: sc#337 merged as #350; the G3 stub daemons print `STRICT strict=true dev_fallback_build=false
  require_checkpoint=true backend=ReferenceCpuBackend`; both formats are in Section 3.4 and `rows-oq3-v5.jq`.
- **G5 Self-test.** `test-v5.sh` passes, including every red case of Section 3.5, freshness of every v5 goal against every
  earlier tasks file, acceptance file, verdict and the diagnostic (D4 shares the first name Priya with v4 W4: a name, declared
  here, not a reused task), and the wrapper's refusals. `test-v5.sh` exists and passes on this draft; re-run on the frozen build.
- **G6 GPU dry run.** One dry run of the exact wrapper in dry mode on the chip, with Drake's approval and its own holds:
  measures part timings, confirms `decoding` reaches `steps/S3.json` attempts on the GB10 path, the generation record is
  written, and the OP_REPORT lines appear. Not a qualification run; its outputs are kept and never scored.
- **G7 Pre-run gate.** `cargo fmt --all --check`, clippy `-D warnings`, the stub and linked test suites reported separately,
  `test-rows-v8.sh`, `test-v3.sh`, `test-v5.sh` on the frozen build; combined build from clean checkouts at the pins.
- **G8 Review and freeze.** An independent review of this file, the tooling and the gate evidence; then every "TO FILL AT
  FREEZE" is filled, the status line changes, and that change alone merges with Drake's go-ahead. **Freeze fill
  prepared as a DRAFT (2026-10-08, status line unchanged):** the eight build pins at 30258fc (re-pinned after the extra hold, sc#360; before that at e27bda8 after the G6 fixes; first filled at 4770703), the model re-hash, the
  generated declaration (identical, QUALIFICATION_ROWS=334), the campaign-file sha256 list and the daemon `CHECKPOINT_SHARDS`
  confirmation (G6, all 22 daemon starts) are filled. On e27bda8 (product source identical at 30258fc) the 11 goals were probed again: the 10 positive goals
  resolve their destination, N1 is refused, and no goal has an uncertain requirement span (`evidence-v5/repin-probes-e27bda8.txt`). Still TO FILL AT FREEZE: the GPU hold names (who runs the parts). Any change to a campaign file
  after this fill means regenerating `evidence-v5/campaign-files.sha256`; any product change means a new build and new pins. The
  G8 change is therefore the status line plus what it touches: `frozen-v5.json` is a campaign file, so that change also
  regenerates `campaign-files.sha256`, and the draft-only check of `test-v5.sh` (status says DRAFT, NOT FROZEN) changes to FROZEN.

## 10. Prediction (stated before any run; UNVERIFIED)

Nothing here is measured. On today's product (9b5e6e8) the verdict would be FAIL with certainty: 5 of the 9 positive launches never reach
the model (Section 0), and `<id>-OPR` has no line to read. After the Section 1 fixes, from v3 and v4 judgment only: N1 and N2
about 85 percent, R1 about 75 percent, E1 about 70 percent, E2 about 55 percent (the end-of-list position), D2 about 35
percent, D4 about 40 percent, D1, D3, D5 about 35 percent each (line counts are the usual risk), D6 about 45 percent. Whole
qualification PASS: a few percent. A FAIL is the expected and acceptable outcome of an honest campaign.

## 11. What v5 does not prove

- One run, one launch per task: one observation each, not a rate. One model, one machine.
- `<id>-DEC` shows the backend's own count of how it chose each token. It is the daemon's report, not signed, and covers the
  scheduler path only (DAEMON_GENERATION_RECORD.md, "What it does not prove").
- `<id>-GR` binds the index and the three shards through the manifest digest the daemon computes; the expected value is stated
  from the listed sha256s and re-hashed at freeze (Section 7). It is the daemon's own record, not signed.
- `<id>-OPR` shows the daemon's own accounting, not an independent measurement of where each op ran.
- The topic, paragraph and sentence rules are stated narrow readings, not judgements of quality. In particular: D3's goal says
  "another form of that word" counts, but only the forms in the Section 3.1 table count (a document that uses only
  "depositor" fails the deposit topic); D5's "between 16 and 30 lines long" counts non-empty lines; R1 checks one sentence and
  the phrase `shoulder`, not that the sentence is a sensible reminder; D2's inline code in Wrap Up is not scored.
- A PASS would not close omega#327, omega#277 or sc#277, would not turn Qwen3 on GB10 on by default, and would not claim release
  readiness.

## 12. Independence and findings recorded while preparing

- **Goals.** The wording of the eleven goals, the destinations and the seeds is by an independent author (a Sonnet 5.5 worker)
  that was told not to open any repository, so no wording was shaped to the requirement reader. The brief is committed verbatim
  (`evidence-v5/task-author-brief.md`). **The requirement kinds of each task were given by the spec author** and mirror v4 on
  purpose (D4 is the shape of W4, D5 of W5, E1 and E2 of U1 and U2, N2 again asks for twelve paragraphs), so that v5 tests the
  product fixes on fresh wording of the same kinds. The spec author (session fb5693) read `requirements.rs`,
  `requirements_extract.rs` and `spine.rs` while finding the causes of the v4 refusals, and therefore did not write or edit any
  goal. Post-authoring changes: the two topic-form changes of Section 3.1, the `phrases` of D3, E1, E2 and R1 (each a substring
  of its goal), all made before any probe of their effect and before any run, and D2's `heading_order` requirement (the goal's own words "in that order", added after the draft review, also before any run). None
  changes a goal.
- **Probes.** Both probes are throwaway examples calling the public functions on an unmodified checkout. Their sources are
  committed as `evidence-v5/probes/*.rs.txt`; the requirement probe's sha256 equals the one in its evidence header. The draft
  reviewer reproduced all three outputs at 9b5e6e8. CPU only, quietlock clear.
- **Silent misses (P3)** are worse than refusals: the product would approve a v4 W1, W4 or W5 document missing the stated words.
  The campaign's rows catch it; the product fix is sc#333.
- **N1 is now refused before the model (v4 finding).** On 9b5e6e8 `task_decision` refuses an outside-the-workspace destination
  before the requirement reader and before any model call. That is the safer behaviour, but v8's `N1-B` and v4's `N1-D` need a
  model attempt, so v4's N1 (`../shared/readme-copy.txt`) would FAIL them on this commit whatever the model does. v4 is not
  edited; v5 replaces those two rows for N1 as Section 3.3 declares. The reader also marks the v5 N1 wording uncertain, which has
  no effect while the destination check comes first; `N1-NR` fails if that order ever changes.
- **The model's own sampling suggestion** (`generation_config.json`: sampled, temperature 0.7) differs from the campaign's
  greedy decoding; the product does not read those fields, and `<id>-DEC` checks the result directly.
- **Machine fields of the task file.** After the product fixes merged, the fields the inherited row modules read were aligned
  with the v4 task file: `min_lines`, `min_code_blocks`, `tail_heading` (null) and `min_tail_words` (0) on the six documents, D4's
  `source` as path plus sha256, and N1's kind `negative-boundary` (the v8 receipt names that kind). No goal, destination, seed,
  phrase or requirement changed (`test-v5.sh` checks the digest of goals, destinations, seeds and phrases). The v5
  requirements state counts as `n` and required words as a list `words`; `rows-oq3-v5.jq` reads both explicitly and fails closed
  when either is missing.
- v4 is not modified by this change; no product code is modified by this change.
