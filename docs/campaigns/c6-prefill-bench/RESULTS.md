# C6 comparative benchmark: AIEN shared prefill vs a MODELED vLLM-style prefix cache

Status: measured on CPU only. GB10 GPU: NOT_RUN. Real vLLM: NOT_RUN.

## What was measured
`cargo run --release -p aien-runtime --example prefill_bench` (code in
`crates/aien-runtime/examples/prefill_bench/`). One shared prompt prefix, N branches forked from it, 8 decode steps.

- AIEN leg: the real runtime path (spine, shared KV pool, fork hooks, TinyLlama CPU backend). Measured.
- Baseline leg: a MODELED vLLM automatic-prefix-caching run (`vllm_model.rs`). It uses vLLM's rule that only
  full KV blocks (16 tokens) are shared, so each extra branch recomputes the partial last block. The compute
  time is measured on our own CPU backend; the KV byte counts are a bookkeeping model, NOT measured.
  The JSON says so: `"baseline_label": "MODELED vLLM automatic prefix caching, not a vLLM run"`.
- Control: one unshared one-shot prefill, used to check numerical parity (logits bit-for-bit).

Model: TinyLlama-1.1B-Chat-v1.0, safetensors, CPU (Spark, 20 cores, Cortex-X925/A725).
Grid: prefix length 40, 200, 700 tokens; N = 1, 2, 4, 8, 16; 2 repetitions per cell (reps = 2, so N=2 samples per cell).

## Numbers (mean of 2 reps, seconds; source `evidence/ltc6-full.json`, run at f2a5a27)

| prefix | N | AIEN prefill | modeled APC prefill | AIEN total | modeled APC total | AIEN peak KV MiB | modeled APC KV MiB |
|---|---|---|---|---|---|---|---|
| 40 | 1 | 2.00 | 2.01 | 5.0 | 5.0 | 2.8 | 2.8 |
| 40 | 4 | 1.95 | 5.02 | 7.9 | 10.5 | 4.8 | 6.9 |
| 40 | 16 | 2.00 | 17.29 | 11.6 | 26.4 | 13.1 | 23.4 |
| 200 | 1 | 9.03 | 7.82 | 12.1 | 10.8 | 9.6 | 9.6 |
| 200 | 4 | 8.90 | 11.15 | 14.8 | 17.2 | 11.7 | 13.8 |
| 200 | 16 | 8.52 | 25.14 | 19.7 | 35.3 | 19.9 | 19.3 |
| 700 | 4 | 33.55 | 33.91 | 40.1 | 41.6 | 35.8 | 35.1 |
| 700 | 8 | 33.41 | 39.65 | 45.6 | 52.9 | 41.3 | 40.6 |
| 700 | 16 | 33.80 | 51.98 | 48.1 | 65.3 | 52.3 | 51.6 |

All 30 cells (all N) are in the JSON. Tokens prefilled by AIEN equal the prefix length for every N; the modeled
baseline prefills extra tokens per branch (for example 880 vs 700 at N=16).

## Reading
- AIEN prefill time stays flat as N grows (one shared prefill). The modeled baseline grows with N because it
  recomputes the partial last block per branch. At short prefixes (40 tokens, mostly partial block) the gap is large
  (N=16: about 8.6x on prefill, 2.3x on total). At 700 tokens it is smaller (N=16: 1.5x prefill, 1.4x total).
- At N=1 there is nothing to share. AIEN was about 15 percent slower on prefill than the modeled baseline at 200 and 700
  tokens (9.03 vs 7.82 s, 34.8 vs 30.2 s). Not investigated; with 2 reps it may be noise or runtime overhead.
- Peak KV memory: AIEN is lower at short prefixes, roughly equal at 200 to 700 tokens (the baseline model already shares
  full blocks), and slightly higher at some long cells (N=4 to 16 at 700: 35.8 vs 35.1 MiB, etc.). No memory win is claimed.
- Parity: every valid cell has 0 bit differences, AIEN vs the unshared control and AIEN vs the baseline.

## Invalid cells: 700 tokens, N=1 and N=2
In these four cells the AIEN run did not decode at all: decode time 0, no tokens, peak KV 0, parity compared 0 values,
and `aien_error` was null (silent). The diag run at head 1afc699 (`evidence/ltc6-diag.json`, N=2, len 700, 4 steps)
shows why it looks stuck: `spine_steps=1000 running=0 waiting=2 root_prefill_pending=false prefill_tokens=700
decode_steps=0`. The bench hit its 1000-step loop guard with both branches still waiting after the root prefill
finished. Cause not found. These cells must not be read as results. Whether this is a bench bug or a real
scheduler/admission bug in the runtime is OPEN.

## Runs and logs
| job | sha | what | log |
|---|---|---|---|
| LTC6-smoke2 | f2a5a27 | N=4, len 200, 4 steps | ~/workspace/test-queue-logs/LTC6-smoke2-light-233731.log |
| LTC6-full | f2a5a27 | full grid, reps 2 | ~/workspace/test-queue-logs/LTC6-full-light-234215.log |
| LTC6-diag | 1afc699 | N=2, len 700, 4 steps (diagnostic) | ~/workspace/test-queue-logs/LTC6-diag-light-004504.log |

The only code change between f2a5a27 and 1afc699 is an added diagnostic string (`aien_exit_state`) in the output; the
full grid was not rerun at 1afc699.

## NOT claimed
- No real vLLM was run. The baseline is a model. Real vLLM on a GPU may differ in both time and memory.
- No GPU (GB10) result. CPU, one model (TinyLlama 1.1B), one machine, 2 reps per cell, no confidence intervals.
- No energy measurement. No memory measurement of the baseline (bookkeeping only).
- Machine load was not controlled (loadavg 8.8 at start of the full run).
- Nothing about the 700-token N=1/N=2 cells beyond "invalid, cause open".
