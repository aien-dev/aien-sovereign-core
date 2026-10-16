# BitNet T2: our CPU reference forward and its sealed golden, host only, 2026-10-15

Design: `~/handoffs/2026-10-15-ternary-engine-design.md` section 5 row T2 and its binding
amendment (perplexity gate). Code: `crates/aien-inference-abi/src/bitnet_forward.rs`
(`HostBitNetForward`), `src/bitnet_tokenizer.rs` (our own byte-level BPE), binary
`src/bin/own_tern_check.rs`. No chip run; no other engine or tokenizer was run.

Run: `own_tern_check golden ~/models/BitNet-b1.58-2B-4T-bf16 passage-faraday-candle-lecture1.txt
~/workspace/bitnet-t2-golden-20261015` (4 min 37 s wall, peak RSS 5.8 GB, `run.log`; run twice, dumps, losses and ids bit-identical).
Layout id `omega-trit2-rm16-hfrows-v1`; model and tokenizer sha256 pinned and checked.

## Passage

Faraday, "The Chemical History of a Candle", Lecture I (Project Gutenberg #14474, public domain),
paragraphs reflowed, footnote markers and underscores removed. sha256
`4b59948c0c829f7d47c8e8302ebb9400e0de86db67695d5fe4d017137f00343a`, committed (538b7730) before
any perplexity was measured. 2,654 tokens after BOS; it round-trips through our tokenizer.

## Gates

| gate | result | receipt |
|---|---|---|
| tokenizer: the spec's five acceptance id lists, exact | pass | `tests/bitnet_tokenizer_real.rs` |
| teacher-forced perplexity under 20 | mean loss 2.84966711 nats (calc:1aaad5a0); perplexity 17.282028, below the upper bound 17.28259628 < 20 (calc:ba7ad8ca) | `golden.json`, `passage-nll-reference.f64` |
| wrong norm order (attn sub-norm after o) raises perplexity by at least 10 % | loss +5.79774559 nats per token (calc:768251f5): perplexity 5,695.39 | `passage-nll-sub-norm-after-o.f64` |
| skipped sub-norms raise it by at least 10 % | +12.34408089 (calc:bfe40a3e): 3,967,901.8 | `passage-nll-skip-sub-norms.f64` |
| q and v scales swapped raise it by at least 10 % | +0.350252295 (calc:9a6a4db6): 24.530553 | `passage-nll-swap-q-v-scales.f64` |
| five own prompts, greedy 32 tokens, coherent | all five read as fluent, on-topic English (`golden.json`, `run.log`) | |
| decode speed | 64.719 ms per token median over 310 steps: 0.0768 of 237.2 GB/s / 1,178,568,100 B (16-bit table, calc:f15f961e); 0.0554 of the 8-bit-table figure 850,230,180 B (calc:b75f2a07). Both are against the model's minimum bytes per token; this host code actually streams 3,397,386,240 B per token (one byte per trit and the f32 table, calc:02eb83dc) | `golden.json` |

Each control's loss rise is above 0.1 nats, so its perplexity ratio is above
e^0.1 >= 1 + 0.1 + 0.1^2/2 = 1.105 (calc:f864e259). The perplexity bound is the Taylor sum of
e^2.8497 to 20 terms plus its tail bound; 2.8497 is above the measured mean loss.

Why q and v: swapping only the q and k scales changes nothing (the scores scale by s_q * s_k
either way; the unit test shows the output within 1e-4), and a v scale is undone by
`attn_sub_norm`. Swapping q and v scales every score by s_v / s_q.

Microsoft's five acceptance prompts, for information only (not a gate): Paris, Berlin,
"6, 7, 8, 9, 10", 100 degrees, Jupiter.

## The golden

`golden.json` seals: the passage sha256 and its token ids' sha256, the perplexity receipt of the
reference and the three controls (sum and mean loss to 17 digits, per-token loss files with
sha256), every prompt's ids and greedy ids, and the sha256 of every dump. Dumps (36 MB, f32
little-endian `[tokens][2560]`) stay outside git in `~/workspace/bitnet-t2-golden-20261015/dumps`,
listed in `DUMPS.SHA256SUMS`: the residual stream after layers 0, 1, 15 and 29, the final-norm
output and the last position's logits, for the five own prompts, the five spec prompts and a
512-token prompt (BOS and the passage's first 511 tokens).

## Arithmetic of the reference (spec 1.2, 1.3, 2.2, 4)

f32; exact int32 ternary products; per token int8 absmax with round half to even; one weight
scale per q, k, v, gate and up segment; RMSNorm square sums in f64; half-split RoPE with f64
angles; BOS 128000 at position 0; tied bf16 table widened to f32 for the logits. The reference
runs in bf16; ours is the more exact f32 oracle.

## UNVERIFIED items of spec section 7, how each was settled

1. RMSNorm precision, RoPE pairing and origin, softmax scale: by reading. HF
   `modeling_bitnet.py:53-58` (norm in f32), `:83-85` (rotate_half: j with j+64), `:299`
   (inv_freq), `:314` (cos/sin duplicated), `:161` (scale head_dim^-0.5); `REF/convert_safetensors.py:43-47`
   (the reference permutes q and k rows for adjacent pairs, so half-split is right for the
   bf16 master); `REF/model.py:291-295` (positions from 0). Unit tests pin the half-split pairing,
   the frequencies, the softmax scale and the GQA head map (against a naive loop); the f32 norm
   precision rests on reading plus the end-to-end perplexity gate. In exact arithmetic the position
   origin does not change attention (RoPE scores depend only on the distance between positions).
2. bf16 mean of the weight scales: settled by T1 (all 21 spec scales equal).
3. GGUF I2_S internals: settled by T1 (block 128, checkpoint row order, 97.25 % agreement).
4. Decode-kernel qkv mis-scaling effect: not measured, by Drake's binding change; we use one
   scale per segment, never `bitnet_kernels.h:80`.
5. Oracle path: this host reference is the oracle (prefill path maths, `REF/generate.py:56,63,70`).

CALC-EXEMPT: perplexities, timings, token counts and RSS are measured values copied from
`golden.json` and `run.log`.
