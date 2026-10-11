# OPEN-MODEL-QWEN3 v6: frozen qualification specification (Qwen3-4B-Instruct-2507)

**Status: FROZEN 2026-10-11 (content: rows, thresholds, tasks, limits, scoring, model, product commit). NOT RUN.**

Owning issue: aien-dev/aien-sovereign-core#384 (lane L10, Drake ruling 2 of 2026-10-10 on aien-architecture#190: Qwen3-4B-Instruct-2507
is the public-release candidate, conditional on a frozen qualification PASS; a FAIL blocks the public bundle; no silent substitution).
Written by Lane B of the overnight program (coordinator session 61b069). This file is merged before any v6 launch.

## 0. What is and is not established

- **v1 to v5 stand as recorded.** v5 is FAIL, 262 of 334 declared rows (VERDICT-v5.md, sc#365, f83de08). No v1 to v5 file is
  changed by v6; `test-v6.sh` proves every v5 campaign file still equals its v5 freeze sha256 (`evidence-v5/campaign-files.sha256`).
- **Why v5 failed** is analysed row by row in `V5-FAILURE-ANALYSIS.md`: 70 rows model-output weakness (D1 wrote 8 lines where 22
  were asked; D2 wrote "Wrap Up" as a label, not a heading; D6 wrote "Bringing Her Home" as plain text), 2 rows time budget (D6's
  second attempt), 0 runtime, 0 chat-template, 0 scorer. No product bug was found, so no fix PR precedes v6.
- **v6 is the same qualification on the current product.** Same eleven tasks and goals, same seeds, same 334 declared rows, same
  thresholds, same limits, same scorer, same model files. What differs is the product commit (28 commits after the v5 build,
  including the requirement-reader, destination, input-reading and append-placement changes merged 2026-10-09 to 2026-10-10)
  and the run identity (run base, output tag, hold names). **No harness setting changed** (Section 3).
- **A FAIL is recorded as FAIL** and keeps the public bundle blocked. No row, threshold, goal, limit or model is changed after
  this file merges, and no launch is repeated.

## 1. Rows, tasks, limits and scoring: identical to v5, by file

v6 runs the v5 files themselves, not copies. Each is bound by its sha256 in `evidence-v5/campaign-files.sha256` (checked by
`test-v6.sh` section 1):

| What | File (unchanged, byte-identical to the v5 freeze) |
|---|---|
| tasks, goals, destinations, seeds, caps, budgets | `tasks-oq3-v5.json`, `seed-v5/` (ACCEPTANCE-v5 Section 2 and 2.1, including Drake's D6 adjustment 1536 tokens / 170 s) |
| rows and how each is counted | `rows-oq3-v3.jq`, `rows-oq3-v4.jq`, `rows-oq3-v5.jq`, `v5-rows.sh`, `../next-phase-1/rows-v5.jq`, `../next-phase-1/rows-v9.jq` (ACCEPTANCE-v5 Section 3) |
| declaration | `../scoring/declarations/oq3-v5.decl.json`, QUALIFICATION_ROWS=334 (D1 34, D2 36, D3 36, D4 35, D5 36, D6 33, E1 33, E2 34, R1 32, N1 11, N2 14) |
| driver and receipt builder | `../next-phase-1/run-campaign.sh`, `../next-phase-1/make-receipt.sh`, `../next-phase-1/v8-results.sh` |
| scorer and verdict rule | `../scoring/score-rows.sh` (scoring-v5): **PASS only if every declared row is present once and PASS** (ACCEPTANCE-v5 Section 5) |
| row definitions (text) | ACCEPTANCE-v5.md Sections 3.1 to 3.5 (the receipt builder reads its task table, `TASK_ACCEPTANCE`) |

The verdict rules of ACCEPTANCE-v5 Section 5 apply unchanged: one run, no rerun of a failure, no repeated launch, no retry after
seeing a result, no task edit, a spec defect found after the run is a finding for a later campaign. The verdict names, for each
failing launch, whether its first failing row is a product row (`PRE`, `OPR`, `PIN`, `GR`, `N1-NR`) or a model row.

## 2. New v6 files (names only)

| File | What it is |
|---|---|
| `run-qwen3-v6.sh` | `run-qwen3-v5.sh` with names changed only: frozen values `frozen-v6.json`, pinned run base `/home/drakestapleton/workspace/oq3-v6-runs`, output tag `oq3-v6`, message and receipt labels. `test-v6.sh` section 2 proves that every line after the header equals the v5 wrapper under exactly these substitutions. The launch record keeps the v5 name and format, `<id>.launch-v5.json`, because the row text of ACCEPTANCE-v5 names it. |
| `frozen-v6.json` | `frozen-v5.json` with the eight build pins of Section 5 and a new status line; the `model` block is byte-identical (checked). |
| `test-v6.sh` | the v6 self-test (Section 7). |
| `evidence-v6/` | build script and summary, model re-hash, requirement and destination probes, CPU decode probe, self-test outputs, and the prepared launchers `run.sh` and `run-part.sh` (G6 dry part 1, then real parts 1 to 4, each in its own hold). |

## 3. Harness settings: none changed

Environment, exactly as ACCEPTANCE-v5 Section 7 and enforced by the wrapper (unchanged lines):

```text
AIEN_KV_CONTEXT_TOKENS            = 4096
AIEN_REQUIRE_BLACKWELL            = 1      (GB10 native Omega engine, no CUDA; a missing GPU engine is fatal)
AIEN_REQUIRE_CHECKPOINT           = 1
AIEN_GB10_QWEN3_DECLARED_ATTEMPT  = 1      (opt-in while omega#327 is open)
AIEN_COMPOSE_EDIT_BUDGET_MS / AIEN_COMPOSE_DOC_BUDGET_MS / AIEN_COMPOSE_MAX_TOKENS / AIEN_COMPOSE_DOC_MAX_TOKENS
                                  = set per launch by the wrapper from tasks-oq3-v5.json (documents 1024 tokens and 120 s,
                                    D6 1536 and 170 s, edits 96 and 29 s, N2 16, R1 96)
unset: AIEN_FORCE_CPU_STUB, AIEN_DEV_FALLBACK, AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK, AIEN_COMPOSE_BUDGET_MS, AIEN_OMEGA_SPIN_US, AIEN_OMEGA_CTA_BUDGET
```

No token cap and no time budget is raised. v5 had no token cut on any failing launch (every reply ended at `eos`), so a cap
change would not address any v5 failure.

## 4. Path: GB10 with the R1 CPU reference leg; why not a CPU-only run

The v5 rows are a GB10 qualification by construction: row `<id>-BE` (`rows-oq3-v4.jq`) requires that every daemon start names
the `OmegaGb10` backend and none names cpu, stub, reference or fallback, and the wrapper refuses `AIEN_REQUIRE_BLACKWELL` other
than 1. The CPU path is exercised where v5 put it: R1 replays its prompt on `ReferenceCpuBackend` and rows R1-X, R1-P and R1-T
require the same prompt and output token ids on both paths.

A CPU-only run of these rows cannot pass and would measure nothing about the model (MEASURED, `evidence-v6/cpu-decode-probe-2026-10-11.txt`):
the CPU reference decodes about 1 389 ms per output token (GB10 about 103 ms in v5), so a 540-token D1 reply needs about 12.5
minutes against its 120 s budget, D6 about 24 minutes against 170 s, and the product refuses any budget above 599 s
(`spine.rs`, `COMPOSE_BUDGET_MS_RANGE`); every `<id>-BE` row would also fail. A CPU-only qualification needs its own rows and
limits: a new campaign and a Drake decision, not part of v6.

## 5. Model, product commit and build (frozen values in `frozen-v6.json`)

Model: Qwen/Qwen3-4B-Instruct-2507, Hugging Face revision cdbee75f, Apache-2.0, directory
`/home/drakestapleton/models/qwen3-4b-instruct-2507-cdbee75/`, the same files as v5. Re-hashed 2026-10-11 with no GPU hold
(`evidence-v6/model-rehash-2026-10-11.txt`), every value equal to ACCEPTANCE-v5 Section 7:

```text
model.safetensors.index.json       d6c42883a895dfef5b0080ed2116a1bcd764f558406b98923d675978a1abf29c
model-00001-of-00003.safetensors   75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6
model-00002-of-00003.safetensors   0b48adbb1f60e901153d91907ba11ce63bd4b8b584482e730f48808d055dfba1
model-00003-of-00003.safetensors   7dd39ccca5e4de123c74c14af44c9bf2eb75df33b4614382af0134528e060d5d
tokenizer.json                     aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4
tokenizer_config.json              a62ff0a2472a0fa1b8eaabcb57c59b58afa42a22831dc141400b6e0cf2b65ce3
config.json                        5beea1a4a34c62782bfb2f911c606741a3bab8f92d80a118fa053c28af12e8ba
generation_config.json             835fffe355c9438e7a25be099b3fccaa98350b83451f9fd2d99512e74f1ade48
model_sha256 (index+shards)        17a78fbba447a4e66a3d886c0998fbcf2f9201d46e5e0c5bfec2d57c975976b7   (manifest, as frozen-v5.json)
```

Product build: clean clone of sovereign-core at **de9c626ad9fd50652c1f767d273b4d63f118e342** (main on 2026-10-11 00:55Z), pinned
omega, physics and aienos materialized by `scripts/pins.sh --gpu`, release build of `aien-cli` and the examples `np1_reference`
and `np1_edit_merge` (`evidence-v6/build.sh`; output `evidence-v6/build-summary.txt`, all three exits 0, build log shows
`has_omega_compose`, `has_omega_gpu`, `has_omega_wait_ms`; every checkout clean). Build directory
`/home/drakestapleton/workspace/oq3-v6-build-de9c626`.

```text
sovereign-core commit   de9c626ad9fd50652c1f767d273b4d63f118e342
omega.lock commit       6c6180cf378075b61291f4565d226eba38b4decd   (unchanged since v4 and v5)
physics commit          6d7cf0d4d8eb2cda7b512100ff6058e25dbb3ddf   (unchanged since v5)
aienos.lock commit      b84c0a67590a934f3f3e001b12ec85ebc086a9eb   (unchanged since v5)
Cargo.lock sha256       792a99d82e2f92a40b67a576789767c0677b91871a3aa7ffa0b429455dc050bf
aien-cli sha256         2a6b09d00c8dbe9237ae1410a9469b6747d232c72468a2c853fe491cf92c2c7a
np1_reference sha256    41dfe956ce48f73168c4be6a1988a66455ea4d4d93b4a093f331bd8b3a6b28ef
np1_edit_merge sha256   00ba3fb68f46fee46a25aae000de80822a319648250f0c4e15d4cd6dc2ce4aef
```

The pins name the product build. The campaign files (wrapper, this file, `frozen-v6.json`) come from the main commit that merges
this file and are bound by the wrapper's per-part identity file; a later campaign-file commit does not change a pin, and the
wrapper refuses a run whose binaries or checkouts differ from `frozen-v6.json`.

## 6. Run plan: one chip slot, four parts

Run base pinned in the wrapper: `/home/drakestapleton/workspace/oq3-v6-runs` (fresh; the prompt embeds the workspace path).
Four parts, each in its own quietlock hold of 20 minutes or less, back to back, owners `laneB-oq3-v6-p1` to `laneB-oq3-v6-p4`:
part 1 = D1 D2 D3, part 2 = D4 D5 D6, part 3 = E1 E2 N1, part 4 = N2 R1 and the score. A part is never extended; part k needs part
k-1 complete in the same run base with the same binaries and files. v5's measured hold times were 7 min 14 s, 8 min, 3 min 17 s
and 4 min 1 s (VERDICT-v5 Section 3): about 23 minutes of chip time in all (UNVERIFIED for v6, same limits, so the worst case
of ACCEPTANCE-v5 Section 8 still fits each 20-minute hold).

Kept for the verdict (as v5): hold start, end and exit per part, `/proc/meminfo` and NVRM kernel-log line counts before and
after each part, the S3 report of each launch, the sha256 of every file in the run base, the receipts, results, score file and
committed replies.

## 7. Gates (all on de9c626 unless stated)

| Gate | What | Result |
|---|---|---|
| G1 asked the model | requirement reader over the eleven goals: no uncertain span for any goal | **MET**: identical line for line to the v5 probe on b2cae6d (`evidence-v6/requirements-probe-de9c626.txt`) |
| G3 destinations | every positive goal resolves its destination (E1, E2 as edits of their seeds); N1 refused for its destination with the `N1-NR` reason text | **MET** (`evidence-v6/destination-probe-de9c626.tsv`) |
| G5 self-test | `test-v6.sh` passes; `test-v5.sh` passes except its one post-run check (its pinned v5 run path exists since the v5 run) | **MET**: 20 of 20 (`evidence-v6/test-v6-de9c626.txt`); 191 of 192 with that one expected check (`evidence-v6/test-v5-de9c626.txt`) |
| G6 GPU dry run | **one** dry run of part 1 (D1 D2 D3) with `OQ3_DRY_TASKS=tasks-oq3-v5.json` in its own hold, run base outside the pinned path, outputs kept and never scored, immediately before the real run in the same chip window. Purpose: prove the 28-commit newer product still runs the v5 driver end to end on GB10 (daemon start lines, `decoding` in S3 attempts, OP_REPORT, CHECKPOINT_SHARDS). If any dry launch fails for a product reason (no `run.json`, a step error not caused by the model reply), the real run does not start and the cause is reported. The dry run sees the same goals v5 already answered, so it reveals nothing new about them | to run |
| G7 build | clean build at the pins, all exits 0, checkouts clean | **MET** (`evidence-v6/build-summary.txt`) |
| freeze | this file merged on main before any launch; `frozen-v6.json` status FROZEN with every pin filled | this PR |

## 8. Prediction (stated before any run; UNVERIFIED)

Decoding is greedy and no merged change targets how the model lays out lines or headings, so the v5 replies are likely to recur
(the new run base changes a few prompt tokens, so they may differ slightly). Per launch, probability of passing every row:
D1 0.15, D2 0.2, D6 0.15 (the v5 failures); D3, D4, D5 0.7 each (each passed in v5 only on its second attempt; D4's prompt may now
include its source file, sc#392); E1 0.85, E2 0.8 (append placement changed by sc#394); N1 0.9, N2 0.9, R1 0.85.
**Whole qualification PASS: about 5 percent. Expected verdict: FAIL, with D1, D2 and D6 failing on model rows.** A FAIL is an
honest and acceptable outcome; it keeps the public bundle blocked.

## 9. What v6 does not prove

- One run, one launch per task: one observation each, not a rate. Greedy decoding, one model, one machine, GB10 at the pins.
- A PASS would qualify Qwen3-4B-Instruct-2507 on these eleven tasks under these rows only; it would not close omega#327,
  omega#277 or sc#277, would not make Qwen3 on GB10 the default, and would not claim release readiness. The CAND-5 manifest in
  aien-architecture `qualification/candidates/` is drafted only on PASS and left for the coordinator.
- A FAIL says the model, as prompted by this product, did not meet these stated document rules; it does not say whether a
  different prompt, sampling or retry policy would. That is a new campaign with its own frozen spec.
- The CPU-only path is not qualified (Section 4).
