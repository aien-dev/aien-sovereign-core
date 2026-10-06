# CAND-4 qualification: acceptance criteria (declared before any qualification run)

```text
campaign_id   = "cand4-qualification"
spec_version  = 1
candidate     = CAND-4 = sovereign-core main after the omega pin bump (sc#232), with the release
                gate (#230) and scoring contract v5 (#231)
status        = DRAFT until the block in section 1 holds no UNFROZEN value. The freeze is the
                commit that fills that block; it changes this file only and is made before any
                qualification run. Nothing in this file changes after the freeze; a change is a
                new spec_version.
scoring       = docs/campaigns/scoring/SCORING-v5.md (score-rows.sh), declarations q1.decl.json
                and q2.decl.json in this directory, committed before the freeze
scope         = Linux-hosted, single machine (the Spark, GB10), the sovereign-core daemon path:
                one workflow (propose, authorize, execute, explain, restart, recall) and its
                recovery. Release internal-only (owner decision).
```

Every claim below names its source (file:line at the commit stated, or a command). Anything
else is marked UNVERIFIED. Line numbers were read at sovereign-core `296c4ac` (main before the
pin bump); `run-cand4.sh check` re-reads the constants of section 2 at the frozen commit and
refuses to run if they differ.

## 1. Frozen inputs (filled once, at the freeze)

`run-cand4.sh` reads this block. It refuses to start a qualification run while any value is
UNFROZEN, and before every launch it recomputes every sha256 below and refuses on a mismatch.

```text
>>> CAND-4 frozen inputs >>>
kind                    = QUALIFICATION
candidate_id            = CAND-4
sc_commit               = UNFROZEN
omega_commit            = UNFROZEN
aienos_commit           = UNFROZEN
physics_commit          = UNFROZEN
build_evidence          = UNFROZEN
aien_cli_path           = UNFROZEN
aien_cli_sha256         = UNFROZEN
cpu_fault_path          = UNFROZEN
cpu_fault_sha256        = UNFROZEN
librx_compose_sha256    = UNFROZEN
libomega_gpu_sha256     = UNFROZEN
omega_build_dir         = UNFROZEN
rx_r13_host_sha256      = UNFROZEN
rx_r13_silicon_sha256   = UNFROZEN
rx_r13_testbuild_sha256 = UNFROZEN
omega_src_dir           = UNFROZEN
aienos_repo             = UNFROZEN
physics_dir             = UNFROZEN
model_dir               = /home/drakestapleton/.cache/huggingface/hub/models--unsloth--Llama-3.2-1B-Instruct/snapshots/5a8abab4a5d6f164389b1079fb721cfab8d7126c
model_safetensors       = 1ff795ff6a07e6a68085d206fb84417da2f083f68391c2843cd2b8ac6df8538f
model_config            = dfb67fd8afe73a1c75245824ef9d64a6ba8983025447e3bf76aa1ea57ee46152
model_generation_config = 6d4f979915331212d7672c68b22a4ddad9e21ed8126cf2bd1ea6b2b88f595c1c
model_tokenizer         = 6b9e4e7fb171f92fd137b777cc2714bf87d11576700a1dcd7a399e7bbe39537b
model_tokenizer_config  = 9ddd255c19fe319c8d4e891163540382e9fbda99f394674f2a929efc47d57458
model_special_tokens    = 94e708c3f5e64acf85bbe5ad01467a1248faadb73e83b41793087ecced586e8f
model_chat_template     = 5816fce10444e03c2e9ee1ef8a4a1ea61ae7e69e438613f3b17b69d0426223a4
max_tokens              = 96
attempt_budget_ms       = 12000
skill_budget_ms         = 29000
max_attempts            = 3
fix_old_dir             = /home/drakestapleton/.claude/jobs/9bfe8553/tmp/np2-fix
fix_old_files_sha256    = 3e4e26eff45e0f7282c335cb2028cd8c314e11b2669c73c16ab27c530c89eaf8
<<< end of CAND-4 frozen inputs <<<
```

- `aien_cli` is the production binary of the CAND-4 clean double build (evidence
  `build_evidence`, both builds SAME). It runs every GPU leg (Q1, the Q2 fixture F0).
- `cpu_fault` is the NEXT-PHASE-2 test binary rebuilt from `sc_commit` by `build-cpu-fault.sh`
  (ACCEPTANCE-v2 section 4 recipe: composition archive linked, no GPU archive, feature
  `fault-hold`, built with `scripts/repro-build.sh`). It is not a release binary and contains
  the test hooks by design; it runs only the Q2 cases. Its digest is declared here before any
  run.
