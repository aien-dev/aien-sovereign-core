# OPEN-MODEL-QWEN3 campaign v1: acceptance criteria (frozen before any campaign run)

```text
campaign_id  = "open-model-qwen3"
spec_version = 1
status       = FROZEN at the commit that adds this file. Nothing in this file changes
               after that commit. The run wrapper (run-qwen3.sh) lands in the same
               commit and is part of the freeze; no campaign run happens before it.
engine       = sovereign-core main 3c247ab (#274), omega 8887454 (omega.lock),
               aienos b84c0a6 and physics 6d7cf0d (omega's aienos.lock and physics.lock).
               The campaign files add no code.
```

Purpose: decide, on evidence, whether Qwen3-4B-Instruct-2507 (Apache-2.0) can do what the
Llama-3.2-1B baseline did in NEXT-PHASE-1 v5 (PASS, sovereign-core #225), under the same
rows that SmolLM2-1.7B failed (open-model-smollm2 VERDICT-v1). The Llama baseline stays the
CAND-4 model whatever this campaign says. The weights keep their upstream licence and are
not relabelled.

## 1. What is reused unchanged

Exactly the open-model-smollm2 v1 arrangement (its ACCEPTANCE-v1 Section 1), by reference:
- `docs/campaigns/next-phase-1/ACCEPTANCE-v5.md` Sections 1 to 4 (steps S1..S8, tasks T1..T3,
  rows Q1..Q4 and A1..A6, every v1..v4 row, the verdict rule), task table
  `next-phase-1/tasks-v5.json`.
- Driver `next-phase-1/run-campaign.sh`, receipt builder `next-phase-1/make-receipt.sh` with
  `TASK_ID` set and its default `V6_ROWS` (rows-v6.jq), rows `next-phase-1/rows-v5.jq`; all
  at their state in the commit that adds this file. Whatever rows that builder evaluates are
  the rows of this campaign.
- Verdict rule: a task is PASS only if every row passes; the campaign is PASS only if all
  three tasks PASS. No task is repeated, no threshold changes after results are seen, no
  run is repeated until one passes. A launch where the daemon does not start (for example
  the GPU channel open failing with NV_ERR_NO_MEMORY, omega#327) is that task's FAIL.
  Performance is an observation, not a row.

## 2. Frozen model, budget and engine

```text
model_id          = Qwen/Qwen3-4B-Instruct-2507, revision cdbee75f17c01a7cc42f958dc650907174af0554
                    Qwen3ForCausalLM, 36 layers, hidden 2560, 32 heads, 8 KV heads, head_dim 128,
                    per-head q/k RMSNorm, tied lm_head, vocab 151936, chat template ChatML,
                    stop ids 151645 (<|im_end|>) and 151643, licence Apache-2.0 (LICENSE file in
                    the model directory), local dir ~/models/qwen3-4b-instruct-2507-cdbee75 (read-only)
index_sha256      = d6c42883a895dfef5b0080ed2116a1bcd764f558406b98923d675978a1abf29c  (model.safetensors.index.json)
shard1_sha256     = 75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6  (model-00001-of-00003.safetensors)
shard2_sha256     = 0b48adbb1f60e901153d91907ba11ce63bd4b8b584482e730f48808d055dfba1  (model-00002-of-00003.safetensors)
shard3_sha256     = 7dd39ccca5e4de123c74c14af44c9bf2eb75df33b4614382af0134528e060d5d  (model-00003-of-00003.safetensors)
config_sha256     = 5beea1a4a34c62782bfb2f911c606741a3bab8f92d80a118fa053c28af12e8ba
gen_config_sha256 = 835fffe355c9438e7a25be099b3fccaa98350b83451f9fd2d99512e74f1ade48
tokenizer_sha256  = aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4  (tokenizer.json)
max_tokens        = 64   (AIEN_COMPOSE_MAX_TOKENS)
attempt budget A  = 12 000 ms   (production COMPOSE_ATTEMPT_BUDGET, NOT changed)
skill budget B    = 29 000 ms   (production COMPOSE_SKILL_BUDGET, NOT changed)
KV context cap    = 4096 tokens (AIEN_KV_CONTEXT_TOKENS, the operator cap from #270; the model
                    declares 262144, which the host cannot hold; v5 prompts are far below 4096)
GPU engine        = omega 8887454, built from source by aien-omega-gpu and aien-omega-compose
                    (HEAD == omega.lock), no CUDA
backend           = OmegaGb10Backend (native engine), GB10 required (AIEN_REQUIRE_BLACKWELL=1)
GB10 Qwen3 opt-in = AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1. Main refuses Qwen3 on the GB10 by
                    default while omega#327 is open; this campaign is the declared attempt
                    that the opt-in exists for. The daemon prints its warning line every time.
decoding          = greedy; stop on the model's end-of-sequence ids
assistant prefix  = `filename:` and the proposal template, unchanged from v5
```

The daemon's own `model_sha256` hashes only the file named by `AIEN_MODEL_PATH`, which for a
sharded model is the index. The wrapper therefore checks every shard against this section
before any launch; a mismatch aborts the run before any launch and is not a task result.

### 2.1 Why these numbers (measured before the freeze, diagnostic, not a campaign run)

Two diagnostic daemon starts on the GB10, same binary and model, 2026-10-07 06:40Z and 06:42Z
(sovereign-core#277 working notes; receipts on the Spark at
~/workspace/hive/WQWEN/c277/diag/). Goals were made up and are not T1..T3 or any other
NEXT-PHASE-1 task; **no T1..T3 goal was given to Qwen3 before this freeze**, on any backend.
- Warm-up: 1 token over 122 prompt tokens in 31 106 ms (68 s from launch to the warm-up line,
  weight load included). VmHWM about 27 GB.
- Proposal attempts: 89 prompt tokens + 9 output tokens 7 765 ms; 90 + 9 tokens 7 952 ms
  (repeat); 84 + 27 tokens 13 199 ms. All ended at end-of-sequence and parsed.
- Slope about 291 ms per output token, fixed part about 5.3 s at about 90 prompt tokens.
  A full 64-token attempt is then about 24 s, inside B (29 s) once; a normal short reply
  (9 to 27 tokens) is 8 to 13 s. A retry starts only when at least A is left
  (ACCEPTANCE-v3 Section 3b), so after a long first attempt no retry starts. These are
  estimates; the receipts record each attempt's `ms`, `tokens` and `finish_reason`.
- In both diagnostic starts the GPU session opened after the weight load (main's order) with
  MemFree falling to about 4.6 GB afterwards, and no NV_ERR_NO_MEMORY occurred. omega#327
  remains open; a recurrence in the campaign is a task FAIL, not a reason to rerun.

Fidelity: Qwen3-4B on the CPU reference matches Hugging Face (top-20 within 5e-5, same top
token, sovereign-core #265/#274) and on the GB10 matches within 0.018 against the 0.15 bound
on one prompt (#274 attempt 2 and the #277 baseline run). GB10 output can differ from the
reference on near-ties; that is a stated limit.

## 3. Run conditions

- Three driver launches (T1, T2, T3), one per task, in that order, inside one GB10 hold
  `quietlock hold --minutes 20` with start and release whispers (`crumb whisper`). An overrun
  is recorded by quietlock and is not a reason to stop a launch; nothing is killed.
- Environment: `AIEN_BIN` built from sovereign-core 3c247ab with `AIEN_OMEGA_DIR` at omega
  8887454 (compose library linked, not the stub), `AIEN_MODEL_PATH` the frozen index file,
  `AIEN_TOKENIZER_PATH` the frozen tokenizer.json, `AIEN_COMPOSE_MAX_TOKENS=64`,
  `AIEN_KV_CONTEXT_TOKENS=4096`, `AIEN_REQUIRE_BLACKWELL=1`,
  `AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1`, `AIEN_FORCE_CPU_STUB` unset. The wrapper refuses to
  start if any of these four values differ.
- The page cache is not dropped and no system setting is changed before or during the run.
- Receipts: one per task in this directory, named by content sha256, with replies under
  `replies/`, written by `make-receipt.sh` (its SPEC string names this file). `VERDICT-v1.md`
  (written after the run) names the three receipts, the binary sha256 and the verdict, and
  sets them next to the Llama v5 and SmolLM2 v1 receipts as an observation.

## 4. What a PASS and a FAIL mean

- PASS: Qwen3-4B-Instruct-2507 completed the three v5 tasks under the same rows as Llama, on
  the GB10, through the production daemon with its restart step, with the GB10 Qwen3 path
  enabled by the declared opt-in. It is then the open-model candidate for a later frozen
  candidate with its own qualification; it does not by itself replace CAND-4. It does not
  close omega#327: one campaign without the failure is not a rate.
- FAIL: the rows say which task and row. The Llama baseline is unaffected.
