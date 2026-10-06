# Llama 3.x engine cut: verification record

Date: 2026-10-06. Branch `next-phase/engine-llama3`, based on
`next-phase/compose-v5` (1ac121d). Machine: DGX Spark (GB10, sm_121).

Tags: **PROVEN** means measured here with the command shown.
**INFERRED** means it follows from code reading or from a related
measurement. **UNVERIFIED** means not checked.

## What the cut does

1. `ModelConfig` and the tensor catalog come from the model's own
   `config.json` (`aien_inference_abi::load_model_config`,
   `checkpoint::llama_catalog`). Anything that is not
   `LlamaForCausalLM`, uses an activation other than `silu`, has
   attention or MLP bias, or uses a `rope_type` other than `default` or
   `llama3` is refused with the field named.
2. Tied embeddings: when `tie_word_embeddings` is true the catalog has no
   `lm_head.weight`, `TransformerWeights.lm_head` is `None`, and
   `output_projection()` returns the embedding table itself. No copy is
   made. Both the CPU reference and the Omega GPU backend read the output
   projection through that one accessor.
3. llama3 rope scaling (`Llama3RopeScaling`, `RopeParams`) follows the
   wavelength bands of the HF reference. Plain rope keeps the exact old
   expression, so TinyLlama is bit identical.
4. `ChatTokenizer::from_model_dir` picks the chat template (Zephyr or
   Llama 3 instruct) from `tokenizer_config.json` or `chat_template.jinja`,
   the stop set from `generation_config.json` `eos_token_id` (falling back
   to `config.json`), and the context length from
   `max_position_embeddings`. The template text carries no BOS; the
   tokenizer post-processor adds exactly one.
5. `COMPOSE_ASSISTANT_PREFIX` is now `"filename:"` (compose-v5 had
   `"filename: "` with a trailing space). One constant, used by the
   daemon proposer, the gate driver and the tests.
6. `finish_reason` reuses compose-v5's plumbing (commit 2af89bd); this
   cut only checks that it reaches the receipt (see below).
7. The daemon reads `config.json` when it sits next to the checkpoint
   (`AIEN_MODEL_PATH` / `AIEN_MODEL_DIR`); with no such file it keeps the
   built-in TinyLlama config. The default model is unchanged.

Side fix found on the way: the batched decode path had a hard-wired stop
set `{0, 1, 2}`. In the Llama 3 vocabulary those ids are `!`, `"` and `#`,
so a reply would have ended at the first `!`. The batch path now uses the
model's stop set (`ModelConfig::stop_token_ids`, legacy `{0,1,2}` only
when the config names none).

## Inputs (sha256)

| model | file | sha256 |
|---|---|---|
| TinyLlama-1.1B-Chat-v1.0 | model.safetensors | 6e6001da2106d4757498752a021df6c2bdc332c650aae4bae6b0c004dcf14933 |
| | config.json | 486bedda3a6988332e60d9638a09ca4b260d34ebcf1b19e22cf3b140b63d8fe9 |
| | generation_config.json | 18046d04f5bd8b4998095ecabdd17a1bf0053d9acdccead4a05be4a3575f3c5c |
| | tokenizer.json | bcd04f0eadf90287bd26e1a183ac487d8a141b09b06aecb7725bbdd343640f2e |
| unsloth/Llama-3.2-1B-Instruct (5a8abab) | model.safetensors | 1ff795ff6a07e6a68085d206fb84417da2f083f68391c2843cd2b8ac6df8538f |
| | config.json | dfb67fd8afe73a1c75245824ef9d64a6ba8983025447e3bf76aa1ea57ee46152 |
| | generation_config.json | 6d4f979915331212d7672c68b22a4ddad9e21ed8126cf2bd1ea6b2b88f595c1c |
| | tokenizer.json | 6b9e4e7fb171f92fd137b777cc2714bf87d11576700a1dcd7a399e7bbe39537b |
| | tokenizer_config.json | 9ddd255c19fe319c8d4e891163540382e9fbda99f394674f2a929efc47d57458 |
| | chat_template.jinja | 5816fce10444e03c2e9ee1ef8a4a1ea61ae7e69e438613f3b17b69d0426223a4 |
| unsloth/Llama-3.2-3B-Instruct (006f5dc) | model-00001-of-00002.safetensors | 13cbd6d16e927a0c5bad54102514e6e18b4a47b3a6eb911e39d678d328d19f55 |
| | model-00002-of-00002.safetensors | 7b770216613ac5c34d7c54bdff1fa616bc4e338a9d0b20af6303e48c295ee23c |
| | model.safetensors.index.json | 0a57c30afc07c81610ae7fe013bea3b9d50365f9a8145ae98e8714220f3d7c7c |
| | config.json | eadff796b79b82cefa0537004668d70a1dd099fd4986b91ef88398ed91b26311 |
| | generation_config.json | 8bab06a398ca622f8b941eb0b5f425d71d007080436cfa4f37d6c03c23b756a9 |
| | tokenizer.json | 6b9e4e7fb171f92fd137b777cc2714bf87d11576700a1dcd7a399e7bbe39537b |
| | tokenizer_config.json | 9ddd255c19fe319c8d4e891163540382e9fbda99f394674f2a929efc47d57458 |

