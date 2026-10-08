//! sc#337: "no silent fallback" must be shown by what a run keeps, not by the
//! absence of a crash. The real submission path the daemon uses
//! (`submit_work` -> scheduler -> `NativeTransformerBackend::execute_step`)
//! reports, in `CompletionEvent::Finished`, the tensor backend and its op
//! counters; the generation record and the daemon's `OP_REPORT` log line carry
//! them. CPU reference tensors, micro model, no chip.
use aien_inference_abi::native_ops::{NativeOpMask, OpAccounting, TensorOp};
use aien_inference_abi::{
    MockInferenceBackend, ModelConfig, NativeTransformerBackend, OpEvidence, ReferenceCpuBackend,
    RopeParams, SamplingParams, TensorBackend, TransformerWeights,
};
use aien_kv_cache::{create_shared_kv_manager, SharedKvManager};
use aien_runtime::spine::AienRuntimeSpine;
use aien_scheduler::{ChannelCompletionSink, CompletionEvent, PromptHandle, SchedulerConfig};
use std::sync::Arc;

fn micro_config() -> ModelConfig {
    ModelConfig {
        model_id: "aien-micro-v1".to_string(),
        max_sequence_length: 512,
        block_size: 16,
        num_layers: 2,
        num_heads: 4,
        head_dim: 16,
        num_kv_heads: 2,
        hidden_dim: 64,
        intermediate_dim: 128,
        vocab_size: 256,
        rms_norm_eps: 1e-5,
        rope_theta: 10000.0,
        rope_scaling: None,
        tie_word_embeddings: false,
        eos_token_ids: Vec::new(),
        qk_norm: false,
    }
}

fn spine() -> (AienRuntimeSpine, SharedKvManager) {
    let kv = create_shared_kv_manager(64, 16);
    let cfg = SchedulerConfig {
        max_batch_size: 16,
        max_batch_tokens: 512,
        max_prefill_tokens: 256,
        prefill_chunk_size: 64,
        chunk_prefill: true,
        watermark_blocks: 4,
    };
    (AienRuntimeSpine::new(64, cfg, kv.clone()), kv)
}

fn greedy() -> SamplingParams {
    SamplingParams {
        temperature: 0.0,
        top_p: 1.0,
        max_tokens: 3,
        stop_token_ids: vec![],
    }
}

/// Runs one request to completion; returns the `Finished` op evidence.
async fn run<B: aien_inference_abi::AienInferenceBackend>(
    backend_for: impl FnOnce(SharedKvManager) -> B,
) -> Option<OpEvidence> {
    let (mut spine, kv) = spine();
    let mut backend = backend_for(kv);
    let (sink, mut rx) = ChannelCompletionSink::channel();
    let sink_id = spine.register_completion_sink(Arc::new(sink));
    let prompt: PromptHandle = Arc::from([1u32, 10, 25, 42].as_slice());
    spine
        .submit_work(prompt, greedy(), 1, Some(sink_id))
        .unwrap();
    spine.run_until_complete(&mut backend, 40).await.unwrap();
    let mut ops = None;
    let mut finished = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            CompletionEvent::Finished { ops: o, .. } => {
                finished = true;
                ops = o;
            }
            CompletionEvent::Error { message, .. } => panic!("{message}"),
            CompletionEvent::Token { .. } => {}
        }
    }
    assert!(finished, "the request must finish");
    ops
}

fn native_with(
    tensors: Arc<dyn TensorBackend>,
) -> impl FnOnce(SharedKvManager) -> NativeTransformerBackend {
    move |kv| {
        NativeTransformerBackend::with_shared_kv_and_backend(
            TransformerWeights::reference_test_weights(&micro_config()),
            tensors,
            kv,
        )
    }
}

/// The reference CPU math, with every op claimed native and every `rmsnorm`
/// counted as a fallback: what a dev build (`AIEN_DEV_FALLBACK=1`) records
/// when a claimed-native op runs on the CPU. `record_reference` counts
/// without the strict check, so the test runs the same in any build.
struct ForcedFallback {
    cpu: ReferenceCpuBackend,
    acct: OpAccounting,
}

