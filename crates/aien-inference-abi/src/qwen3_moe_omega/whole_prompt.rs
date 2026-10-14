//! Whole-prompt prefill wired into [`Qwen3MoeOmegaEngine`] (W-L1G-W, 2026-10-14). New file, a
//! child module of `qwen3_moe_omega` so it can reach the engine's private `kv`, `pos`, `history`.
//!
//! Switch: the engine field `whole_prompt` (OFF by default: `None`). [`Qwen3MoeOmegaEngine::set_whole_prompt`]
//! builds or drops the driver. When it is set, `generate_with` hands the not-yet-stored part of
//! the prompt to [`WholePromptPrefill::run`] in ONE call (a fixed launch count per layer) instead
//! of the `prefill_chunk` loop, provided: no layer is traced, the routing mutant is off, and the
//! KV is omega's fused cache ([`KvAttention::fused_kv`]). Otherwise the chunk loop runs as before.
//! Prefix reuse is kept: the driver starts at `pos0 = self.pos` after `rewind` / `reset`.
use std::time::Instant;

use super::{KvAttention, Qwen3MoeOmegaEngine, StepTiming};
use crate::qwen3_moe_omega_prefill::WholePromptPrefill;

/// The real driver type the engine holds.
pub type EngineWholePrompt = WholePromptPrefill<aien_omega_pf::OmegaPfDevice>;

impl Qwen3MoeOmegaEngine {
    /// Turns the whole-prompt prefill on (builds the driver over the resident FP8 weights, read
    /// in place) or off. Returns why it cannot be turned on: the prefill kernels or the device
    /// attention entry not linked, the KV not omega's fused cache, or weights not FP8 everywhere.
    pub fn set_whole_prompt(&mut self, on: bool) -> Result<(), String> {
        if !on {
            self.whole_prompt = None;
            return Ok(());
        }
        if self.whole_prompt.is_some() {
            return Ok(());
        }
        if !aien_omega_pf::is_native() || !aien_omega_pf::has_attention() {
            return Err("whole-prompt prefill: omega's prefill GEMM / prefill attention are not linked in this build".into());
        }
        if self.kv.fused_kv().is_none() {
            return Err(format!("whole-prompt prefill needs omega's fused KV cache, attention path is {}", self.kv.path_name()));
        }
        self.whole_prompt = Some(Box::new(WholePromptPrefill::new(aien_omega_pf::OmegaPfDevice, &self.weights)?));
        Ok(())
    }

    /// Gives the whole-prompt GEMMs their own GPU-cached copy of every FP8 weight (about the
    /// model's FP8 size again, held until the switch is turned off). Returns the bytes copied.
    pub fn whole_prompt_cache_weights(&mut self) -> Result<usize, String> {
        let c = self.weights.config.clone();
        self.whole_prompt.as_mut().ok_or("whole-prompt prefill is off")?.cache_weights(&c)
    }

    /// Whether `generate_with` will take the whole-prompt path.
    pub fn whole_prompt_on(&self) -> bool {
        self.whole_prompt.is_some()
    }

    /// The last whole-prompt call's counters (launches, chip and host time).
    pub fn whole_prompt_stats(&self) -> Option<&crate::qwen3_moe_omega_prefill::PrefillStats> {
        self.whole_prompt.as_ref().map(|w| &w.stats)
    }

    /// Called by `generate_with` before its chunk loop. When the switch is on and the path
    /// applies, runs `tokens` (positions `self.pos ..`) as one whole prompt, stores them as
    /// history, sets `hidden` and adds the timing; returns how many tokens it consumed (all or 0).
    pub(super) fn whole_prompt_prefill(&mut self, tokens: &[u32], hidden: &mut Vec<f32>, timing: &mut StepTiming) -> Result<usize, String> {
        if tokens.is_empty() || !self.trace_layers.is_empty() || self.routing_mutant {
            return Ok(0);
        }
        let Some(wp) = self.whole_prompt.as_mut() else {
            return Ok(0);
        };
        let Some(fused) = self.kv.fused_kv() else {
            return Ok(0);
        };
        let (pos0, m) = (self.pos, tokens.len());
        if pos0 + m > fused.capacity() {
            return Err(format!("whole-prompt prefill: {pos0} + {m} tokens > context {}", fused.capacity()));
        }
        let t = Instant::now();
        let out = wp.run(&self.weights, &mut fused.prefill_attention(), tokens, pos0);
        let wall = t.elapsed().as_nanos() as u64;
        // On failure the cache rows past pos0 are unspecified: forget them before reporting.
        let h = match out {
            Ok(h) => h,
            Err(e) => {
                self.rewind(pos0);
                return Err(e);
            }
        };
        fused.mark_filled(pos0 + m);
        self.pos = pos0 + m;
        self.history.extend_from_slice(tokens);
        let st = &wp.stats;
        timing.add(&StepTiming {
            total_ns: wall,
            attention_ns: st.attn_ns,
            experts_ns: 0,
            lm_head_ns: 0,
            host_ns: st.host_ns,
            chip_ns: st.chip_ns + st.attn_ns,
            launches: st.launches() as u64,
            route_ns: 0,
            moe_host_ns: 0,
        });
        *hidden = h;
        Ok(m)
    }
}
