# T2: Is persistent KV park/restore wired on the real omega engine path?

Ticket: aien-spark/aien-sovereign-core#496 (map #494). Written 2026-10-13.
Tags: VERIFIED = read in source, cited file:line. INFERRED = reasoned, from what is stated. UNKNOWN = not established.

## Short answer

- No. On the real engine path (`aien-serve-http --engine omega`) park/restore is not wired at all. VERIFIED, see section 2.
- The persistent KV code (PRs #487-#493) is wired only into a different program, the `aien` daemon built on the runtime spine and a different model path. It was never connected to the omega engine. VERIFIED, section 3.
- The identity fields that exist would not make a restore safe on the omega path today. They cover the checkpoint files and the model shape. They do not cover the FP8 weight format choice, the omega build, or the omega numeric rounding. The one guard for numbers (`KV_ARITH_EPOCH`) is a hand-bumped counter whose test only exercises the reference CPU backend. VERIFIED, section 4.
- Extra finding: `aien-serve-http` and the omega engine (`qwen3_moe_omega.rs`) are not on GitHub main and not on any GitHub branch I could list. They exist only on a local, unpushed branch `own30b/serve`. VERIFIED, section 1.

## 1. Where the code lives (sources read)

- main: `b9b52f9` of aien-spark/aien-sovereign-core (fresh clone, `/tmp/research-pkv-omega`). `crates/aien-serve-http` does not exist; `git ls-tree` of origin/main shows no `qwen3_moe_omega.rs`. VERIFIED.
- GitHub search for `aien-serve-http` in aien-spark/aien-sovereign-core, aien-spark/omega, aien-spark/aienos, aien-spark/aien-local-stack returned nothing; branch lists of aien-spark and aien-dev/aien-sovereign-core contain no `own30b/*` branch. VERIFIED (the code-search API can be incomplete, so "not on GitHub" is VERIFIED for branch names and main tree, INFERRED for everything else).
- The omega serve code was read from the local branch `own30b/serve` at commit `74aeb467480f42dc58d344a7fbe29197c656fa88` (read by cloning the local repo `~/workspace/sc-own30b` into `/tmp/pkv-own30b`; the original was not modified). This branch is 34 commits past main's `f560be4`. VERIFIED. Citations below marked `[own30b]` refer to that commit.
- PR stack (all OPEN, each based on the previous): #487 (fix/preempt-resume-reemit, base main), #488 `pkv/484-persistent-kv` (base main), #489 `pkv/c4-host-fs`, #490 `pkv/pr3-worker-review`, #491 `pkv/pr4-runtime`, #492 `pkv/pr5-restart`, #493 `pkv/pr6-concurrency-bench` (tip). Cited as `[pkv6]` = branch `pkv/pr6-concurrency-bench`. VERIFIED via `gh pr view`. Not merged, so none of it is "current merged behavior".
- ADR 0003 already knows about the omega branch and says the overlap is only "in-memory prefix reuse"; "that branch has no disk, no identity, no sharing across requests" (`docs/adr/0003-persistent-kv-identity-storage-contract-and-ownership.md:50` [pkv6]). VERIFIED.

## 2. Call path on the omega engine, and where park/restore is absent

1. `aien-serve-http` main builds the engine: `--engine omega` calls `OmegaDoor::load(model_dir, max_context (default 32768), weights, spin_us)` (`crates/aien-serve-http/src/main.rs:168-181` [own30b]). VERIFIED.
2. `OmegaDoor` holds only an `OmegaEngine` and a lock wait (`omega_engine.rs:73-76` [own30b]). It loads weights via `OmegaEngine::load_with(model_dir, max_context, format, ...)` (`omega_engine.rs:98`). VERIFIED.
3. Every request goes through `Engine::generate` (trait at `engine.rs:83-96`; omega impl `omega_engine.rs:139-189`), which takes the GPU lock (`:170`) and calls `OmegaEngine::generate_with` (`:174`). VERIFIED.
4. The engine keeps one sequence's KV and a token list `history` (`qwen3_moe_omega.rs:2125` [own30b]). Reuse is in memory only: longest common prefix with `history`, then `rewind` (`qwen3_moe_omega.rs:2809-2818`, `rewind` at `:2570-2575`). A different conversation overwrites it. VERIFIED.
5. The KV is `OmegaPagedKv`: one sequence, bf16 pool, block size 16, allocated by omega as a resident GPU-visible block when the driver grants it (`qwen3_moe_omega.rs:1844`, `:1856-1866`, `:1876-1890`). The trait it implements has only `capacity/reset/truncate/append/attend/resident` (`:1754-1768`): no export, no import, no park, no restore. VERIFIED.
6. Search: a grep over `crates/aien-serve-http` for `kv_store|KvStore|park|restore|residency|persistent|AIEN_KV|aien_runtime` finds nothing; `aien-serve-http/Cargo.toml` depends only on `aien-inference-abi` and `aien-omega-gpu` (no `aien-runtime`, no `aien-kv-store`). VERIFIED.
7. Also `aien-inference-abi` on `own30b/serve` does not depend on `aien-kv-store` (its Cargo.toml lists abi-core, kv-cache, platform, omega crates only). VERIFIED.

Conclusion: nothing on the omega serve path calls park or restore. VERIFIED. Even a restart of the server loses the prefix cache. INFERRED from 4-6.

## 3. Where park/restore IS wired (not the omega engine)

- Only the `aien` daemon: `crates/aien-cli/src/commands.rs:1816-1880` [pkv6]. With `AIEN_KV_STORE_DIR` set it builds `ModelKvBinding { model, arith_id, model_sha256, model_digest_kind }` and calls `shared_kv::enable_persistent_kv` (`crates/aien-runtime/src/shared_kv.rs:668-720`), which installs the residency layer in the runtime spine. VERIFIED.
- The backend side of park/restore is a seam on the `InferenceBackend`-style trait in `aien-abi-core/src/lib.rs:644-661` [pkv6]: `computed_prefix` (default `None`: "nothing is ever parked from this backend") and `restore_sequence` (default `Err`: caller falls back to plain prefill). VERIFIED.
- Only `NativeTransformerBackend` (`transformer_backend.rs`) overrides these seam methods (its KV lives in the runtime's shared pool). `OmegaGb10Backend` (`omega_backend.rs`) has no override: grep for `computed_prefix|restore_sequence` in it returns nothing. VERIFIED for omega_backend.rs; INFERRED that NativeTransformerBackend is the only implementer (I checked the files in the PR list, not every crate).
- The Qwen3-30B-A3B omega engine is not a `TensorBackend`/seam implementer at all: it is a separate struct `Qwen3MoeOmegaEngine` with its own forward pass (`qwen3_moe_omega.rs:2100`). VERIFIED.
- The daemon's model config cannot describe this model: `ModelConfig` has dense fields only (`aien-abi-core/src/lib.rs:123-159`: layers, heads, hidden, intermediate, vocab, rope, qk_norm; no expert count, top-k, or MoE intermediate size). VERIFIED. INFERRED: so even if the daemon were pointed at Qwen3-30B-A3B, the identity would not carry the mixture-of-experts shape. UNKNOWN whether the daemon can load that checkpoint at all (not tested; running it is out of scope).
- The benchmark (#493) uses a synthetic CPU model, so it proves the store and runtime logic, not the GPU engine. (From the ticket and `crates/aien-kv-bench`; I did not re-run it.) INFERRED.

## 4. What identity exists today

Namespace id = SHA-256 over 14 fields (`crates/aien-kv-store/src/identity.rs:234-283` [pkv6], domain `AIEN-PKV-NS-V1`). VERIFIED. Fields:

1. record format version; 2. `model_sha256` (32 bytes); 3. `model_digest_kind` (`file` or `index+shards`); 4. `model_id` string; 5. num_layers, num_kv_heads, head_dim, block_size; 6. rope_theta; 7. rope scaling (Llama-3 style, optional); 8. qk_norm flag; 9. KV dtype (fp4/fp8/bf16/fp16/fp32); 10. six pool layout strides; 11. backend format (`HostPagedPoolV1` or `NativeF32DecodeStateV1`); 12. chunk token count; 13. sharing namespace string (operator setting); 14. a compute-config digest.

The compute-config digest (`identity.rs` `ComputeConfig::digest`, domain `AIEN-PKV-CFG-V1`) covers: num_heads, hidden_dim, intermediate_dim, vocab_size, rms_norm_eps, tie_word_embeddings, hidden_act (silu only), `arith_id` (the backend's name string), `arith_epoch` (`KV_ARITH_EPOCH`). VERIFIED. Built in `aien-runtime/src/compute_config.rs:64-104` with an exhaustive destructuring of `ModelConfig` so a new field cannot be forgotten silently (comment at `:66`). VERIFIED. Chunk keys chain from the namespace id and the token prefix (`identity.rs`, ADR section 1). VERIFIED.

### Coverage against your four questions

| Must be bound | Covered today? | Evidence |
|---|---|---|
| Model weights | YES, for the checkpoint files on disk | `model_sha256` is a streamed hash of the safetensors file, or a manifest hash over the shard index and every shard (`aien-runtime/src/generation.rs:17-26`, kinds at `aien-cli/src/commands.rs:1203,1252` [pkv6]). Startup refuses persistence if the digest is unknown (`commands.rs:1846-1849`). VERIFIED |
| Weights as actually loaded (FP8 vs bf16 format) | NO | Omega can load the same directory three ways: `Bf16` (all dequantized), `Fp8Experts`, `Fp8All` (`qwen3_moe_omega.rs:469-477` [own30b]; flag `--weights bf16\|fp8-experts\|fp8`, `main.rs:3`). None of this reaches the digest; the file hash is the same. VERIFIED that the format is absent from every identity field listed above. INFERRED that the three give different K/V numbers (different arithmetic by design; no test seen that compares them for equality). |
| Engine / omega build | NO | `arith_id` for omega is only the string "OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)" or "(stub, no chip)" (`omega_backend.rs:815-820` [pkv6]) and that class is not the engine serve-http runs. No git commit, `omega.lock` hash, kernel build id or driver version is hashed. VERIFIED by reading `ComputeConfig::digest` and `NamespaceFields`. The ADR itself states `arith_id` "is a coarse label" (`ADR 0003:113`). VERIFIED |
| FP8 rounding class | NO (only an indirect, manual proxy) | `KV_ARITH_EPOCH = 1` (`aien-inference-abi/src/lib.rs:30-36` [pkv6]) is a counter a human must bump after a numeric change. The pinning test `arith_epoch_pinned_bytes` runs "the fixed prompt on the reference backend in a bf16 pool" (`aien-runtime/tests/compute_config_binding_test.rs:306-322, 374-386`), so it would not notice an omega or FP8 kernel change. VERIFIED. The ADR names this as residual risk: "a numeric change the pinned test does not exercise could slip through (UNKNOWN residual risk; the test model should cover every op the production path uses)" (`ADR 0003:520`). VERIFIED |
| Config | PARTLY | Dense-model shape and norm eps are covered. MoE shape (experts, top-k, expert size), and omega run-time knobs that change numbers if they do (prefill GEMM vs GEMV, device vs host routing, attention path; `omega_engine.rs:105-125` [own30b]) are not covered. VERIFIED absent. UNKNOWN whether those knobs change the K/V values (the code names them as speed paths; a test that compares them bit for bit exists for the GEMM prefill per commit `d37f006`, "GEMM bit-identical to GEMV", per git log; I did not read the test). |
| KV layout and dtype | YES | dtype + six strides + backend format + chunk size are in the namespace. Omega's KV is the same `aien_kv_cache` pool type with bf16 (`qwen3_moe_omega.rs:1856-1876`), so INFERRED the layout can be described by `HostPagedPoolV1`. UNKNOWN whether omega's resident device block has the same bytes layout the restore code assumes. |

So: model files and shape, yes; the exact weight format, omega build, FP8 rounding and omega run settings, no. A cache written by one omega setup could be offered to another setup with the same files. The identity layer would not stop it. INFERRED from the table.

## 5. What is missing to wire it on the omega path (a list, not a design)

1. A KV export/import on `OmegaPagedKv` (today: `append/attend/truncate/reset` only, `qwen3_moe_omega.rs:1754-1768`).
2. A park hook after each generation and a restore step at the start of `Engine::generate`, in or around `OmegaDoor` (`omega_engine.rs:148-189`). Today the only seam is the runtime-spine one in `aien-abi-core` (`:644-661`), which this engine does not implement.
3. A store handle and settings in `aien-serve-http` (no `aien-kv-store` or `aien-runtime` dependency today).
4. An identity builder for the omega path: weights digest of the directory plus the weight format; omega build id (`omega.lock` commit or the linked kernel build); a rounding class that the omega path, not the reference CPU, is pinned against; MoE shape; any numeric run settings. None exist.
5. A pinned-bytes check on real omega output (or on the closest runnable thing), since the current pin cannot see omega changes.
6. A decision for the multi-turn case: the last prompt token must be recomputed (ADR section 6.5, `ADR 0003:50`) which the omega engine already does (`min(prompt.len() - 1)`, `:2811-2812`). That part is compatible. VERIFIED.
7. The code to be pushed: the omega server is not on GitHub, so any wiring work cannot be reviewed there until it is.

## 6. Unknowns left open

- Whether the live server on :18091 runs commit `74aeb467` or something else. I did not touch it, per the rules. Its code is thus INFERRED to match the local branch.
- Whether the three weight formats or the speed knobs change K/V bytes beyond rounding noise (needs a GPU run; out of scope).
- Whether the daemon could serve Qwen3-30B-A3B at all.
- Whether PR #493's synthetic benchmark numbers transfer to the GPU (no data seen on that).
