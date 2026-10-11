# OPEN-MODEL-QWEN3 v7: qualification specification for Qwen3-8B

**Status: DRAFT NOT FROZEN. NOT RUN. This file freezes nothing; `frozen-v7.json` says DRAFT NOT FROZEN and every pin is a placeholder, so
`run-qwen3-v7.sh` refuses a real run.** Owning issue: aien-dev/aien-sovereign-core#384 (Drake ruling 2026-10-11, aien-architecture#190
comment 6109731585 and sc#384 comment 6109731974: Qwen3-4B-Instruct-2507 retired after the frozen FAILs v5 and v6; next candidate to evaluate,
not promote, is Qwen/Qwen3-8B). Evidence for the model facts: `EVAL-qwen3-8b.md`. Written by lane D1, 2026-10-11.

## 0. What v7 is

The v6 specification (`QUALIFICATION-v6.md`) with the model line replaced and two harness corrections. Nothing else changes.

```text
model        = Qwen/Qwen3-8B, Hugging Face revision b968826d9c46dd6066d109eabc6255188de91218, Apache-2.0, thinking OFF (EVAL Section 4)
rows         = the 334 declared rows of oq3-v5.decl.json, unchanged (D1 34, D2 36, D3 36, D4 35, D5 36, D6 33, E1 33, E2 34, R1 32, N1 11, N2 14)
thresholds   = unchanged: PASS only if every declared row is present once and PASS (scoring-v5, score-rows.sh); one run, no rerun, no retry,
               no task edit; tasks, caps, budgets (including D6 1536 tokens / 170 s) and the seeds are the v5 files themselves
verdict rules= ACCEPTANCE-v5 Section 5, unchanged
```

No acceptance threshold, row, task, token cap or time budget is changed in response to a result. Budgets are not enlarged for the 8B even though
its decode is expected to be slower (EVAL Section 6): a slower model is a model result.

## 1. Harness correction (a): the driver engages ALLEN

Defect (OBSERVED, RESULT-v6.md Section 3): since sc#386 compose memory needs an engaged ALLEN identity. The frozen driver
`../next-phase-1/run-campaign.sh` starts the daemon with no `AIEN_ALLEN_SUBJECT`, so step S1 ("remember") was refused with
"ALLEN is not engaged on this machine" on every v6 launch, which failed 17 rows (recall, completion, N1-RH, N2-H).

Correction: `run-campaign-allen.sh` is a copy of that driver with one added block between S0 and the measured window (before `.mark`, so the eight
timed steps are untouched). The block follows the WHOLE-SYSTEM-E2E harness (`scripts/whole_system_e2e.sh` `start_daemon`, `build_subject`) and
`scripts/allen_e2e_demo.sh` S1: read the digest of Cortex record 1 of the new compose home, stop the first daemon, build a host-built fixture
subject bound to that digest with the test-only encoder (`build-allen-subject.sh`, which runs `crates/aien-allen/tests/e2e_demo_subject.rs`), start
the daemon again with `AIEN_ALLEN_SUBJECT` and a one-time `AIEN_ALLEN_ADOPT`, and refuse to continue unless the daemon logged `ALLEN: engaged` and
`allen status` returned a fingerprint. The restart in S7 only names the subject. The subject is a fixture, not an AIENOS-provisioned identity;
`run.json` records `allen.subject_kind` saying so. The frozen original driver stays untouched, so v1 to v6 remain reproducible; v7 binds the new
driver, the builder and the gate in its identity hash (`identity-part<k>.sha256`).

Second defect found by the proof run (EVAL Section 7): S1 now writes `compose.allen-memory/` beside the compose home, which the unchanged containment rows
(`outside_new_files == ["./compose.cortex-mark"]`) would read as a stray file. The new driver lists those files in `run.json` `.allen.state_files`
and leaves them out of `outside_new_files`, as it already does for `./compose`, `./state` and `./prov`. No row text changes. Reviewers confirm this scope.

## 2. Harness correction (b): gate G6 checks every product row

Defect (OBSERVED): the v6 G6 rule and launcher (`evidence-v6/run.sh`) checked only `PRE`, `OPR`, `BE`, `PIN` of D1 to D3. The dry run did show the
S1 refusal (dry D1 `v1 Correctness: recall` FAIL) and the gate let it through.

Correction: `g6-gate.sh DRY_RESULTS DECLARATION DRY_RUN_BASE D1 D2 D3` (rules in its header). For each dry launch it requires: `run.json` exists;
steps S1, S2, S7, S8 ok; `ALLEN: engaged` logged; every product row PASS (`PRE`, `DEC`, `OPR`, `BE`, `PIN`, `MEAS`, `v1 Identity`, rescues,
containment: speculation, `v1 Memory`, `v1 Correctness: recall`); and if the launch committed, every other declared row PASS except the four
model-behaviour rows `RQ<k>`, `CUT`, `F`, `SB`. A missing row is a failure. `test-v7.sh` proves the gate goes red on the v6 failure
(S1 refused, no ALLEN line, recall FAIL) and green on a clean synthetic launch (16 of 16 checks).

