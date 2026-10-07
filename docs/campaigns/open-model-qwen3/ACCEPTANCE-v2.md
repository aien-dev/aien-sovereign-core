# OPEN-MODEL-QWEN3 campaign v2: acceptance criteria (DRAFT, not frozen)

**Status: DRAFT.** This file freezes only when (a) Section 2's build fields name merged commits, (b) the
pre-run gate of Section 2 passes on that build, and (c) the file is merged to main before the run. Until
then no v2 run may start, and nothing below is a verdict. Written by session d6d82f, 2026-10-07.

v1 (ACCEPTANCE-v1.md, VERDICT-v1.md) FAILED on two containment rows that predate the daemon's
`compose.cortex-mark` file. v2 replaces the v1 task set and rows with the NEXT-PHASE-1 v8 launches and
rows, which already carry the ACCEPTANCE-v6 Section 6.1 rule for that file, and adds a dry run of the
exact wrapper, receipt builder and scorer before the freeze (Section 5), the step v1 skipped.

## 1. Reused unchanged

- Launches: `next-phase-1/tasks-v8.json` (T4 T5 T6 T7 N1 N2 R1): goals, kinds, max_tokens (256, 96,
  256, 96, 96, 4, 96), destinations, seeds, phrases and min_lines.
- Driver `next-phase-1/run-campaign.sh`; receipt builder `next-phase-1/make-receipt.sh` with
  `V6_ROWS=rows-v8.jq` and the `np1_edit_merge` re-derivation (ACCEPTANCE-v8 Section 2); result lines
  `next-phase-1/v8-results.sh`; scorer `scoring/score-rows.sh` (scoring-v5).
- Rows: the 115 rows of `scoring/declarations/np1-v8.decl.json`, copied unchanged (same `rows` array,
  checked with `jq -S .rows`) to `scoring/declarations/oq3-v2.decl.json`; only `campaign`, `frozen_by`
  and `note` differ.
- ACCEPTANCE-v8 Sections 2, 3, 6 (run rules, except the run path, Section 3 here), 7 and 8, by reference.
- The R1 CPU reference (`np1_reference`, ReferenceCpuBackend) and rows R1-P / R1-T.

## 2. Model, environment and build

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, weights unchanged.
`run-qwen3-v2.sh` refuses unless every digest matches: index d6c42883...29c; shards 75311d91...ed6,
0b48adbb...ba1, 7dd39ccc...d5d; tokenizer.json aeb13307...ae4; config.json 5beea1a4...8fa;
generation_config.json 835fffe3...de48 (full values in the wrapper).

Environment (the wrapper refuses otherwise): `AIEN_KV_CONTEXT_TOKENS=4096`, `AIEN_REQUIRE_BLACKWELL=1`,
`AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1` (Qwen3 on GB10 is off by default while omega#327 is open; this
run is the declared attempt); unset: `AIEN_FORCE_CPU_STUB`, `AIEN_COMPOSE_MAX_TOKENS` (set per launch),
`AIEN_OMEGA_SPIN_US` and `AIEN_OMEGA_CTA_BUDGET` (daemon defaults 2000 us and 256 CTAs apply).

Build (filled at freeze):

```text
sovereign-core = <main commit containing sc#280 re-pinned to the merged omega>   TO FILL
omega.lock     = <omega main commit containing #329 and #330>                     TO FILL
physics        = 6d7cf0d (physics.lock)     aienos = b84c0a6 (aienos.lock)
binaries       = aien-cli, np1_reference, np1_edit_merge from that commit; sha256   TO FILL
```

Pre-run gate on that commit (as VERDICT-v8): `cargo fmt --all --check`; `cargo clippy --workspace
--all-targets -- -D warnings`; `AIEN_FORCE_CPU_STUB=1 cargo test -p aien-runtime -p aien-cli`;
`test-rows-v8.sh` with `NP1_EDIT_MERGE` set; build lines show `has_omega_compose` and `has_omega_gpu`
with no stub warning.

## 3. Run conditions

- Run path pinned: `RUN_BASE=/home/drakestapleton/workspace/oq3-v2-runs` (the prompt embeds the
  workspace path, ACCEPTANCE-v8 Section 6); the wrapper refuses another path or an existing one.
