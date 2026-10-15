# ADR 0004: One generation controller, many forward passes: the seam between them

- **Status**: Accepted (2026-10-15). The objective, the order, the acceptance criteria and the rule that this interface is fixed before any extraction begins were decided by Drake on 2026-10-15 and are binding. The interface itself was designed three ways in parallel (minimal, GPU path first, test surface first), compared, and merged by the design owner (Fable 5.1); the merged result is what this ADR records.
- **Date**: 2026-10-15
- **Author**: Drake Stapleton <aien@aienos.com> (design drafted by the seam design owner, Fable 5.1)
- **Source read at**: aien-sovereign-core `origin/main` 4c83e389 (OWN-8B stage 1b merged). Every file:line below is from that commit. The 8B plan is `~/handoffs/2026-10-15-own8b-design.md` (rev 2026-10-15 FP8). The architecture review that found the duplication is the 2026-10-15 inference-stack review (candidates 1 and 2).
- **Evidence labels**: VERIFIED (read in source at the cited line), INFERRED (follows from source), PLANNED (a test or step designed here with no receipt yet). Nothing in this ADR was run on hardware. Every test named below is PLANNED until its step merges green.
- **Scope**: design only. CALC-EXEMPT: the remaining bare numbers are counts read from source (methods, fields, layers, heads, line numbers) or figures quoted from receipts, not arithmetic. This ADR changes no code. It fixes the interface that the migration steps in section 7 implement, one step per pull request.

## In plain words

Our GPU engine for the 30B model does two different jobs in one struct. One job is the maths: run a token through 48 layers of attention and experts on the chip. The other job is bookkeeping: remember the conversation, reuse the part already computed, save and restore the working memory, loop word by word, pick the next word, stop at the right time, stream the words out. The 8B model needs different maths (36 layers, 8 KV heads, dense feed-forward, FP8 matrix-vector kernels) but exactly the same bookkeeping. The plan as written would copy the bookkeeping. Drake decided instead: one bookkeeping module (the generation controller), written once, and one small, fixed interface through which it drives any model's maths (a forward pass). This ADR is that interface. The 30B keeps every kernel, every launch order and every byte it writes today; only the loop around it moves. The 8B then adds a forward pass, not an engine.

## Drake's decision (2026-10-15, binding, verbatim)

- Architectural objective: one generation runtime that supports multiple model implementations, not two nearly identical inference engines.
- Order:
  1. Extract model-independent KV geometry: memory layout, allocation, export and restore rules, without depending on Qwen3MoeConfig.
  2. Extract the shared generation controller: prompt handling, prefill scheduling, token generation, prefix reuse, rewind, persistence, streaming, termination. Implement it once.
  3. Keep the model mathematics specialized. The 30B MoE and the 8B dense engines keep their own forward passes, weight layouts and optimized GPU kernels. Add no abstraction to the performance-critical GPU path just to remove duplication.
  4. Connect the 8B to the shared controller.
  5. Unify the HTTP entry point and the correctness checks, only after both engines work.
  6. (Separate) the spine.rs compose-bridge lift.
- Non-negotiable acceptance criteria:
  - The 30B keeps identical deterministic outputs, correct KV restore, its existing generation behaviour, and no material performance regression.
  - The 8B produces correct tokens through its own forward pass and the common controller.
  - Every migration is independently testable and reversible. The working 30B engine is never replaced in one large change.
- The interface between the shared generation controller and each model's forward-pass implementation is established BEFORE any extraction begins.

Added by the orchestrator on 2026-10-15 after exp445 stage 1 (Omegese 23/60 against text 34/60 without masking, McNemar p = 0.013; 11 of 21 refusals were type mismatches): the planned fix is decode-time constrained decoding, a per-step legal-token mask from `omegese next --machine` (omega `docs/one_layer/OMEGESE.md` section 6.2). That mask is model-independent and belongs in the controller. The seam must let a constraint be applied between the model's logits and the sampler, let a forward pass optionally compute logits only for a row subset (the "sparse output head", aien-architecture `docs/research/omegese-and-gb10-decode.md` recommendation 5) at no cost when no constraint is active, and let the host fake test constrained generation end to end. No constraint engine is implemented by this ADR.

## Context (what exists, VERIFIED)