- The model snapshot files: digests of model, config, generation config and tokenizer equal
  NEXT-PHASE-1 ACCEPTANCE-v5 section 5. The code also reads `tokenizer_config.json` or
  `chat_template.jinja` for the chat template (`crates/aien-inference-abi/src/tokenizer.rs:188-204`),
  so every file of the snapshot is pinned.

## 2. Model, template and budgets (identical to NEXT-PHASE-1 v5; no tuning)

- Model: unsloth/Llama-3.2-1B-Instruct snapshot 5a8abab, the tested model (owner decision:
  Llama stays for CAND-4, release internal-only). Decoding greedy, stop on end of sequence.
- Prompt: `proposal_prompt` and the assistant prefix `filename:` (no trailing space) of the
  frozen commit (`crates/aien-runtime/src/spine.rs:727-738` at `296c4ac`), the Llama 3 chat
  template of the snapshot, the constraint text and workspace seed of
  `next-phase-1/run-campaign.sh` (unchanged).
- `max_tokens = 96`, set by the environment `AIEN_COMPOSE_MAX_TOKENS=96`. The binary's default
  without it is 48 (`crates/aien-runtime/src/server.rs:433-436`). CAND-4 is qualified only with
  the variable set; a run without it is a configuration this campaign does not cover.
- **Attempt budget: the code value is the frozen one.** ACCEPTANCE-v5 section 5 states
  "attempt budget A = 21 000 ms". The code enforces `COMPOSE_ATTEMPT_BUDGET = 12_000 ms`
  (`spine.rs:717`) and `COMPOSE_SKILL_BUDGET = 29 s` (`spine.rs:712`). What the binary does
  (`propose_with_retries`, `spine.rs:753-779`): attempt 1 always starts and gets what is left of
  29 000 ms as its limit; attempt k > 1 starts only if at least 12 000 ms is left. The v5 spec
  value 21 000 ms is not in the code. Discrepancy recorded; frozen here: 12 000 ms per-attempt
  start threshold, 29 000 ms skill budget, 3 attempts (`spine.rs:709`). In the v5 run every
  task finished on attempt 1 in under 5 s (VERDICT-v5 section 1), so the difference did not
  act there. No value is tuned.
- **Retry policy.** The harness never retries: each declared launch runs once; a failed launch
  is that row's FAIL and is kept. The binary's own automatic retry (up to 3 proposal attempts
  inside one Skill run, above) is part of the frozen code, is not a rescue (ACCEPTANCE-v5
  section 5), and every attempt is recorded with its `finish_reason`; row Q3 requires the
  accepted attempt to stop on `eos`.
- Environment of every launch: `env -i` with only `PATH`, `HOME`, `USER`, `LANG` and the
  declared variables (`AIEN_BIN`, `AIEN_MODEL_PATH`, `AIEN_TOKENIZER_PATH`,
  `AIEN_COMPOSE_MAX_TOKENS=96`, and for GPU legs `AIEN_REQUIRE_BLACKWELL=1`). In particular
  `AIEN_DEV_FALLBACK` (a runtime opt-in to fallbacks that a production binary honours,
  `crates/aien-inference-abi/src/strict.rs:15-33`) is never set.

## 3. Production hygiene (H), run first, no GPU

`hygiene.sh` (this directory), its JSON result kept as a receipt named by its sha256.

| row | check | PASS iff |
|-----|-------|----------|
| H1 | production aien-cli carries no fault hook | `strings -a aien_cli` has 0 lines matching `AIEN_FAULT_HOLD`, `fault hold`, `reconcile_panic`, `reconcile_error`, `forced start-up reconcile` |
| H1c | positive control for H1 | the same patterns are found in `cpu_fault` (the check can see them) |
| H2 | production build had no test feature | the `-vv` release log of the build (`build_evidence/sc-rel-2.log`) has no `feature="fault-hold"` and no `feature="dev-fallback"` on any rustc line |
| H3 | production aien-cli carries no omega test-build piece | no `AIEN_TEST_BUILD piece:` string in aien_cli; `nm` of the linked `librx_compose.a` has no `aien_test_build_` symbol |
| H4 | omega production programs | `tools/r16_prod_hygiene.sh host <rx_r13_living_host>` PASS (link map, strings, ARGUS linked, host ARGUS probe); `R16_PROD_NO_RUN=1 tools/r16_prod_hygiene.sh silicon <rx_r13_living_silicon>` PASS (static only: no GPU) |
| H4c | negative control for H4 | `R16_PROD_NO_RUN=1 r16_prod_hygiene.sh silicon <rx_r13_living_testbuild_silicon>` FAILS (the test build is detected) |
| H5 | omega refuses test pieces at build time | `make test-prod-refuses-test-pieces` at `omega_commit`, in a scratch copy with its own OUT_DIR: exit 0 |
| H6 | report only | every `AIEN_*` name in aien_cli containing TEST, STUB, FAULT, FALLBACK or DEV, listed, not scored (`AIEN_DEV_FALLBACK` is expected: section 2) |