impl TensorBackend for ForcedFallback {
    fn name(&self) -> &'static str {
        "forced-fallback"
    }
    fn op_report(&self) -> aien_inference_abi::native_ops::OpReport {
        self.acct.report()
    }
    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
        self.acct.record_reference(TensorOp::Rmsnorm);
        self.cpu.rmsnorm(out, x, weight, eps)
    }
    fn rmsnorm_heads(&self, x: &mut [f32], weight: &[f32], head_dim: usize, eps: f32) {
        self.cpu.rmsnorm_heads(x, weight, head_dim, eps)
    }
    #[allow(clippy::too_many_arguments)]
    fn apply_rope(
        &self,
        q: &mut [f32],
        k: &mut [f32],
        pos: usize,
        head_dim: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        rope: &RopeParams,
    ) {
        self.cpu
            .apply_rope(q, k, pos, head_dim, num_q_heads, num_kv_heads, rope)
    }
    fn matmul_vec(
        &self,
        out: &mut [f32],
        x: &[f32],
        weight: &[f32],
        out_dim: usize,
        in_dim: usize,
    ) {
        self.cpu.matmul_vec(out, x, weight, out_dim, in_dim)
    }
    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) {
        self.cpu.swiglu(out, gate, up)
    }
    fn compute_logits(
        &self,
        logits: &mut [f32],
        hidden: &[f32],
        embed_weight: &[f32],
        vocab_size: usize,
        hidden_dim: usize,
    ) {
        self.cpu
            .compute_logits(logits, hidden, embed_weight, vocab_size, hidden_dim)
    }
    #[allow(clippy::too_many_arguments)]
    fn gqa_attention(
        &self,
        out: &mut [f32],
        q: &[f32],
        k_cache: &[f32],
        v_cache: &[f32],
        seq_len: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) {
        self.cpu.gqa_attention(
            out,
            q,
            k_cache,
            v_cache,
            seq_len,
            num_q_heads,
            num_kv_heads,
            head_dim,
        )
    }
}

/// A production run on the CPU reference backend: the call reports the
/// backend by name and zero fallbacks.
#[tokio::test]
async fn a_production_run_reports_its_backend_and_zero_fallbacks() {
    let ops = run(native_with(Arc::new(ReferenceCpuBackend::new())))
        .await
        .expect("the native backend reports its op evidence");
    assert_eq!(ops.backend, "ReferenceCpuBackend");
    assert_eq!(ops.native_fallbacks, 0, "{ops:?}");
    assert!(ops.report.starts_with("OP_REPORT "), "{}", ops.report);
}

/// A forced fallback shows: a non-zero `native_fallbacks` and the op by name.
#[tokio::test]
async fn a_forced_fallback_shows_a_non_zero_count() {
    let tensors = Arc::new(ForcedFallback {
        cpu: ReferenceCpuBackend::new(),
        acct: OpAccounting::new(NativeOpMask::ALL),
    });
    let ops = run(native_with(tensors))
        .await
        .expect("the native backend reports its op evidence");
    assert_eq!(ops.backend, "forced-fallback");
    assert!(ops.native_fallbacks > 0, "{ops:?}");
    assert!(
        ops.report.contains("native_fallbacks=[rmsnorm:"),
        "{}",
        ops.report
    );
    let line = aien_runtime::generation::op_report_line(Some(&ops)).unwrap();
    assert!(
        line.starts_with("OP_REPORT ") && line.ends_with(" backend=forced-fallback"),
        "{line}"
    );
}

/// A backend that does not account its ops makes no claim: None, never zero.
#[tokio::test]
async fn a_backend_without_op_accounting_makes_no_claim() {
    assert_eq!(run(|_| MockInferenceBackend::new(1)).await, None);
    assert_eq!(aien_runtime::generation::op_report_line(None), None);
}

/// The record field: present with the backend's evidence, absent without it.
#[test]
fn record_field_matches_the_evidence_and_is_absent_without_it() {
    let id = aien_runtime::generation::ModelIdentity {
        model_sha256: "m".repeat(64),
        model_digest_kind: "file".into(),
        model_path: "/m".into(),
        tokenizer_sha256: "t".repeat(64),
        tokenizer_path: "/t".into(),
    };
    let ops = OpEvidence {
        backend: "omega-gb10".into(),
        native_fallbacks: 0,
        reference_runs: 7,
        report: "OP_REPORT native=[matmul_vec] reference=[rmsnorm] native_fallbacks=[] reference_runs=[rmsnorm:7]".into(),
    };
    let ev = |o| aien_runtime::generation::TurnEvidence {
        prompt_ids: &[1, 2],
        output_ids: &[3, 4, 5],
        text: "abc",
        total_tokens: 5,
        finish_reason: "eos",
        request_id: 1,
        operation_id: 1,
        decoding: None,
        ops: o,
    };
    let start = aien_runtime::generation::DaemonStart(1);
    let with = aien_runtime::generation::build_record(&id, &ev(Some(&ops)), start);
    assert_eq!(
        with["ops"],
        serde_json::json!({
            "backend": "omega-gb10",
            "native_fallbacks": 0,
            "reference_runs": 7,
            "scope": "process",
            "report": ops.report,
        })
    );
    let without = aien_runtime::generation::build_record(&id, &ev(None), start);
    assert!(without.get("ops").is_none(), "{without}");
}