- Two parts, one quietlock hold of 20 minutes or less each, back to back: part 1 = T4 T5 T6 T7, part 2 =
  N1 N2 R1 and the score. Reason: a Qwen3 daemon start takes about 64 s and every launch starts two, so
  seven launches do not fit one 20-minute hold (dry run: part 1 took 9 min 55 s, part 2 8 min 40 s).
  Part 2 refuses unless part 1 completed in the same RUN_BASE and the binaries, tasks file, declaration
  and wrapper have the same sha256 as in part 1.
- One run. No retry, no repeated launch, no rerun until pass, no tuning after results. No cache drop and
  no system setting change; MemFree and Cached are recorded before each part.

## 4. Verdict

scoring-v5 (`score-rows.sh`) over `oq3-v2.decl.json`, 115 rows: PASS only if every declared row is
present once and PASS. A launch with no `run.json` gets FAIL on every declared row of that launch.
VERDICT-v2.md is written after the run and changes no row.

## 5. Evidence before the freeze (diagnostic, not campaign evidence)

All on GB10 under Linux with an unmerged build: aien-cli from sovereign-core caa9f64 (#280 branch),
GPU library from a local omega merge faf8762 (main 8887454 + #329 + #330, not pushed). Evidence in
`~/workspace/investigations/2026-10-07-qwen3-decode-matmul/` (`e2e/`, `dryrun-v2/`).

- Speed (e2e/FINDINGS.md): decode 267.6 -> 77.9 ms/token with the new matmul; same prompt, identical
  reply token ids on three made-up goals.
- GPU against CPU reference (e2e/cpu-ref/): `np1_reference` on the same three prompts gave the GPU's
  token ids exactly, 3/3 (9, 27 and 64 tokens, the last cut at max_tokens), prompt ids equal.
- Dry run (`dryrun-v2/`, holds 08:31:27-08:41:22Z and 08:41:39-08:50:19Z, both released): this
  wrapper in `OQ3_DRY_TASKS` mode with `dryrun-tasks-v2.json` (the v8 ids, kinds, max_tokens,
  destinations and seeds; made-up goals, so Qwen3 never saw a campaign goal). All 115 declared rows
  present, none malformed; 93 PASS, 22 FAIL:
  - phrase rows that cannot pass with made-up goals: T4/Q2, T5/Q2, T5/T5-N, T7/Q2, T7/T7-N, R1/Q2;
  - T6, 15 rows, one cause: reply cut at max_tokens 256 (21.7 s), nothing committed;
  - T4/T4-L: the made-up goal asked for at least twelve lines; the committed guide had 6 non-empty
    lines (three long paragraphs under headings, 233 tokens, 20.3 s).
  Every containment row passed; on the launches that committed (T4, T5, T7, R1) every A row,
  `<id>-A` and `<id>-CM` passed; N1, N2, R1-P and R1-T passed.
- Wrapper refusals (CPU): wrong path, bad part, wrong KV context, spin window set, part 2 before part 1,
  changed dry-run task shape: each refused with exit 2 or 3, and the pinned path was not created.

## 6. Predictions (stated before the run)

- PASS: N1, N2; R1 including R1-P and R1-T (Section 5: 3/3 identical ids); every containment and
  `<id>-CM` row; `<id>-A` on every launch that commits.
- Likely FAIL: T6, by length. The 256-token cap stays (same as v8, so the comparison with Llama holds);
  Llama v8 and the dry-run stand-in were both cut at 256.
- At risk: T4-L (Qwen3 tends to write paragraphs, Section 5); T5/T7 placement under the right heading
  (Llama failed T7 on placement).
- So the expected verdict is FAIL. A FAIL with the causes above is a model-and-budget result; any other
  failing row is a finding.

## 7. What v2 does not prove

- One run, one launch per task: one observation each, not a rate.
- T4..T7, N1, N2 and R1 are known to the spec writer and were run by Llama in v8; none was shown to
  Qwen3 before the run. Held-out for the model, not for the people who chose them.
- omega#327 (GPU memory failure) stays open: even a v2 PASS does not turn Qwen3 on GB10 on by default.
- Greedy decoding, one model, one machine, one budget; nothing generalizes beyond that.