## 4. Q1 useful workflow (GPU)

- Tasks: T1, T2, T3 of `next-phase-1/tasks-v5.json` (ACCEPTANCE-v5 section 2), unchanged.
- **Repetitions: 3 per task, 9 launches**, in the order round 1 (T1, T2, T3), round 2, round 3.
  One driver launch per task (`next-phase-1/run-campaign.sh`, called unchanged with its own run
  root), two daemon starts per launch, zero hand input. Each round runs inside one
  `quietlock hold --owner laneQ --minutes 20` with start and release whispers, only when
  `~/workspace/.spark-quiet` is absent; another lane's hold is waited for, never interrupted.
- Scoring per launch: `next-phase-1/make-receipt.sh` with `TASK_ID`, `RESCUES=0`, unchanged:
  every v1..v4 row (S1..S8; content digests equal; recall byte-identical; approvals exactly 1;
  rescues 0; identity; memory; containment workspace; containment speculation) and every v5 row:
  Q1 path equals the requested path and the change set is exactly that path; Q2 required phrases
  present, no echoed prompt line; Q3 stop reason `eos`; Q4 no chat markers; A1 containment incl.
  speculative effects; A2 approval bound to the exact effect (content, disk and proposal
  digests, path); A3 explanation cites receipt fields that exist, names the committed path;
  A4 restart keeps identity and recall returns the committed record (World commit, canonical
  Cortex persistence); A5 exactly one expected approval, counted on its own; A6 manual
  rescues 0. A launch is PASS only if its receipt verdict is PASS.
- A launch whose driver does not produce `run.json` is FAIL, and the harness still writes an
  attempt record for it.
- Q1 results: one line per launch `{row: T<k>, rep, verdict, outcome, env: "GPU"}`, scored by
  `score-rows.sh q1.decl.json` (3 rows x 3 repetitions). Q1 PASS iff that verdict is PASS.

## 5. Q2 recovery (NEXT-PHASE-2 v4 case set, against CAND-4 binaries)

- Fixture F0 (GPU): `next-phase-2/run-faults.sh fixture` on `aien_cli` with
  `AIEN_REQUIRE_BLACKWELL=1`, model as section 2, inside one quietlock hold (<= 20 min, whispers).
  Valid only if S3 committed. No fallback fixture: if F0 is not valid, Q2 is NOT_RUN.
- Cases: `run-faults.sh cases` on `cpu_fault`, all cases, `REPS=3` (C6i once, controls once,
  as the script runs them), `FIX_OLD = fix_old_dir` for C8a (its files sha256 re-checked by the
  script). The cases never touch the GPU (the script stops if a daemon is not CPU-reference).