GPU engine: prebuilt `libomega_gpu.a` from omega 62b6a28 (sha256
ef80e431...), linked with `AIEN_OMEGA_GPU_LIB`.

## How the gates were run

The gate driver is a small scratch example kept out of the tree (it
hard-codes the three NEXT-PHASE-1 tasks). It follows the daemon path
exactly: `load_model_config` then `load_from_safetensors` (strict
catalog), the shared KV spine, `NativeTransformerBackend`,
`ChatTokenizer::from_model_dir`, one warm-up turn on `WARM_UP_TEXT`, then
for each task `proposal_prompt` -> `format_chat` + `"filename:"`, greedy,
96 tokens max, the model's stop set.

```
# CPU (reference backend)
AIEN_FORCE_CPU_STUB=1 cargo build --release --example llama3diag -p aien-runtime
llama3diag <model_dir> tasks 96 > l1b-cpu.json
# GPU (Omega backend), under the quiet lock
AIEN_OMEGA_GPU_LIB=<libomega_gpu.a> cargo build --release --example llama3diag -p aien-runtime
quietlock hold --owner np1-engine --minutes 20 -- env DIAG_BACKEND=omega llama3diag <model_dir> tasks 96
```

The HF reference is `transformers` greedy decoding in float32 on the
exact prompt ids the Rust side produced (so tokenizer and model are
checked separately), run from a scratch script outside the repo. It also
records the smallest top-1 vs top-2 logit gap along each reply.

## Gate A: tests and lint

- **PROVEN** `cargo clippy --workspace --all-targets -- -D warnings`
  clean (CPU stub build).
- **PROVEN** `cargo test --workspace -- --test-threads=1`: every test target and all doc tests pass
  (CPU stub build) except 3 tests. The 3 failures are machine checks in
  `spark-adapters`, which does not depend on any crate this cut touches:
  the branch name must start with `feat/` or `fix/` (this branch is
  `next-phase/...` by instruction), a Forgejo git remote must exist, and
  a live service on port 18095 answered 401 instead of 404.
- **PROVEN** new unit tests: TinyLlama `config.json` gives the built-in
  `tinyllama_1_1b()` config (apart from the explicit stop set `[2]`) and
  the same catalog as `tinyllama_catalog()`; the Llama 3.2 1B catalog
  has no `lm_head` and the tie flag set; llama3 `inv_freq` for
  head_dim 64 matches 32 values computed independently in f64 (and HF's
  own f32 function to about 1e-8); refusals name the bad field;
  the Llama 3 template renders the instruct layout; the Llama 3
  tokenizer gives exactly one BOS (128000) at the start.

## Gate B: CPU

**PROVEN** TinyLlama regression. Through the new `config.json` path
(Zephyr template, stop `[2]`) on the v4 attempt-1 prompt: the same
prompt ids, the same 48 output ids as v4, and the same warm-up token
(29956).

**PROVEN** Llama-3.2-1B, CPU reference, all three tasks token-for-token
equal to HF float32 greedy, all ending on `eos` (128009):

| task | prompt tokens | output tokens | stop | smallest logit gap |
|---|---|---|---|---|
| NOTES.md | 106 | 16 | eos | 0.349 |
| docs/HOURS.txt | 117 | 16 | eos | 2.918 |
| checklist.md | 113 | 18 | eos | 0.0058 |

Task 3 had a near tie (0.0058) and still matched.

Replies (prefix included):

```
filename: README.md
# Project Notes
This project keeps every change inside its workspace.

filename: docs/HOURS.txt
open 09:00
close 17:30

filename: checklist.md
lock: Locking system
key: Key management
lamp: Lighting system
```