Correction to the correction (dry attempt 2, 2026-10-11T22:26:12Z to 22:40:00Z, hold rc 0, G6 FAIL; records in `evidence-v7/attempt-2-g6-fail/`):
the first gate listed `v1 Memory`, `DEC` and `MEAS` as rows independent of the model. They are not. A refused reply writes no memory
record, so `v1 Memory` FAILs on every launch that did not commit (RESULT-v6 already classed it (a); attempt 2 D1 and D3 showed it, the
committed D2 passed it); it now belongs to the committed group. An attempt the product ends by timeout has no decoding and no token ids,
so `DEC` and `MEAS` FAIL by the frozen v5 rules whenever the model is too slow for the task budget (attempt 2 D3: reply 1 refused after
63 s, reply 2 cut at the 57 s left of the 120 s budget, 0 tokens kept); the gate now requires them unless `s3-report.json` shows a
timeout attempt, prints a NOTE naming it, and counts it as a campaign finding, not a harness fault. `g6-reasons-corrected-gate.txt`
replays the corrected gate over the attempt 2 records: the remaining failures are `OPR` (Section 2.1) and `PIN` plus `GR` on the
committed D2 (pre-freeze by construction, Section 7). `test-v7.sh` now proves both corrections red and green (20 of 20 checks).

### 2.1 Product-side finding from attempt 2: the OP_REPORT line order (fixed in sc#454)

The frozen v5 `-OPR` row parses the daemon line as `OP_REPORT native=[..] reference=[..] native_fallbacks=[..] reference_runs=[..]
backend=<rest>`. sc#405 (chip call count, after v6) put ` gpu_executions=<n>` before ` backend=`, so every attempt 2 launch failed
`-OPR` although the logs show native ops only, zero fallbacks and zero STRICT violations (`op-report-lines.txt`). sc#454 moves the
count after `backend=` and adds the layout contract to `op_report_line` with a test. The v5 rows stay unchanged (Drake: thresholds
never move); the freeze build must include sc#454.

## 3. Model, engine and build

- Weights downloaded and hashed: `frozen-v7.json` `model` block (index, five shards, tokenizer, three config files). The five shards and
  `tokenizer.json` equal Hugging Face's own sha256 for that revision (EVAL Section 1).
- Engine gap that blocks the daemon: the Qwen3-8B chat template is refused by `tokenizer.rs` (EVAL Sections 4 and 5). It needs a pinned second Qwen
  layout with thinking off. That is product code and lands in its own PR before this spec can freeze. Until then no v7 run is possible.
- Environment, path and run plan: as v6 Sections 4 and 6 (GB10 with the R1 CPU reference leg, `AIEN_KV_CONTEXT_TOKENS=4096`,
  `AIEN_REQUIRE_BLACKWELL=1`, `AIEN_REQUIRE_CHECKPOINT=1`, `AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1`, four parts each in its own hold after one dry part 1).
  Expect more chip time than the 30 min 10 s of v6: decode is predicted to be about half as fast (EVAL Section 6, UNVERIFIED).
- Build: clean clone at the freeze commit, pinned dependency checkouts, as `evidence-v6/build.sh`.

## 4. Predictions (stated before any run; UNVERIFIED)

- The ALLEN correction removes the 17 v6 rows of class (b) from the failure list (the proof run in the PR shows S1 accepted; confidence 90 percent).
- Model-output rows (D1, D2, D6 style rows: line count, heading shape) may improve with the 8B but nothing here predicts a PASS. No claim is made about the verdict.
- Budget rows (class (d)) are likely to grow because the 8B is slower under unchanged budgets (confidence 60 percent).

## 5. Freeze checklist (all must be true; each is a yes or no with evidence)

