# OWN-30B cut 3 (builder F): Qwen3-30B-A3B-Instruct-2507 Stage A on Omega, findings

Checkpoint ~/models/qwen3-30b-a3b-instruct-2507-fp8 (FP8 experts, bf16 rest). Engine:
`aien_inference_abi::qwen3_moe_omega` (sc own30b/qwen3-30b-general; commit in each `.start`).
Golden: `own30b_check golden` on the CPU oracle (f32 attention, exact FP8 MoE oracle, greedy,
16 new tokens, 4 fixed prompts of 1 / 20 / 204 / 657 tokens), `golden.json` + `golden.log`.
Every chip run: `scripts/own30b-chip-run.sh` (flock /tmp/aien-gb10.lock, whisper, stamps,
raw output, rc, SHA256SUMS). Per run: `<name>.start` (commit, binary and golden digests, env,
command, free -g), `<name>.raw`, `<name>.err`, `<name>.rc`, `<name>.end`, `<name>/run.json`.

## Runs (one variable each)
| run | omega | spin us | prompts | parity (3 layers) | controls | tokens vs golden | decode |
|---|---|---|---|---|---|---|---|
| base-pinned | omega.lock 4d0bb369, expert matmuls = whole-tensor fallback | 0 | 1,2 | PASS | routing CAUGHT (skew n/a) | IDENTICAL | 0.6 tok/s |
| win | A's archive 25dd7c10 (window matmul), sha256 947f24b0... | 0 | 1,2,3,4 | PASS | routing CAUGHT, skew CAUGHT | IDENTICAL x4 | 7.5-7.7 tok/s |
| win-r2 | same | 0 | 1,2,3,4 | PASS | both CAUGHT | IDENTICAL x4 | 7.5-7.6 tok/s |
| win-spin2000 | same, sc 022d474 | 2000 (daemon default) | 1,2,3 | PASS | both CAUGHT | IDENTICAL x3 | 10.7-12.1 tok/s |

## Numbers (VERIFIED on the chip, from the .raw files)
- Load: 60-68 s for 60.42 GB resident bf16 + 0.67 GB host (embed, norms, routers); KV pool 768 MB
  resident at 8192 context. Peak host RSS during load 61.99 GB (resident mapping counts), 0.90 GB
  over resident + host; 63.3 GB at the end of a run.
- Layer parity (p2-short, 20 positions, chip traces replayed on the host): attention cosine(min)
  0.999986 / 0.999780 / 0.999997 and MoE cosine(min) 0.999998 / 0.999993 / 0.999994 on layers
  0 / 23 / 47; chip top-8 routing equals the oracle's at every traced position.
- Controls: routing mutant (expert id + 1) drops layer 0 MoE cosine to 0.3686; A's window-skew
  mutant (1 tile) drops it to -0.0212. The pinned build has no window API, so the skew control is
  not applicable there (reported as such, not as caught).
- Whole-model greedy: all 4 prompts identical to the CPU golden through 16 new tokens (p2 stops at
  7 with <|im_end|>, same as the CPU).
- Decode (window path, spin 0): 129-139 ms/token = 913 launches/token (48 x (3 attention + 16
  expert) + lm_head). Per step: attention 40 ms, experts 84-97 ms, lm_head 5.4 ms, host 7-12 ms.
  Spin 2000 us: 83-94 ms/token (attention 26-30 ms, experts 50-58 ms). The launch tax dominates:
  about 0.1 ms per expert launch at spin 0 and 0.065 ms at spin 2000 for 3 MB of weights each.
- Prefill is token-at-a-time (every prompt token is a full step): 134 ms/token at spin 0.
- Attention 40 ms/token = 48 layers x 0.83 ms = 3 launches per layer (fused qkv [2048][5120]
  matmul, paged bf16 attention over the resident KV pool, o [4096][2048] matmul) plus the host
  q/k head-norm, rope and KV write between them (the host part is inside attention_ns; the
  separately counted host_ns covers input norm, residual, post norm, routing, swiglu, weighted sum).
  At these shapes each launch moves 21 MB / tiny / 17 MB, so 0.28 ms per launch is overhead, not
  bandwidth.

## Limits
- One checkpoint, greedy only, 16 new tokens, 3 traced layers; no long-context parity beyond
  657 + 16 tokens; no batched prefill; no serving reservation/prepare/seal in these runs.
- A's archive is linked through the AIEN_OMEGA_GPU_LIB development override (not a qualified build).