The 1B model names the wrong file in task 1 (README.md) and decorates
task 3. That is the model's answer, matched by HF; it is not an engine
error.

**PROVEN** Llama-3.2-3B (two shard files, so this also exercises the
sharded loader), CPU reference, all three tasks token-for-token equal to
HF float32 greedy, all ending on `eos`. Weight load 33.4 s, warm-up
12.7 s (first id 2181).

| task | prompt tokens | output tokens | stop | smallest logit gap | prefill ms | decode tok/s |
|---|---|---|---|---|---|---|
| NOTES.md | 106 | 65 | eos | 0.131 | 11543 | 0.86 |
| docs/HOURS.txt | 117 | 16 | eos | 1.922 | 12785 | 0.82 |
| checklist.md | 113 | 41 | eos | 0.300 | 12498 | 0.85 |

The 3B model answers task 2 exactly. In task 1 it writes `docs/README.md`
and in task 3 it puts the full workspace path in the filename line. Both
are model answers that HF also gives.


## Gate C: GPU (Omega backend, no CUDA)

**PROVEN** Llama-3.2-1B on the Omega GB10 backend: all ops native
(`rmsnorm, apply_rope, matmul_vec, matmul_batch, swiglu, gqa_attention,
paged_attention, paged_attention_batch, compute_logits`), no fallbacks.
All three replies token-for-token equal to the CPU run (and so to HF),
all ending on `eos`.

| task | prefill ms | decode tok/s | total ms |
|---|---|---|---|
| NOTES.md | 2061 | 5.15 | 4975 |
| docs/HOURS.txt | 2192 | 5.16 | 5097 |
| checklist.md | 2214 | 5.21 | 5479 |

Warm-up turn 9866 ms (124 prompt tokens, first id 40). Weight load
11.7 s. Load average was about 7.5 during the run because the workspace
test suite was running at the same time, so these times are an upper
bound, not a benchmark.

**PROVEN, BLOCKED** Llama-3.2-3B on the Omega backend does not run. The
omega attention kernel accepts only `head_dim == 64` (stated in
`aien-omega-gpu`, and returned as `TOO_LARGE` on the first decode
step); Llama 3.2 3B uses `head_dim` 128. The strict real-model guard
stopped the run instead of falling back to CPU, which is the intended
behavior. The gap is in the omega engine (pinned 62b6a28), not in this
cut. Llama 3.2 1B (head_dim 64) and TinyLlama (64) are unaffected.

A first attempt failed earlier, at channel creation with "out of memory"
(`RM_ALLOC` status `NV_ERR_NO_MEMORY`), while the HF 3B reference run
was holding host memory. The retry passed that stage and hit the
head_dim limit above. **INFERRED** the first failure was memory
pressure from the concurrent run.


## finish_reason reaches the receipt

- **PROVEN** (compose-v5 test `finish_reason_is_recorded_per_attempt`,
  still passing here) the proposal attempt record carries
  `finish_reason` per attempt, and `finish_reason_label` maps the
  scheduler's reasons to `eos` / `max_tokens`.
- **INFERRED** (code path) daemon: scheduler `Finished { finish_reason }`
  -> `generate_text` sets `Generation.finish_reason` -> `model_proposer`
  keeps it -> `propose_with_retries` copies it into each
  `ProposalAttempt` -> `ComposeRun.proposal_attempts` in the run JSON ->
  `rows-v5.jq` reads `accepted_attempt.finish_reason` and
  `make-receipt.sh` prints it. Not exercised end to end through a live
  daemon in this cut.
- **PROVEN** the gate driver reports the same label from the same
  function: `eos` for all Llama replies above, `max_tokens` for the
  48-token TinyLlama regression.

## Notes and limits

- The Llama 3 template here is the plain instruct layout. HF's shipped
  Llama 3.2 template also inserts a system block with "Cutting Knowledge
  Date" and today's date. That makes the prompt depend on the wall clock,
  so it is left out on purpose. The HF comparison above uses the same
  prompt ids as the Rust side, so it is not affected.
- A TinyLlama daemon that now reads `config.json` stops on `[2]` only
  (the old backend set was `{0, 1, 2}`). The v4 regression output is
  unchanged. **INFERRED** ids 0 and 1 are never produced by TinyLlama
  greedy decoding in practice.
- The daemon's `model_sha256` hashes only the file it was pointed at; for
  a sharded model that is one shard. **UNVERIFIED** whether any receipt
  depends on it covering all shards.
- Only greedy decoding was checked. Sampling is not covered.