1. [x] (2026-10-11, sc#436 merged 7cfc7a8; render oracle re-verified independently the same day, EVAL Section 8) Chat template gap closed on main: Qwen3-8B template pinned with fixture, provenance and render oracle; thinking off; new tests pass.
2. [x] (re-hashed on the chip host 2026-10-11, `evidence-v7/model-rehash-2026-10-11.txt`, 10 of 10 equal) Download hashes recorded in `frozen-v7.json` (OBSERVED) and re-hashed on the chip host within the freeze day.
3. [x] (2026-10-11T16:02:55Z, SMOKE-8B-v1 attempt 2 on main c26f4d7, evidence under `evidence-smoke-8b/attempt-2/`, sc#440) Engine smoke test PASS (EVAL Section 8, steps 2 to 5: template render, golden ids on CPU reference, GB10 equals CPU reference token for token;
   the untied output head is exercised). Recorded under `evidence-v7/`.
4. [x] (17bc36882b981cdaa253076c43273e3b5e54251db26cb6a5f51de396c456f5a6, index+shards, from the daemon line of dry attempt 2, `evidence-v7/attempt-2-g6-fail/model_sha256-line.txt`, the same on every daemon start of D1 to D3) `model_sha256` filled from the daemon's `CHECKPOINT_SHARDS` line in a smoke run.
5. [x] (clean build at main a6e5f85, the last product change (sc#454), `evidence-v7/build-summary-a6e5f85.txt`, all checkouts dirty=0; the freeze commit pins it, as v6 pinned de9c626 from wrapper f8d8c72) Clean build at the freeze commit; every pin in `frozen-v7.json` filled; checkouts clean.
6. [x] (at the freeze candidate a6e5f85 + this file: `evidence-v7/test-v7-a6e5f85.txt` 20 ok 0 failed; `evidence-v7/campaign-files-check-a6e5f85.txt` 63 OK; pre-check on c26f4d7: 16 ok, 63 OK) `test-v7.sh` passes at the freeze commit; the v5 campaign file hashes still match (`evidence-v5/campaign-files.sha256`) for every file v7 reuses.
7. [ ] G6 dry run of part 1 on GB10 in its own hold AFTER the freeze commit (as v6: the -PIN and -GR rows need the frozen pins and model
   hash; attempt 2 ran on the draft and failed them by construction), then `g6-gate.sh` exit 0 over D1 D2 D3 (all product rows, ALLEN
   engaged; attempts 1 and 2 recorded under `evidence-v7/attempt-1-hold-refused/` and `evidence-v7/attempt-2-g6-fail/`). The dry
   run must also show the containment and identity rows PASS with the ALLEN state directories present beside the compose home (they are new
   since v6 and unproven against the unchanged row files; the proof run in the PR shows the directories exist, whether any row counts them
   is decided by the dry run, not assumed).
8. [x] (Drake 2026-10-11: "wait for the other agent ... then you can follow that with the hold"; holds are 20 minutes each since sc#453, the dry part 1 one hold, the real run four more) Coordinator go for the chip window (Drake: a quiet hold is his approval), quiet flag checked with `quietlock check`, `frozen-v7.json` status changed to FROZEN in the freeze PR, merged before any launch.

## 6. What v7 does not prove

A v7 PASS would qualify Qwen3-8B on these eleven launches, greedy decoding, thinking off, one run. It would not show the model is better than the 4B
outside them, it would not promote it (promotion is a separate Drake decision, CAND-5 manifest in aien-architecture), and the ALLEN subject is a
host-built fixture, not a provisioned identity.

## 7. Launchers (added 2026-10-11, `evidence-v7/`)

`build.sh <sc commit>` (clean clone + pinned release build, CPU only), `run-part.sh` (one part per hold; Qwen3-8B folder, v7 wrapper),
`run-dry.sh <freeze commit> <build folder>` (gate G6: refuses unless `frozen-v7.json` is FROZEN at that commit; dry part 1 in one hold,
then `g6-gate.sh` over D1 D2 D3, writes `records/g6.txt`
and the daemon's `CHECKPOINT_SHARDS` line for checklist item 4), `run.sh <freeze commit> <build folder>` (four real parts, refuses
without `G6 PASS` and without `frozen-v7.json` FROZEN). Each is the v6 launcher with the v7 names; the diff against `evidence-v6/`
is the review. Holds are 20 minutes: quietlock refuses longer holds without Drake's approval token (attempt 1, 2026-10-11T22:10:23Z, rc 77, refused with 40; recorded in `evidence-v7/attempt-1-hold-refused/`) and on overrun keeps the flag until the command exits without killing it, so a part that runs past 20 minutes (the 8B is predicted to take about twice the v6 7 to 8 minutes) completes under the kept flag.

## 8. Speed observed in dry attempt 2 (Qwen3-8B on GB10, f32, greedy; not a verdict)

| launch | attempt 1 | attempt 2 | product outcome |
|---|---|---|---|
| D1 | 655 tokens in 93.2 s, refused (14 of 22 lines) | none: 26.8 s left < 30 s per-attempt minimum | not committed |
| D2 | 354 tokens in 53.0 s, refused (section missing) | parsed in 56.2 s | committed |
| D3 | 425 tokens in 63.2 s, refused (13 of 18 lines) | cut by the product at 56.9 s (budget left), 0 tokens kept | not committed |

About 7 tokens a second against the 4B's about 10 (v6 D1 to D3: 345 to 524 tokens in 33 to 53 s). The task budgets (120 s, ACCEPTANCE-v5
Section 2) are frozen product limits and stay; a refused first reply leaves the 8B little or no room for a retry, so the real campaign is
expected to record more timeouts and uncommitted launches than v6 did. That is what the campaign measures; it is not a reason to change it.
