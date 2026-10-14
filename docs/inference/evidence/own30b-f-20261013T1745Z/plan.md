# OWN-30B cut 3 (builder F): Qwen3-30B-A3B-Instruct-2507 Stage A on Omega, chip receipts

Frozen candidate: sc own30b/qwen3-30b-general (commit in each .start), checkpoint
~/models/qwen3-30b-a3b-instruct-2507-fp8, golden = CPU oracle tokens from `own30b_check golden`
(exact FP8 MoE oracle + f32 attention, greedy, 16 new tokens, 4 fixed prompts).

Runs (one variable each, under flock /tmp/aien-gb10.lock, announced with crumb whisper):
1. base-pinned: binary built against omega.lock 4d0bb369 (expert matmuls = whole-tensor fallback,
   slow). Prompts 1,2; parity on layers 0,23,47; routing mutant control. Establishes correctness
   of loader + attention + routing + sampling independent of A's window entry.
2. win: same sc commit, binary linked to A's archive (omega own30b/moe-window 25dd7c10,
   sha256 947f24b0...). Prompts 1,2,3,4; parity + both controls (routing mutant, window skew 1 tile);
   decode tok/s, load time, peak host RSS.
3. win-r2: repeat of 2 for timing stability (tokens must be identical).

Pass rules: layer parity cosine >= 0.999 and routing agrees on every traced position; controls
must drop cosine below 0.99; whole-model tokens identical to golden, or first divergence only at
a step whose CPU top-2 margin is below 1e-3 (recorded, not waved); peak RSS minus (resident + host)
within 6 GB.
