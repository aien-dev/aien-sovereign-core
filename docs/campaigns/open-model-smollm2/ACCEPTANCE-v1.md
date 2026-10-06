# OPEN-MODEL-SMOLLM2 campaign v1: acceptance criteria (frozen before any campaign run)

```text
campaign_id  = "open-model-smollm2"
spec_version = 1
status       = FROZEN at the commit that adds this file. Nothing in this file changes
               after that commit. The run wrapper (run-smollm2.sh) lands in the same
               commit and is part of the freeze; no campaign run happens before it.
engine       = sovereign-core PR #233 (ChatML template read from the model's own
               files; per-template compose assistant prefix), head 4aaa4e6.
```

Purpose: decide, on evidence, whether SmolLM2-1.7B-Instruct (Apache-2.0) can do what the
Llama-3.2-1B baseline did in NEXT-PHASE-1 v5 (PASS, sovereign-core #225), so the outside
release is no longer tied to the Llama licence. The Llama baseline stays the CAND-4 model
whatever this campaign says.

## 1. What is reused unchanged

The whole NEXT-PHASE-1 v5 acceptance and scoring, by reference, not copied:
- `docs/campaigns/next-phase-1/ACCEPTANCE-v5.md` Sections 1, 2, 3, 4 (eight steps S1..S8, the
  three tasks T1..T3 with goal, destination and required phrases, rows Q1..Q4 and A1..A6,
  every v1..v4 row, the verdict rule). The task table is `next-phase-1/tasks-v5.json`
  (unchanged, so the same three goals as the Llama run).
- Driver `next-phase-1/run-campaign.sh`, receipt builder `next-phase-1/make-receipt.sh`,
  rows `next-phase-1/rows-v5.jq`, all at their state in the commit that adds this file.
- Row Q4 already lists `<|im_start|>`, `<|im_end|>` and `<|endoftext|>`, the ChatML markers.
- Verdict rule: a task is PASS only if every row passes; the campaign is PASS only if all
  three tasks PASS. No task is repeated, no threshold changes after results are seen, no
  run is repeated until one passes. A launch where the daemon does not start is that
  task's FAIL. Performance is an observation, not a row.

## 2. Frozen model, budget and engine

```text
model_id          = HuggingFaceTB/SmolLM2-1.7B-Instruct, revision 31b70e2e869a7173562077fd711b654946d38674
                    LlamaForCausalLM, 24 layers, hidden 2048, 32 heads, head_dim 64, tied lm_head,
                    vocab 49152, chat template ChatML, stop token <|im_end|> (id 2), licence Apache-2.0
                    (model card metadata; no LICENSE file upstream), local dir
                    ~/models/SmolLM2-1.7B-Instruct-31b70e2e869a (read-only, PROVENANCE.md there)
model_sha256      = f55217be716b6a997b97b9d8d7eb6fad02e00858f5010ec24f64603c3a98a0e8  (model.safetensors)
config_sha256     = 994f50b16abb4ae00880baefe03c10260b5bd608d2bf586f7056ca05a534feea
gen_config_sha256 = 87b916edaaab66b3899b9d0dd0752727dff6666686da0504d89ae0a6e055a013
tokenizer_sha256  = 9ca9acddb6525a194ec8ac7a87f24fbba7232a9a15ffa1af0c1224fcd888e47c  (tokenizer.json)
max_tokens        = 64   (AIEN_COMPOSE_MAX_TOKENS)
attempt budget A  = 12 000 ms   (the production constant COMPOSE_ATTEMPT_BUDGET, NOT changed)
skill budget B    = 29 000 ms   (the production constant COMPOSE_SKILL_BUDGET, NOT changed;
                                 the 30 000 ms quiescence wait of the compose skill is not raised)
GPU engine        = omega.lock 62b6a2899fa230e57fcf7855ea7e2c26404514b6; libomega_gpu.a
                    sha256 ef80e4313d4e678335dceac3493aadec8d0df4352dd5356854b92e0a9e070887
backend           = OmegaGb10Backend (native engine, no CUDA), GB10 required
decoding          = greedy (temperature 0); stop on the model's end-of-sequence token
assistant prefix  = `filename:` (no trailing space), proposal template unchanged from v5
```

No production-path change relative to main other than PR #233 (ChatML template, same
assistant prefix text). The proposal template is not touched.

