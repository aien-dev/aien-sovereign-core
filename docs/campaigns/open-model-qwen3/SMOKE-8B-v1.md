# SMOKE-8B-v1: Qwen3-8B correctness smoke (preregistered)

Status: PREREGISTERED before any 8B token was produced by this engine (2026-10-11). Lane: sc#384, authorized by Drake (sc#384 comment 6110013055). A smoke test, not a qualification and not a promotion of the 8B to a product model. Qualification of the 8B stays a separate, later decision.

## Why this exists

The 8B differs from the qualified 4B in ways the engine had not been shown to handle: a pinned chat template with a thinking switch (now pinned, thinking off, `ChatTemplate::ChatMlQwen3NoThink`) and an untied output head (`tie_word_embeddings=false`, separate `lm_head.weight`; CPU loader path covered by `crates/aien-inference-abi/tests/qwen3_untied_head.rs`). This smoke shows, once, that the whole path produces the same tokens on the CPU reference and on the GB10.

## Fixed inputs

- Model: `~/models/qwen3-8b`, Hugging Face `Qwen/Qwen3-8B` revision b968826d9c46dd6066d109eabc6255188de91218. Before any run, the script checks sha256 of `config.json` and the five shards against the values recorded in `EVAL-qwen3-8b.md` Section 1 (branch d1/qwen3-8b-eval, or main once sc#434 merges): config f7c4eadf..., shards 31d6a825..., 5991236c..., c5185c47..., b5ee7de7..., 20c2d636... A mismatch stops the run.
- Conversation: system `You are a careful assistant.`, user `Reply with the single word OK.`, thinking off. The rendered prompt must equal, byte for byte,
  `<|im_start|>system\nYou are a careful assistant.<|im_end|>\n<|im_start|>user\nReply with the single word OK.<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n`
  (the driver exits with code 4 otherwise).
- Decoding: greedy (temperature 0), 16 new tokens at most, stop on the model's stop set (`<|im_end|>` among them), one warm-up turn first, as the daemon does. Driver: `crates/aien-runtime/examples/qwen3_8b_smoke.rs`; runner: `scripts/smoke_qwen3_8b.sh`.

## Steps and verdicts (all decided now)

1. Template (already unit-tested, no model needed): the engine's rendering equals the hand oracle, the jinja2 3.1.6 rendering (`render_oracle.json`) and the publisher's file byte for byte. Test: `tests/qwen3_8b_chat_template.rs`.
2. Untied head, CPU: `tests/qwen3_untied_head.rs` (synthetic checkpoint with a row-swapped `lm_head.weight` against the transformers oracle logits of the tiny Qwen3; an untied config without `lm_head.weight` is refused).
3. CPU leg (`smoke_qwen3_8b.sh cpu`): `ReferenceCpuBackend`, host f32 weights. Its `prompt_ids` and `output_ids` are recorded as the golden values in `SMOKE-8B-v1.golden.json`. The CPU leg is the reference by construction; it is judged only for being well formed (non-empty output, fallback_count 0) and for the reply being sensible text, which is reported, not gated (a 16-token greedy reply of an 8B on one prompt is weak evidence of quality and is not claimed as such).
4. GB10 leg (`smoke_qwen3_8b.sh gb10`): the daemon's GB10 construction (session open, resident bf16 load, `OmegaGb10Backend`, production-strict build, `AIEN_REQUIRE_BLACKWELL=1`). PASS if and only if all of:
   - `prompt_ids` equal the golden `prompt_ids`;
   - `output_ids` equal the golden `output_ids` token for token (parity);
   - `fallback_count` is 0.
   Any other outcome is FAIL. A near-tie that flips a token is a FAIL and is reported with the logit margin; no tolerance is raised to accept it. A refusal by name (engine not linked, model refused, resident load or session failure) is a FAIL with the reason, never a silent CPU result.
5. Recorded, not gated: wall time per leg, `VmHWM`, decode ms per token.

## What the chip run needs (not run by this lane)

- The machine free: the whole-system runs RUN-7-CPU and RUN-7-GB10 finished, `quietlock check` clear, no `whole_system_e2e` process (the script checks both).
- About 16.4 GB of device-resident weights plus serving buffers on the GB10; the host must not hold a second f32 copy (the resident load avoids it). The 4B needed the same care (omega#327: NV_ERR_NO_MEMORY when MemFree was low).
- The golden file from the CPU leg committed or present at `docs/campaigns/open-model-qwen3/SMOKE-8B-v1.golden.json`.
- The pinned GPU engine build through `scripts/pins.sh --gpu` (done by the script).
- Known limit, UNVERIFIED (confidence 60 percent that it works first time): the GB10 mode of the driver mirrors the daemon's construction by reading `crates/aien-cli/src/commands.rs` (`build_native_daemon_backend`) but has only been compiled, not run on the chip. If it fails on setup (for example a serving-buffer reservation the daemon does and the driver does not), the failure is a driver gap, to be fixed and re-run, not a model verdict.

## Limits

- One prompt, 16 tokens: parity on this prompt, not correctness in general.
- No Hugging Face reference token ids were produced for the 8B; the golden values come from this engine's CPU leg, whose correctness rests on the 4B/tiny-model oracle tests of the same code. EVAL Section 8 step 3 (a Hugging Face reference run, off-repo) remains open and is a stronger check; UNVERIFIED whether the CPU leg equals it.

## Recorded after the CPU leg ran (2026-10-11, the only leg run so far)

CPU leg on `~/models/qwen3-8b` (sha256 check passed): rendered prompt equal to the golden text, 31 prompt tokens, `ReferenceCpuBackend`, fallback_count 0, wall time 154 s for the whole script (load plus warm-up plus turn; the turn itself 19.6 s). Reply `OK`, output ids `[3925]`, then the stop token (finish reason `eos`; stop tokens are not part of `output_ids`). Golden values: `SMOKE-8B-v1.golden.json`. This does not change any verdict rule above. Consequence for the strength of the check: the golden holds ONE generated token, so GB10 parity on it is a weak test of the decode path; a longer-output prompt would be a stronger smoke and is a follow-up if Drake wants it (changing the prompt means re-preregistering). The GB10 leg has not been run.