- Scoring: `score-rows.sh q2.decl.json results.jsonl external.json`, where `q2.decl.json` is
  `scoring/declarations/np2-v4.decl.json` (same rows, reps, controls, C6d `not_applicable`)
  plus the receipt-level row `C3-control` (F0's daemon names `OmegaGb10Backend`) and an `env`
  label per row; `external.json` = `{"F0": fixture.s3_committed}`. The NEXT-PHASE-2
  `make-receipt.sh` v4-format receipt is also written (builds, link proof, not-proved list); it
  is evidence, the scoring-v5 verdict decides.
- Environment label of every row (in `q2.decl.json`):
  - **GPU**: F0 and `C3-control` (production `aien_cli` on the GB10).
  - **CPU fault injection**: C1a, C1b, C2a, C2b, C2c, C2d, C7a, C7c (a `fault-hold` hook is
    armed: `AIEN_FAULT_HOLD` in the executor or the daemon).
  - **host**: control-C1..C8, C3a, C3b, C4, C5a, C5b, C5c, C6a..C6i, C6c-ctl, C7b, C8a, C8b
    (`cpu_fault` binary with no hook armed; the harness acts from outside the process: SIGKILL,
    file damage, CLI commands).
- Q2 PASS iff the scoring-v5 verdict is PASS (C6d may be NOT_APPLICABLE, named, never counted
  as PASS).

## 6. Coverage of six failure cases (declared before any run; never PASS by implication)

| # | case | status | reason / row |
|---|------|--------|--------------|
| 1 | actual GPU loss during execution | OUT OF SCOPE, NOT COVERED | No row removes the device mid-run. C3a covers only "engine absent at start" (refusal), not loss during a run. The effect path (S4 authorize, S5 execute) runs in the CLI process and does not use the GPU (ACCEPTANCE-v2 A1, `crates/aien-cli/src/compose.rs`), so a loss can only end S3 (the proposal); the production response is a fatal `STRICT_REAL_MODEL_VIOLATION` (`strict.rs:50-56`, panic = abort, `Cargo.toml:58`); that a real device loss reaches this path is UNVERIFIED. CAND-4 makes no GPU fault-tolerance claim. |
| 2 | Omega in-settle crash hooks | NOT COVERED; required by the scope, missing | The hooks exist only in `AIEN_TEST_BUILD` omega builds (`rx_compose.c` `#ifdef RXC_TEST_HOOKS`; `make test-prod-refuses-test-pieces` refuses them in production); `librx_compose.a` is built without them. A daemon crash during S3 (the World settle) is a recovery event on the daemon path, so the scope needs it. Smallest test (proposed, not in this spec): on `cpu_fault`, no hook, SIGKILL the daemon 1 s into `aien compose propose` (C3b's CPU propose), 3 reps; PASS iff the restart either opens with R4 (identity, prefix digest over the pre-S3 records) and no effect, or refuses with a named code that `aien compose recover` clears with a repair record. |
| 3 | capability-root revocation on the live effect path | OUT OF SCOPE, NOT COVERED | Not on the path: no sovereign-core crate names `caproot`, `CapabilityRef`, `aienos_cap` or `RX_OP_REVOKE` (`grep -rln` over `crates/` empty at `296c4ac`; ACCEPTANCE-v2 A11). The path's own revocation, Cortex authorization revoke, is tested by C5a and C5b; that is not capability-root revocation and is not reported as such. |
| 4 | J-Space spill corruption with nonempty spilled data | CONDITIONAL: C6d | C6d injects only if `jspace.data` is nonempty or `spill_end > 0`; otherwise its result is NOT_APPLICABLE (never PASS). At omega 62b6a28 the composition never wrote `jspace.data` (ACCEPTANCE-v3 G5; v4 C6d NOT_APPLICABLE x3); at `omega_commit` this is UNVERIFIED and measured by C6d's guard. If C6d is NOT_APPLICABLE the case is NOT COVERED, and not required: the CAND-4 workflow then has no spilled data to damage. |
| 5 | real interruption between journal append and record-mark update | NOT COVERED; required by the scope, missing | C6c-ctl reproduces the on-disk state only (the old mark put back), not a real kill. The window exists: `advance_mark` runs after the appends returned (`spine.rs:882-909`) and writes the mark by tmp + fsync + rename (`cortex_mark.rs:92-118`, `write` at :94). Smallest test (proposed, not in this spec, no code change): on `cpu_fault`, no hook, N = 20 trials of `authorize` with a SIGKILL of the daemon at a random 0-50 ms offset; restart each; PASS iff every restart opens with no `E_MARK` and recall ok (R4), and at least one restart logs `record mark advanced` (evidence the window was hit; none hit = NOT_RUN). Deterministic variant needs a new daemon hold point (code change, later candidate). |
| 6 | coordinated rollback or modification of durable stores and their record mark | OUT OF SCOPE, NOT COVERED | Requires an actor with write access to the owner's home who rewrites journal and mark together; the mark is a crash-consistency check, not tamper resistance (NEXT-PHASE-2 not-proved list). CAND-4 claims no tamper resistance against local writers. |

The CAND-4 recovery claim is limited to the Q2 rows as run. Cases 2 and 5 are named in the
verdict as required and missing; CAND-4 does not claim them.

## 7. Receipts, verdict, what is never done

- Every launch and every scoring step writes a receipt named by the sha256 of its content into
  `receipts/` here (never edited), including failed launches; reply bytes as
  `receipts/replies/<sha256>.txt`; an `INDEX.md` line each.
- CAND-4 qualification PASS iff: every digest check passed, H PASS (H1, H1c, H2, H3, H4, H4c,
  H5), Q1 PASS and Q2 PASS. Otherwise FAIL, naming each failing row. NOT_RUN, NOT_APPLICABLE,
  INCOMPLETE and UNVERIFIED are never PASS.
- No launch is repeated, no threshold changes after results are seen, no run is repeated until
  it passes, no tuning. `VERDICT-CAND4.md` is written after the runs: PASS or FAIL per row,
  failed attempts kept and named, the limits of section 6, anything UNVERIFIED.
