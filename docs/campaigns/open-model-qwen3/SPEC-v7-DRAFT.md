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

1. [ ] Chat template gap closed on main: Qwen3-8B template pinned with fixture, provenance and render oracle; thinking off; new tests pass.
2. [ ] Download hashes recorded in `frozen-v7.json` (done in this PR: OBSERVED) and re-hashed on the chip host within the freeze day.
3. [ ] Engine smoke test PASS (EVAL Section 8, steps 2 to 5: template render, golden ids on CPU reference, GB10 equals CPU reference token for token;
   the untied output head is exercised). Recorded under `evidence-v7/`.
4. [ ] `model_sha256` filled from the daemon's `CHECKPOINT_SHARDS` line in a smoke run.
5. [ ] Clean build at the freeze commit; every pin in `frozen-v7.json` filled; checkouts clean.
6. [ ] `test-v7.sh` passes at the freeze commit; the v5 campaign file hashes still match (`evidence-v5/campaign-files.sha256`) for every file v7 reuses.
7. [ ] G6 dry run of part 1 on GB10 in its own hold, then `g6-gate.sh` exit 0 over D1 D2 D3 (all product rows, ALLEN engaged). The dry
   run must also show the containment and identity rows PASS with the ALLEN state directories present beside the compose home (they are new
   since v6 and unproven against the unchanged row files; the proof run in the PR shows the directories exist, whether any row counts them
   is decided by the dry run, not assumed).
8. [ ] Coordinator go for the chip window, quiet flag checked with `quietlock check`, `frozen-v7.json` status changed to FROZEN in the freeze PR, merged before any launch.

## 6. What v7 does not prove

A v7 PASS would qualify Qwen3-8B on these eleven launches, greedy decoding, thinking off, one run. It would not show the model is better than the 4B
outside them, it would not promote it (promotion is a separate Drake decision, CAND-5 manifest in aien-architecture), and the ALLEN subject is a
host-built fixture, not a provisioned identity.