1. `Qwen3MoeOmegaEngine` (`crates/aien-inference-abi/src/qwen3_moe_omega.rs:3217-3251`) holds in one struct: `dgraph` (the one-list decode graph, declared first so it drops before the cache whose chip handle it holds), `weights`, `kv: Box<dyn KvAttention>`, `trace_layers`, `traces`, `routing_mutant`, `device_routing`, `last_timing`, `prefill_chunk`, `prefill_gemm`, `prefill_device_routing`, `pos`, `history`, `whole_prompt`.
2. Bookkeeping that reads only `kv`, `history` and `pos`: `max_context_tokens`, `position`, `reset`, `history`, `rewind`, `reusable_prefix` (longest common prefix capped at `prompt.len() - 1`), `export_kv`, `restore_kv` (reset, import, set history; reset again on error), `kv_switches` (`:3805-3940`); `generate_with`, `generate_inner`, `generate` (`:4196-4320`). `generate_inner` rewinds or resets, runs `whole_prompt_prefill`, then `step_batch` in chunks of `prefill_chunk`, then loops: take the graph's greedy token when the sampler is the argmax, else read logits and call the sampler; stop ids consumed, never emitted; `on_token` may cancel; `decode_step(next)`.
3. Model maths, MoE-typed: `step` (`:3941`), `step_batch` (`:3295`), `step_batch_gemm` (`:3521`), `logits` (`:4173`), `whole_prompt_prefill` (`qwen3_moe_omega/whole_prompt.rs:116`, consumes all or none of the tokens), `decode_step` (`qwen3_moe_omega_dgraph.rs:254-327`: one command list including the lm_head and the chip argmax, commits the KV, pushes history, returns an empty hidden vector and leaves a pending token), `dgraph_take_greedy`, `dgraph_take_logits` (reads the 151936 logits back only when a non-greedy sampler needs them), `dgraph_clear` (`:331-365`). The five sites that advance `history` and `pos` sit inside the maths (`:3506`, `:3716`, `:4165`, `whole_prompt.rs:150`, `dgraph.rs:313`).
4. `trait KvAttention` (`:2500-2570`): `capacity`, `reset`, `truncate`, `append`, `attend`, `resident`, `fused_attend`, `path_name`, `fused_kv`, `dgraph_cache`, `dgraph_commit`, `export_blocks`, `import_blocks`. Four adapters: `HostPoolKv` (`:2670`), `HostKv` (`:2791`), `OmegaPagedKv` (`:2877`), `OmegaFusedKv` (`qwen3_moe_omega_dattn.rs:15-70`, fixed by the kernel to 32 q heads, 4 KV heads, 128 wide; uploads every layer's q_norm and k_norm). Every constructor takes `&Qwen3MoeConfig` and reads only `layers`, `q_heads`, `kv_heads`, `head_dim`, `rope_theta`. `PoolBlockGeometry::for_config(&Qwen3MoeConfig)` (`qwen3_moe_omega_kvio.rs:28-45`) reads `layers`, `kv_heads`, `head_dim`, `KV_BLOCK_SIZE`. The payload is `HostPagedPoolV1` (ADR 0003 section 2.2).
5. The HTTP door `OmegaDoor` (`crates/aien-serve-http/src/omega_engine.rs:142-420`) holds `OmegaEngine` by concrete type and drives it through `max_context_tokens`, `reusable_prefix`, `restore_kv`, `generate_with`, the persistence `snapshot(&mut Qwen3MoeOmegaEngine)` (`kv_persist.rs:1101`) and `kv_switches`. The door always passes its own sampler, `SamplingObserver`, so every decode step reads the full logits back from the chip (`generate_with` sets `argmax_sampler = false`, `:4205`); the chip-argmax shortcut is taken only by `generate` (`:4303-4320`), which is what `own30b_check` runs. `IdentityInputs` (`kv_persist.rs:197-310`) destructures `Qwen3MoeConfig` and `OmegaKvSwitches` exhaustively, with no `..`, so a new switch cannot be forgotten.
6. The 8B side (`qwen3_dense_omega.rs`): `Qwen3DenseConfig` (`:172`; 36 layers, hidden 4096, 32 q heads, 8 KV heads, head_dim 128, intermediate 12288, vocab 151936, rope_theta 1e6), `DenseGemv` (`:465`), `DenseUploader` (`:474`), `DenseLayer` (`:650`), `Qwen3DenseOmegaWeights` (`:663`), `HostFp8Gemv` (`:542`). No engine exists. The plan's stage 2 decode is an FP8 GEMV per projection plus split attention on `OmegaPagedKv` at 32/8; prefill is the FP8 GEMM in 1024-row passes with split attention over the rows; later K2 (fused attention at 32/8) and K3 (a dense one-list graph with chip argmax).
7. Gates for the 30B: `own30b_check chip` (`src/bin/own30b_check.rs`): greedy tokens IDENTICAL to the golden file on 4 prompts, per-layer parity (cosine at least 0.999), routing and window-skew mutants caught, decode tok/s. Latest receipt: 38.8 / 36.1 / 37.2 / 36.6 tok/s (25.8 to 27.7 ms per token) in `docs/inference/evidence/own30b-idle-speed-20261015/check-full/run.log:27-39`; PR #524 recorded 38.9 / 35.9 / 37.4 / 37.0 against 39.1 / 36.1 / 37.6 / 37.0 before it, so run-to-run noise is about 0.2 to 0.5 tok/s. KV restore gates: `restored_prefix_continues_exactly_like_the_uninterrupted_engine` (`:5565`), `paged_and_host_pool_caches_export_the_same_bytes_and_round_trip` (`:5514`), and `crates/aien-serve-http/tests/kv_persist.rs` (`park_then_restart_then_restore_equals_the_uninterrupted_run` and the other six).

## Decision drivers

- Rule 3 above: nothing is added to the per-layer GPU path. The attention half of a layer is the same maths in both models, and it stays duplicated on purpose.
- The one-list decode graph chooses the token on the chip and reads logits back only on demand. The seam must never force a hidden-state or logits round trip the graph avoids today.
- Phase routing is part of the 30B's numerics: a 1-token prompt suffix goes through `step_batch`, which falls back to `step`, never through the graph. Routing by token count would change kernels and could flip tokens.
- The door's full-logits path is the serving path and must stay bit-identical.
- The interface is the test surface: a host fake behind it must make every controller behaviour testable without a chip or weights.
- Two real adapters (30B, 8B) plus the fake make the seam real.

## 1. Vocabulary

Generation controller: the module that owns the sequence ledger (history and position), prefix reuse, rewind, export and restore, sampling, constraints, termination and streaming. Written once. Type `GenerationController<M>`.
Forward pass: a model implementation behind the seam: weights, layer maths, kernels, attention path, prefill strategy, decode graph, lm_head, traces and mutants. Trait `ForwardPass`; adapters `Qwen3MoeOmegaEngine` (30B), `Qwen3DenseOmegaEngine` (8B), `EchoChainModel` (host fake).
KV geometry: the five numbers plus the block size that fix the KV cache's memory layout and payload bytes. Type `KvGeometry`. Produced by each model's config; consumed by every cache constructor and by persistence.
KV store view: the model-independent half of the cache (`KvStore`): geometry, capacity, truncate, export, import. The controller sees only this half.
Head: a pending output of the last forward call, readable once, from which the next token is chosen.
Step rule: what a constraint allows at one decode step (anything, an allowed set, one forced token, or finish).

## 2. The interface

### 2.1 KV geometry and the KV store view (order step 1)

```rust
/// Model-independent KV shape. Produced by a model config, consumed by every cache constructor
/// and by persistence. bf16 elements, HostPagedPoolV1 payload (ADR 0003).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KvGeometry {
    pub layers: usize,
    pub q_heads: usize,      // attend output width; the fused kernel's `fits` check
    pub kv_heads: usize,
    pub head_dim: usize,
    pub rope_theta: f32,     // the fused cache applies RoPE on the chip
    pub block_size: usize,   // KV_BLOCK_SIZE
}
impl KvGeometry { pub fn pool_block(&self) -> PoolBlockGeometry; }
impl From<&Qwen3MoeConfig> for KvGeometry { .. }
impl From<&Qwen3DenseConfig> for KvGeometry { .. }

/// The model-independent half of a KV cache. A supertrait split out of KvAttention.
pub trait KvStore: Send {
    fn geometry(&self) -> KvGeometry;
    fn capacity(&self) -> usize;
    /// Drops every position at or past `len`; `truncate(0)` is today's `reset`.
    fn truncate(&mut self, len: usize);
    fn export_blocks(&mut self, start: usize, len: usize) -> Option<Result<Vec<u8>, String>>;
    fn import_blocks(&mut self, start: usize, len: usize, bytes: &[u8]) -> Option<Result<(), String>>;
}
/// Unchanged and model-private: append, attend, resident, fused_attend, path_name, fused_kv,
/// dgraph_cache, dgraph_commit.
pub trait KvAttention: KvStore { .. }
```

Every cache constructor and `PoolBlockGeometry::for_geometry` take `&KvGeometry` (`OmegaFusedKv::new` also takes the per-layer norm weights, as today). `for_config(&Qwen3MoeConfig)` stays as a one-line shim until step 5. `OmegaFusedKv::fits(&KvGeometry)` keeps refusing anything but 32/4/128 until K2 lands. Export bytes are byte-identical by construction: the geometry fields are the same five numbers the constructors read today.

### 2.2 The forward pass (order step 2, the seam)

```rust
pub enum Want { Greedy, Logits }
pub enum Next<'a> { Token(u32), Logits(&'a [f32]) }

/// Every setting of a forward pass that can change a K/V byte, as named entries, plus its kind.
/// Built by exhaustive destructuring inside the adapter (no `..`), so a new switch cannot be
/// forgotten. `Err(reason)` means K/V from this run must never be stored (a mutant, tracing).
pub struct KvIdentityInputs { pub kind: &'static str, pub entries: Vec<(&'static str, String)> }

pub trait ForwardPass: Send {
    fn kind(&self) -> &'static str;          // "qwen3-moe-omega", "qwen3-dense-omega", "echo-chain"
    fn vocab(&self) -> usize;
    /// The cache, lent as its model-independent half. Safe to truncate, export and import
    /// between forward calls; the model holds no pending state across them.
    fn kv(&mut self) -> &mut dyn KvStore;
    /// Draft tokens one `decode` call may verify at once. 1 until speculative decode lands.
    fn max_decode_batch(&self) -> usize { 1 }
    /// Prompt phase. Writes K/V for positions pos..pos+len in every layer and leaves ONE head
    /// (head 0, the last token). The model chooses the strategy (whole-prompt, GEMM passes,
    /// chunked step_batch, token at a time) and its chunk sizes behind the seam.
    fn prefill(&mut self, pos: usize, tokens: &[u32], timing: &mut StepTiming) -> Result<(), String>;
    /// Generation phase. 1 <= len <= max_decode_batch(). Writes len positions and leaves len
    /// heads; head j predicts the token after tokens[j]. Returns nothing: no hidden state
    /// crosses the seam. On the 30B this is decode_step (graph or step), unchanged.
    fn decode(&mut self, pos: usize, tokens: &[u32], timing: &mut StepTiming) -> Result<(), String>;
    /// Reads head `head` once. Want::Greedy: Token(t) if the model already holds the argmax under
    /// the sample_argmax rule (lowest index of the maximum), else Logits. Want::Logits: always
    /// Logits (on the graph path, the one read-back of vocab f32; on the host path, the lm_head).
    fn next(&mut self, head: usize, want: Want, timing: &mut StepTiming) -> Result<Next<'_>, String>;
    /// Sparse output head: logits of head `head` for `rows` only, into out[0..rows.len()].
    /// Default: next(head, Logits) then gather. A model may override with a row-list lm_head.
    /// Never called when no constraint is active, so it costs nothing on the unconstrained path.
    fn next_rows(&mut self, head: usize, rows: &[u32], out: &mut [f32], timing: &mut StepTiming)
        -> Result<(), String> { /* default as described */ }
    /// Forgets every unread head (today's dgraph_clear). Called by the controller on reset,
    /// rewind, restore, a forward error and at the start of every generation.
    fn clear_pending(&mut self);
    fn kv_identity(&self) -> Result<KvIdentityInputs, String>;
}
```

Invariants every adapter must keep (the contract suite in section 5 checks them on all three):

- I1, the ledger is the authority: the controller passes `pos`, which equals its `history.len()`; the model must refuse a `pos` its cache does not hold at every layer (the graph path already checks this in `dgraph_cache(pos)`; the split path checks against the cache's filled count). An adapter that keeps its own position mirror during migration resynchronises on entry (`pos == 0` is `reset`, `pos < mirror` is `rewind(pos)`, `pos > mirror` is an error).
- I2, heads are read once: a second read of the same head, or a read after `truncate` or a new forward call, is an error (today's `pending = false` take rule).
- I3, all or nothing from the ledger's view: on `Err` from `prefill` or `decode` the cache rows from `pos` onward are unspecified and the controller truncates to `pos` and clears pending (section 2.4).
- I4, `Want::Greedy` may return `Token` only under exactly the `sample_argmax` tie rule the host would use.
- I5, the controller passes the whole un-reused prompt suffix to `prefill` in one call. The 30B's chunk shape (`prefill_chunk`, `whole_prompt`, `prefill_gemm`) is a KV identity entry, so slicing it differently from outside would change K/V bytes.
- I6, drop order and cache identity: the cache is owned by the adapter and fixed for the adapter's lifetime; the graph's raw chip handle never crosses the seam. The 30B keeps `dgraph` declared before `kv` exactly as today (`:3218-3224`).

Cost on the GPU path: per generated token, two static calls (`decode`, `next`) and one `Vec` push. No hidden vector, no logits and no timing struct crosses that did not cross today. The graph path with `Want::Greedy` returns the pending `u32` with no read-back; with `Want::Logits` it does the one read-back it does today.

### 2.3 Sampler and constraint (controller side)

```rust
pub trait Sampler {
    /// false: the plain argmax; `pick` is never called and the chip's own argmax may stand in.
    fn wants_logits(&self) -> bool;
    fn pick(&mut self, logits: &[f32], step: usize) -> u32;
}
/// What a constraint allows at one decode step.
pub enum StepRule {
    Any,                 // no constraint this step: the unconstrained path, byte for byte
    Allow(Vec<u32>),     // the legal token ids (sorted, non-empty); the sampler sees only these
    Force(u32),          // exactly one legal token: emitted without reading a head
    Finish,              // the output is complete: stop, FinishReason::Constraint
}
pub trait Constraint {
    /// Called once per step before the sampler, with the prompt and the tokens generated so far.
    fn rule(&mut self, prompt: &[u32], generated: &[u32]) -> Result<StepRule, String>;
}
```

The Omegese adapter that turns `omegese next --machine` items (`word`, `newname`, `number LO HI`, brackets, `space`, `newline`, end of input) into token ids lives outside the controller and is not designed here. The controller's rules:

- `Any`: identical to today's loop. `next(head, want)` with the sampler's `wants_logits`.
- `Allow(rows)`: the controller calls `next_rows(head, &rows, buf)`, writes the rows into a vocab-sized buffer filled with negative infinity (one reused buffer, about 608 KB on a 151936 vocab (calc:c40df54a), tens of microseconds against a 26 ms step), and calls `pick` on it. The chip argmax is never used under a constraint, whatever `wants_logits` says: a constrained argmax is the argmax over the allowed set.
- `Force(t)`: the head is discarded (`clear_pending`), `t` is emitted and decoded like any chosen token, and the step counter advances (seeded sampling keeps its replay property).
- `Finish`: `FinishReason::Constraint`, nothing emitted, nothing stepped.
- A constraint error ends the generation with `Err` after the rollback of section 2.4.

`FinishReason` gains the variant `Constraint`. That is a pure addition; the compiler finds every match.

### 2.4 The generation controller

```rust
pub struct GenerationController<M: ForwardPass> {
    model: M,                 // owns the cache; declared first, drops first
    history: Vec<u32>,        // positions 0..history.len() are stored in every layer
    masked: Vec<f32>,         // the constraint buffer, allocated on first use
}
impl<M: ForwardPass> GenerationController<M> {
    pub fn new(model: M) -> Self;
    pub fn max_context_tokens(&self) -> usize;              // model.kv().capacity()
    pub fn position(&self) -> usize;  pub fn history(&self) -> &[u32];
    pub fn reset(&mut self);                                // clear_pending; kv.truncate(0); history.clear()
    pub fn rewind(&mut self, len: usize);                   // clear_pending; kv.truncate(len); history.truncate(len)
    pub fn reusable_prefix(&self, prompt: &[u32]) -> usize; // moved verbatim from :3858
    pub fn export_kv(&mut self, start: usize, len: usize) -> Result<Vec<u8>, String>;   // verbatim from :3870
    pub fn restore_kv(&mut self, tokens: &[u32], payload: &[u8]) -> Result<(), String>; // verbatim from :3890
    pub fn kv_identity(&self) -> Result<KvIdentityInputs, String>;
    pub fn generate(&mut self, prompt: &[u32], params: &GenerateParams, sampler: &mut dyn Sampler,
                    constraint: Option<&mut dyn Constraint>, on_token: &mut dyn FnMut(u32) -> bool)
        -> Result<GenerateOutcome, String>;
    pub fn model(&self) -> &M;  pub fn model_mut(&mut self) -> &mut M;   // the side door (traces, mutants, switches)
}
/// Today's OmegaEngine: one lock, one sequence at a time. Send + Sync.
pub struct Locked<M: ForwardPass>(Mutex<GenerationController<M>>);
```

The loop, written against the interface (the order is today's `generate_inner` line for line):

```rust
fn generate(&mut self, prompt, params, sampler, mut constraint, on_token) -> Result<GenerateOutcome, String> {
    self.model.clear_pending();
    if prompt.is_empty() || prompt.len() >= self.max_context_tokens() { return Err(..) }   // as today
    let reused = self.reusable_prefix(prompt);
    if reused > 0 { self.rewind(reused) } else { self.reset() }
    let (mut pre, mut dec) = (StepTiming::default(), StepTiming::default());
    self.forward(|m, pos| m.prefill(pos, &prompt[reused..], &mut pre), &prompt[reused..])?;   // I5
    let (mut emitted, mut finish, mut step) = (0, FinishReason::Length, 0);
    while emitted < params.max_new_tokens {
        let rule = match constraint.as_deref_mut() { Some(c) => c.rule(prompt, &self.history[prompt.len()..])?, None => StepRule::Any };
        let next = match rule {
            StepRule::Finish => { finish = FinishReason::Constraint; break }
            StepRule::Force(t) => { self.model.clear_pending(); t }
            StepRule::Allow(rows) => { self.model.next_rows(0, &rows, &mut self.masked_rows, &mut dec)?; sampler.pick(self.masked_dense(&rows), step) }
            StepRule::Any => match self.model.next(0, if sampler.wants_logits() { Want::Logits } else { Want::Greedy }, &mut dec)? {
                Next::Token(t) => t, Next::Logits(l) => sampler.pick(l, step) }
        };
        step += 1;
        if params.stop_token_ids.contains(&next) { finish = FinishReason::Stop; break }   // consumed, never emitted
        emitted += 1;
        if !on_token(next) { finish = FinishReason::Cancelled; break }                   // emitted, not stepped
        if emitted >= params.max_new_tokens || self.position() >= self.max_context_tokens() { break }
        self.forward(|m, pos| m.decode(pos, &[next], &mut dec), &[next])?;
    }
    Ok(GenerateOutcome { emitted_tokens: emitted, finish, prompt_tokens: prompt.len(), reused_prompt_tokens: reused,
                         prefill_timing: pre, decode_timing: dec, prefill_ms, decode_ms })
}
/// forward(f, toks): on Ok, history.extend(toks); on Err, kv.truncate(history.len()), clear_pending, return Err.
```

Two deliberate behaviour changes, both covered by red-before-green tests in section 5:

- Rollback on a forward error (I3). Today a failed `step` leaves a half-written position in some layers until the next generation's rewind evens them (`step` pushes history only after every layer, `:4165`). The controller truncates the cache to the ledger at once, so between controller calls every layer holds exactly `0..position()`.
- Timing is added into caller-supplied `StepTiming` values, which deletes the `last_timing` reset at `:4249`. The sums are the same: `prefill_timing` is the sum of the prefill calls, `decode_timing` the decode calls plus each head read. `prefill_ms` and `decode_ms` keep their wall-clock meaning.

What the `generate` convenience of today becomes: a `Sampler` built from `sample_next` whose `wants_logits()` is `temperature > 0.001`. That is today's `argmax_sampler` flag, unchanged. The door's `SamplingObserver` is a `Sampler` with `wants_logits() == true`, so the door's path reads the full logits every step exactly as today (section 3, point 3).

### 2.5 Where things live

In front of the seam (the controller, once): prompt checks; the prefix rule; rewind, reset, restore and export choreography and their error rules; the ledger; rollback; the sampling decision (argmax or logits); constraints and the sparse-rows buffer; stop ids, length, context-full and cancel; streaming order; timing buckets and `GenerateOutcome`; speculative accept and reject (section 3, point 7); the lock (`Locked`).

Behind the seam (each forward pass): weights and every layer's maths; the attention path and the cache with all four `KvAttention` adapters; the prefill strategy and chunk sizes; the whole-prompt driver; the decode graph, its pending token and logits buffer; the lm_head and any sparse-row lm_head; traces, mutants and per-kernel timing; the identity entries.

Outside both (the door): the GPU lock file, the persistence store, the identity digest over `KvIdentityInputs` plus weights format, build and environment.

Traces and switches never cross the seam. The gate harness reaches `trace_layers`, `traces`, `routing_mutant` and the whole-prompt setters through `model_mut()` on the concrete `GenerationController<Qwen3MoeOmegaEngine>`. Side-door contract: switch changes only; anything that touches the cache is followed by `controller.reset()`.

## 3. Points settled explicitly

1. Who owns the KV cache: the forward pass. Three reasons, all VERIFIED. The decode graph takes the cache's raw chip handle (`kv.dgraph_cache(pos)`, an `OmegaGpuDecodeAttnCache *`) into `DecodeGraph::create`, and memory safety rests on `dgraph` being declared before `kv` in the same struct so the graph drops first (`:3218-3224`); a controller-owned cache would carry that pointer's lifetime across the seam as an unwritten contract. The fused cache holds model weights (`OmegaFusedKv::new` uploads every layer's q_norm and k_norm, `dattn.rs:52-60`) and a kernel-specific `fits` rule, so it is not model-free. The whole-prompt driver reaches into it through `fused_kv()`. The controller needs only the model-independent half, which the model lends through `kv()` as `KvStore`; the ledger stays with the controller and is the authority for position (I1).
2. Static generics at the seam. `GenerationController<M: ForwardPass>` is monomorphised; gate binaries and tests use the concrete 30B type and reach traces through `model_mut()` with no downcast. The door becomes `OmegaDoor<M: ForwardPass>` and the only dynamic boundary stays where it already is: the serve-http `Engine` trait (`engine.rs:91-110`, three adapters today). `main.rs` picks `OmegaDoor<Qwen3MoeOmegaEngine>` or `OmegaDoor<Qwen3DenseOmegaEngine>` from `--arch` and hands either to `Arc<dyn Engine>`. `ForwardPass` has no associated types and no generic methods, so `dyn ForwardPass` remains possible for tests that mix adapters.
3. The door's full-logits path stays bit-identical. The door's sampler wants logits, so the controller asks `Want::Logits` and the 30B adapter does today's `dgraph_take_logits` read-back. `Want::Greedy` is asked only where today's code sets `argmax_sampler = true` (the `generate` convenience that `own30b_check` runs). Switching the door to the free chip argmax is a separate, opt-in change with its own gate (the sampling gate plus golden tokens); it is not part of the migration in section 7.
4. Prefill and decode are separate methods, not one call with a phase hint. The 1-token prompt suffix is a `prefill` call and therefore runs `step_batch`, which falls back to `step`, and never the graph; that is a type-level fact, not a hint the controller could get wrong. Prefill strategy (whole-prompt, GEMM passes, chunking) is chosen behind the seam from the model's own switches; the controller passes the whole suffix in one call (I5).
5. Rollback on a forward error is a deliberate behaviour change, tested red before green (section 5, test R1).
6. The constraint hook sits between the head read and the sampler (section 2.3): allowed set, forced token, finish, plus the optional sparse output head `next_rows` with a gather default that is never called on the unconstrained path. The fake tests it end to end (section 5).
7. Speculative decode fits without changing any method: `decode(pos, &[t0, d1..dn])` with `max_decode_batch() >= n + 1` leaves `n + 1` heads; the controller reads `next(j, ..)` for rising `j`, applies the stop, cancel, length and constraint rules per accepted token, and `rewind`s the rejected tail. A graph adapter may read all chip argmaxes in one small read at the first `next`. Today's adapters return `max_decode_batch() == 1`, so the controller never sends more than one token.
8. The fake is C's `EchoChainModel` with its two negative controls (section 4).

## 4. The three adapters

30B MoE (`qwen3_moe_omega/forward_pass.rs`, new file, about 150 lines; `qwen3_moe_omega.rs` gains the `KvStore` supertrait split, the `KvGeometry` constructors and one `head_hidden: Vec<f32>` field that today is a local in `generate_inner`). `prefill` is `whole_prompt_prefill` then the `step_batch` chunk loop, word for word from `:4242-4247`, storing the hidden vector. `decode(&[t])` is `decode_step(t)`. `next(0, Greedy)` is `dgraph_take_greedy(true)` giving `Token`, else `logits(&head_hidden)` plus `sample_argmax`; `next(0, Logits)` is `dgraph_take_logits` or `logits(&head_hidden)`. `clear_pending` is `dgraph_clear`. `kv()` is `&mut *self.kv`. `kv_identity` destructures `Qwen3MoeConfig` and `OmegaKvSwitches` exhaustively into named entries and refuses the routing mutant. During steps 3 and 4 the engine keeps its own `history` and `pos` mirror and resynchronises on entry (I1); step 5 deletes the mirror and `generate_inner`. Nothing in `step`, `step_batch`, the graph or the KV adapters changes.

8B dense (`Qwen3DenseOmegaEngine { weights, kv: Box<dyn KvAttention>, head_hidden, .. }`, the plan's stage 2). `prefill`: FP8 GEMM in 1024-row passes with split attention over the rows; `decode`: FP8 GEMV per projection plus split attention on `OmegaPagedKv` built from `KvGeometry::from(&Qwen3DenseConfig)` (36 layers, 32/8, 128); `next` always `Logits` until K3, whose dense one-list graph adds `Token` with no controller change; `kv_identity` destructures `Qwen3DenseConfig` and the dense switches (FP8 mutant hooks included, so mutants refuse persistence the same way). Traces are dense-shaped and reached through `model_mut()`. No router, no MoE, no whole-prompt path.

Host fake (`EchoChainModel`, `test_support` behind `host-test-engine`). No weights, vocab 64, the tiny geometry (2 layers, 1 KV head of 128), running on a real `HostPoolKv` so export, restore and park move real `HostPagedPoolV1` bytes. Forward: for token `t` at position `p` and layer `l` it appends K and V rows of small integers exact in bf16, `((t*131 + p*17 + l*7 + d + 3*s + salt) % 61) - 30` (`s` 0 for K, 1 for V), then attends with a fixed query over `0..=p` at the last layer; that output is its hidden. Logits: `h` is a hash of the hidden bits mixed with `p`, `logits[i] = ((h + 37*i) % 64) / 64`; 37 is odd so the argmax is unique. The fake keeps no history of its own, so every logit depends on every stored row: a stale row, a missing row, a wrong position or a wrong payload changes the token. Instruments: a call log (`Prefill{pos,len}`, `Decode{pos,tokens}`), `logits_reads`, `rows_reads`, `graph_mode` (argmax taken eagerly at `decode` and held, like the graph), `fail_at` (fails after writing layer 0 only, a deliberate half-write), `salt` (an identity entry), `max_decode_batch`. Two negative controls prove its sharpness: `NoTruncateKv`, a `HostPoolKv` wrapper that ignores `truncate`, must make a warm run differ from a cold run; and restoring history A's payload under tokens B must continue differently from a cold run on B.

## 5. Tests

Survive unchanged (they test caches, samplers or maths, not the controller): `paged_and_host_pool_caches_export_the_same_bytes_and_round_trip` (`:5514`, with the `for_config` shim), `seeded_sampling_replays_and_greedy_is_argmax` (`:5159`), `batched_prefill_matches_token_at_a_time_bit_for_bit_with_fewer_launches` (`:4946`), the routing and GEMM mutant tests, the decode-graph mutant parse test, the GPU-lock test, `own30b_check` (golden tokens, parity, mutants, tok/s), and serve-http `tests/kv_persist.rs` and `tests/loopback.rs` until step 7.

Move to the controller's interface, run on the fake and on the tiny MoE through the 30B adapter: `prefix_reuse_prefills_only_the_new_tokens_and_matches_a_cold_run` (`:4888`), `generate_stops_on_stop_ids_caps_length_and_cancels` (`:5102`), `restored_prefix_continues_exactly_like_the_uninterrupted_engine` (`:5565`) and its damaged-restore sibling. `kv_switches_report_every_run_setting` (`:5662`) becomes an adapter test: every switch changes an entry of `kv_identity`, mutants and tracing refuse.

New, PLANNED, made possible by the fake (names are the test names):

- Contract suite `forward_pass_contract<M: ForwardPass>(make)` run on all three adapters: `Token(t)` equals the argmax of that head's `Logits` (I4); a head read twice, or after `truncate`, errors (I2); `prefill` of `n` tokens leaves exactly one head; export then import then decode equals the uninterrupted run; a `pos` the cache does not hold is refused (I1); `next_rows` equals the gather of `next(Logits)`.
- `the_unreused_suffix_reaches_prefill_in_one_call` (I5): call log equals `[Prefill{pos: reused, len: prompt.len() - reused}]` then exactly one `Decode` per emitted token except the last.
- `greedy_with_chip_argmax_reads_no_logits`: `graph_mode` greedy has `logits_reads == 0`; sampled has exactly one read per token; graph and host modes give identical tokens.
- R1, red before green: `a_forward_error_leaves_every_layer_at_the_ledger`: with `fail_at` mid-prefill, after the error every layer of the cache holds exactly `position()` rows and `history` is unchanged; the next turn equals a cold run. Written against the pre-rollback loop first (it fails on the half-written layer 0), then the rollback lands and it passes.
- `stop_and_cancel_tokens_are_never_stepped`; `each_on_token_fires_before_the_next_decode`; `a_full_context_ends_with_length_and_no_error`; `a_failed_restore_resets_the_ledger`; `max_new_tokens_zero_reads_no_head`.
- Constrained generation, end to end on the fake: `an_allowed_set_picks_the_best_token_inside_it` (the unconstrained argmax is known from the permutation; an `Allow` set that excludes it yields the best in-set token, and `rows_reads == 1`, `logits_reads == 0`); `a_forced_token_is_emitted_decoded_and_counted` (it appears in the stream, in the history and in the K/V bytes; the step counter advanced; no head read); `finish_ends_with_finish_reason_constraint`; `any_is_byte_identical_to_no_constraint` (same tokens, same call log, same read counters as a run with `None`); `a_constraint_error_rolls_back_like_a_forward_error`; `seeded_sampling_replays_under_a_constraint`.
- Speculative decode (when `max_decode_batch > 1` exists on the fake): accept, reject and a stop inside an accepted run, with the tail rewound.
- The two negative controls of section 4, both asserted red.
- Dual-run equivalence on the tiny MoE (steps 3 and 4 only, deleted at step 5): old `generate_inner` against `GenerationController` plus the 30B adapter, identical tokens, identical `reused_prompt_tokens`, identical `finish`, identical `StepTiming` sums, for greedy and seeded sampling, cold and warm, and after a restore.

Chip only: `own30b_check` (golden tokens on 4 prompts, parity, mutants, tok/s), the native branch of the export round trip, graph argmax equals host argmax on real weights, whole-prompt and fused numerics, the pkv park and restore chip item.

## 6. Rejected alternatives

- One `advance(pos, feed, tokens, pick)` call with a `Feed::{Prompt, Generated}` hint (design A). Smallest surface, but phase routing becomes a hint the controller could pass wrongly, and speculative decode needs a sibling method; two methods with the phase in the name cost nothing and make the 1-token-suffix rule structural.
- The controller owns `Box<dyn KvAttention>` and lends it to the model per call (design C). It puts invariant I1 in one struct, which is attractive, but the graph's raw chip handle and the fused cache's uploaded norm weights would then cross the seam, and the drop order would have to be documented on the controller instead of living where the handle is created. The `KvStore` view gives the controller the same five operations without the handle.
- `position()` on the model as the source of truth (design B). The ledger would then be a mirror that can drift; passing `pos` in and having the model refuse a mismatch keeps one authority and lets the migration keep the engine's mirror temporarily.
- An associated `Switches` type per model (design C). The door needs one identity shape to hash and to test field by field; named entries give that and keep the exhaustive destructure next to the fields.
- `dyn ForwardPass` at the controller. Unnecessary: the dynamic boundary already exists at the serve-http `Engine` trait, and a static controller keeps the gate harness's trace access free of downcasts.
- Routing prefill and decode by token count. Would send the 1-token prompt suffix into the graph and could flip tokens.
- A shared attention-half module used by both forward passes. Rejected by Drake's rule 3: no abstraction on the GPU path to remove duplication. The attention half stays duplicated.
- The plan's "mirror, not edit" second engine (handoff section 1.2 last bullet). Rejected by Drake's objective.
- A dense logits mask for constraints (vocab-sized multiply per step) as the only mechanism. Kept as the sampler-facing buffer, but the model-facing call is the row list, so a sparse lm_head can land later without touching the controller.

## 7. Migration sequence (each step one pull request, reverted by one revert)

Gates named G30 are the 30B's: G30-tokens is `own30b_check chip` greedy tokens IDENTICAL to the golden on all 4 prompts with parity and mutants green; G30-kv is `restored_prefix_continues_exactly_like_the_uninterrupted_engine`, `paged_and_host_pool_caches_export_the_same_bytes_and_round_trip` and the serve-http `tests/kv_persist.rs` suite; G30-speed is the decode tok/s threshold below. Host tests are `cargo test -p aien-inference-abi -p aien-serve-http` with the `host-test-engine` feature, as CI runs them.

G30-speed threshold: on each of the 4 prompts, decode tok/s at least 97 percent of the same prompt's number in the most recent sealed receipt on main (today 38.8 / 36.1 / 37.2 / 36.6, so at least 37.6 (calc:6c7cc596) / 35.0 (calc:e9fba8ea) / 36.1 (calc:db2fae50) / 35.5 (calc:bd5d53b2)), measured under the quiet flag with the same spin window and weight format. Justification: the seam adds two static calls and one push per token, well under a microsecond against 25.8 (calc:eee92f5c) to 27.7 ms (calc:498d9c9e) per token; the observed run-to-run noise is 0.2 to 0.5 tok/s (about 1.3 percent at worst, calc:0912fcab; PR #524); 3 percent is about 0.83 ms per token (calc:2ecb7cb8), twice the noise and far above anything the seam can add, so a miss is a real regression. One miss earns one rerun; two misses block the merge. An accidental logits read-back (the one cheap thing the seam could add by mistake) is far below 3 percent, which is why `greedy_with_chip_argmax_reads_no_logits` guards it on the host instead.

| Step | Change | Gate | Revert |
|---|---|---|---|
| 1 | `KvGeometry`, `KvStore` supertrait split, every cache constructor and `PoolBlockGeometry::for_geometry` take the geometry; `for_config` shims; `From<&Qwen3MoeConfig>` and `From<&Qwen3DenseConfig>` | host tests green; a new test that `KvGeometry::from(&c)` carries the same five numbers `for_config` read; G30-kv; G30-tokens with `--tokens-only` (the fused constructor signature changed) | one revert; no caller behaviour changes |
| 2 | Add `ForwardPass`, `Sampler`, `Constraint`, `StepRule`, `GenerationController`, `Locked`, `EchoChainModel`, the contract suite and every controller test of section 5 including R1 red-before-green and the two negative controls; `FinishReason::Constraint` | host tests green; the fake tests green; the negative controls red; the 30B untouched (no chip gate) | one revert; pure addition |
| 3 | The 30B adapter `impl ForwardPass for Qwen3MoeOmegaEngine` beside the old loop, with the mirror resync; the dual-run equivalence test; the moved tests run on both adapters | host tests green including the dual run and the contract suite on the tiny MoE; G30-tokens; G30-speed (the adapter is exercised by the dual run only, so this gate is a baseline refresh) | one revert; nothing calls the adapter yet |
| 4 | `OmegaEngine::generate` and `generate_with` route through `GenerationController` (`OmegaEngine` becomes `Locked<Qwen3MoeOmegaEngine>` with the same methods) | G30-tokens; G30-kv; G30-speed; serve-http `kv_persist` and `loopback` unchanged and green; the pkv chip park-and-restore item | one line |
| 5 | Delete `generate_inner`, the engine's `history` and `pos` mirror, the five history-push sites and the dual-run test; `for_config` shims go | host tests; G30-tokens; G30-kv; G30-speed | one revert; only after step 4 has passed its gates on two separate days |
| 6 | `Qwen3DenseOmegaEngine` implements `ForwardPass` (the plan's stage 2); the 8B runs through `GenerationController` | the contract suite on the host with `HostFp8Gemv` tiny weights; the 8B oracle of the plan (section 2.4: per-layer exactness, greedy agreement, the FP8 mutant caught); the 30B is untouched | one revert |
| 7 | `OmegaDoor<M>`; `IdentityInputs` takes `KvIdentityInputs` plus the door's own inputs; `--arch moe\|dense`; `snapshot` takes the controller | `kv_persist` and `loopback` on the fake and on the tiny MoE, including `each_identity_input_changed_alone_is_never_served` over the named entries; the identity digest is either pinned equal to main's recorded value by a test or the namespace bump is recorded in ADR 0003's record (a bump means one cold miss, never wrong data); chip: the pkv park-and-restore item on the 30B and the 8B served through the door | one revert |
| 8 | (order step 5, second half, separate ADR if needed) the gate harness shared by `own30b_check` and the 8B check | out of scope here | |

Rules across all steps: the 30B is the running code until step 4 and keeps its maths unchanged throughout; no step touches `crates/aien-runtime/src/spine.rs`; each step's PR body names its gate receipts; a chip gate run needs the quiet flag; the constraint engine (the Omegese adapter) is a separate piece of work that plugs into the hook from step 2.

## Consequences

- Prefix reuse, rewind, park, restore, termination, streaming and constraints are written once and tested on the host in milliseconds. The 8B gets them by implementing the ten `ForwardPass` methods, two of which have defaults.
- `StepTiming` keeps its MoE-flavoured fields; the 8B leaves them at zero. Splitting it is later work, off the migration path.
- Each model still owns its prefill chunking and its identity entries; that is the thin part of the leverage and it is deliberate (the strategies and the switches are model-specific).
- Serving more than one sequence per model at a time is outside this seam (one ledger, one cache, one lock), as it is today.

## Known limits

- The side door (`model_mut`) is a trust contract, not a type guarantee: a gate harness that touches the cache through it must call `reset()` afterwards.
- The identity digest may change at step 7 if the entries are not reproduced in today's order; the step's gate forces a choice between pinning it and recording the bump.
- The sparse output head is a default gather until a row-list lm_head kernel exists; the research note's speed gain (recommendation 5) is not claimed by this ADR.