### 2.1 Why these numbers (measured before the freeze, diagnostic, not a campaign run)

Measured on the GB10 through the same engine (`smollm2diag`, six prompts, 2026-10-06):
prefill 3.6 to 4.3 s for 122 to 138 prompt tokens; decode 3.5 to 3.7 tokens/s (about 286 ms
per token); warm-up 16.6 s (declared, outside the task window); VmHWM about 61 GB.

- A full 96-token attempt would cost 4.3 + 96 x 0.286 = about 31.7 s, more than B (29 s).
  A full 64-token attempt costs about 22.6 s and fits B once. This is why max_tokens is 64
  and not the 96 of the Llama v5 run, and why B is not raised.
- A normal reply ends at end-of-sequence long before the limit (12 to 22 tokens in the
  diagnostic, 7.5 to 10.7 s per attempt). A retry prompt is about 40 tokens longer
  (about +1.2 s prefill), so a retry costs about 8.7 to 12 s. A = 12 000 ms is therefore
  a measured fit for a retry of a normal reply, and is NOT the cost of a reply that runs to
  the token limit. If attempt 1 is a normal reply (about 8 to 10 s) at least 19 s are left,
  so attempt 2 starts; after a 22 s runaway attempt 6 s are left, less than A, and no
  attempt 2 starts. Both cases are the retry policy working as written
  (ACCEPTANCE-v3 Section 3b).
- The campaign receipts record each attempt's `ms`, `tokens` and `finish_reason`; the
  measured per-attempt numbers are reported next to this estimate. They do not change
  A or any row.
- Fidelity (same diagnostic): token ids from the AIEN tokenizer equal Hugging Face on all six
  prompts; the AIEN CPU reference is token-identical to Hugging Face fp32 greedy on all six;
  the GB10 is identical on four of six and differs on two (T1 at token 5, top-two logit gap
  0.00046; V3 at token 6, gap 0.076), both near-ties. This is a stated limit: GB10 output
  can differ from the reference on near-ties. The CPU diagnostic replies for T1..T3 were
  seen before this freeze: T1 and T2 produced an in-format answer wrapped in `<...>`; T3
  ran to the token limit with a markdown heading list. A FAIL of T3 on Q3 is therefore
  likely; the campaign is still run once, as frozen, and the verdict is whatever the rows give.

## 3. Run conditions

- Three driver launches (T1, T2, T3), one per task, in that order, each inside a GB10 hold:
  `quietlock hold --minutes <= 20` with start and release whispers (`crumb whisper`). The
  hold is taken by whoever is ready first; a held lock is waited for, never broken. The
  Spark GPU is used only when `~/workspace/.spark-quiet` does not exist.
- Environment: `AIEN_BIN` built from the commit named in the receipt, `AIEN_MODEL_PATH` the
  frozen safetensors file, `AIEN_TOKENIZER_PATH` the frozen tokenizer.json,
  `AIEN_COMPOSE_MAX_TOKENS=64`, `AIEN_REQUIRE_BLACKWELL=1`, `AIEN_FORCE_CPU_STUB` unset.
- Before the run, the model files' sha256 are re-checked against Section 2 (the wrapper does
  this); a mismatch aborts the run before any launch and is not a task result.
- Receipts: one per task in this directory, named by content sha256, with replies under
  `replies/`, written by `make-receipt.sh` (its SPEC string names this file). `VERDICT-v1.md`
  (written after the run) names the three receipts and the verdict, and sets them next to
  the Llama v5 receipts as an observation.

## 4. What a PASS and a FAIL mean

- PASS: SmolLM2-1.7B-Instruct completed the three v5 tasks under the same rows as Llama.
  It is then a candidate for the open-model release lane; it does not by itself replace
  CAND-4 (a later candidate, own qualification, per the CAND-4 decision).
- FAIL: the rows say which task and row. The Llama baseline is unaffected. The next models
  per the open-model search report (SmolLM3-3B, Qwen3-4B-Instruct-2507) need engine work.
