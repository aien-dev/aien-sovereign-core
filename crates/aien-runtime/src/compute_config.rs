//! Binding of the resolved `ModelConfig` to the persistent-KV identity (ADR
//! 0003 section 1.2 fields 4 to 8 and 14, section 1.2.1, review finding 1).
//!
//! Two config.json files with the same weights but a different `rms_norm_eps`
//! produce different K/V, and the weights digest does not cover `config.json`.
//! So every `ModelConfig` field is accounted for in ONE place,
//! [`ModelBinding::from_model`], which destructures `ModelConfig`
//! **exhaustively with no `..`**: adding a field to `ModelConfig` is a compile
//! error here until its author decides "bound" (and where) or "excluded, with a
//! reason". The reasons for today's exclusions are written next to the pattern.
//!
//! The binding is built from the RESOLVED `ModelConfig` (serde defaults
//! applied), never from raw `config.json` text (ADR 1.2.1: JSON has no
//! canonical form and raw files carry fields that cannot change a K/V value).

use aien_abi_core::ModelConfig;
use aien_inference_abi::KV_ARITH_EPOCH;
use aien_kv_store::{
    BackendFormat, ComputeConfig, HiddenAct, KvDtype, KvError, LayoutStrides, NamespaceFields,
    RopeScaling,
};

/// Everything a `ModelConfig` contributes to the namespace id: the compute
/// configuration (field 14, through its digest) and namespace fields 4 to 8.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelBinding<'a> {
    pub compute: ComputeConfig<'a>,
    /// Namespace field 4.
    pub model_id: &'a str,
    /// Namespace field 5.
    pub num_layers: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub block_size: u32,
    /// Namespace field 6.
    pub rope_theta: f32,
    /// Namespace field 7.
    pub rope_scaling: Option<RopeScaling>,
    /// Namespace field 8.
    pub qk_norm: bool,
}

/// The namespace inputs that do not come from `ModelConfig`: the weights
/// identity, the byte layout, the chunking and the sharing scope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoreInputs<'a> {
    pub model_sha256: [u8; 32],
    pub model_digest_kind: &'a str,
    pub dtype: KvDtype,
    pub layout: LayoutStrides,
    pub backend_format: BackendFormat,
    pub chunk_tokens: u32,
    pub sharing_namespace: &'a str,
}

fn wire_u32(v: usize) -> Result<u32, KvError> {
    u32::try_from(v).map_err(|_| KvError::Overflow)
}

impl<'a> ModelBinding<'a> {
    /// `arith_id` is `TensorBackend::name()` of the backend that computes the
    /// K/V; `arith_epoch` is the constant `KV_ARITH_EPOCH`. Refused
    /// (`KvError::Overflow`) when a count does not fit its wire width.
    pub fn from_model(cfg: &'a ModelConfig, arith_id: &'a str) -> Result<Self, KvError> {
        // Exhaustive on purpose: no `..`. Do not add one.
        let ModelConfig {
            // Namespace fields 4 to 8: bound by the namespace id itself.
            model_id,
            num_layers,
            num_kv_heads,
            head_dim,
            block_size,
            rope_theta,
            rope_scaling,
            qk_norm,
            // Compute configuration, ADR 1.2.1 table fields 1 to 6.
            num_heads,
            hidden_dim,
            intermediate_dim,
            vocab_size,
            rms_norm_eps,
            tie_word_embeddings,
            // Excluded, with the reasons of ADR 1.2.1 "Fields not listed":
            // it caps the context and does not change any K/V value (the cap is
            // applied to the restorable prefix instead, ADR 6.3);
            max_sequence_length: _,
            // it decides when a sequence stops, not what is computed.
            eos_token_ids: _,
        } = cfg;
        Ok(Self {
            compute: ComputeConfig {
                num_heads: wire_u32(*num_heads)?,
                hidden_dim: *hidden_dim as u64,
                intermediate_dim: *intermediate_dim as u64,
                vocab_size: *vocab_size as u64,
                rms_norm_eps: *rms_norm_eps,
                tie_word_embeddings: *tie_word_embeddings,
                // The loader refuses everything but silu (model_dir.rs), so the
                // byte is a constant today; it exists so a future activation
                // cannot alias.
                hidden_act: HiddenAct::Silu,
                arith_id,
                arith_epoch: KV_ARITH_EPOCH,
            },
            model_id,
            num_layers: wire_u32(*num_layers)?,
            num_kv_heads: wire_u32(*num_kv_heads)?,
            head_dim: wire_u32(*head_dim)?,
            block_size: wire_u32(*block_size)?,
            rope_theta: *rope_theta,
            rope_scaling: rope_scaling.map(|s| RopeScaling {
                factor: s.factor,
                low_freq_factor: s.low_freq_factor,
                high_freq_factor: s.high_freq_factor,
                original_max_position_embeddings: s.original_max_position_embeddings as u64,
            }),
            qk_norm: *qk_norm,
        })
    }

    /// The full namespace inputs with field 14 filled from this binding's
    /// compute digest. `NamespaceFields::namespace_id()` does the hashing.
    pub fn namespace_fields(
        &self,
        store: &StoreInputs<'a>,
    ) -> Result<NamespaceFields<'a>, KvError> {
        Ok(NamespaceFields {
            model_sha256: store.model_sha256,
            model_digest_kind: store.model_digest_kind,
            model_id: self.model_id,
            num_layers: self.num_layers,
            num_kv_heads: self.num_kv_heads,
            head_dim: self.head_dim,
            block_size: self.block_size,
            rope_theta: self.rope_theta,
            rope_scaling: self.rope_scaling,
            qk_norm: self.qk_norm,
            dtype: store.dtype,
            layout: store.layout,
            backend_format: store.backend_format,
            chunk_tokens: store.chunk_tokens,
            sharing_namespace: store.sharing_namespace,
            compute_config_digest: self.compute.digest()?,
        })
    }
}

/// `ComputeConfig::from(&ModelConfig)` of the ADR text (a free function: both
/// types are foreign to this crate, so a `From` impl is not allowed).
pub fn compute_config_from<'a>(
    cfg: &'a ModelConfig,
    arith_id: &'a str,
) -> Result<ComputeConfig<'a>, KvError> {
    Ok(ModelBinding::from_model(cfg, arith_id)?.compute)
}
