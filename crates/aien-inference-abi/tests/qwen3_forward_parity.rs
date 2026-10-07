//! Qwen3 (`Qwen3ForCausalLM`) on the CPU reference path.
//!
//! Oracle: transformers 5.17.0 / torch 2.14.0 on CPU, float32 after upcasting the bf16
//! weights, run by a scratch script outside the repository (provenance in
//! `fixtures/qwen3-tiny/PROVENANCE.md` and `forward_reference.json`). Our code is never its own
//! oracle. Tolerance: absolute 1e-4, the ABSOLUTE_TOLERANCE of tinyllama_parity.rs (set before
//! the first comparison, not tuned).
//!
//! The tiny model has `num_heads * head_dim = 64` against `hidden_size = 32`, per-layer
//! q_norm/k_norm weights that are not 1, tied embeddings, and head_dim read from config.json.
//! The negative test removes the q/k norm and shows the logits move far outside the tolerance,
//! so the comparison is not vacuous.
//!
//! The ignored test `real_qwen3_4b_instruct_2507_matches_transformers` (run with `--ignored`) runs
//! the real model in `AIEN_QWEN3_DIR` against
//! `fixtures/qwen3-4b-instruct-2507-config/real_forward_reference.json` (top-20 logits of the
//! prompt "The capital of France is", transformers 5.17.0 CPU float32). It fails when
//! `AIEN_QWEN3_DIR` is unset, so a missing model never reads as a pass. Its
//! tolerance (5e-3 absolute on 36 layers) was set before the run.

use aien_inference_abi::{
    load_model_config, model_config_from_hf_json, omega_model_refusal, NativeTransformerBackend,
    SequenceState, TransformerWeights,
};
use serde_json::Value;
use std::path::PathBuf;

const ABSOLUTE_TOLERANCE: f32 = 1e-4;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

struct Reference {
    ids: Vec<u32>,
    logits: Vec<f32>,
    argmax: usize,
}

fn reference(path: PathBuf) -> Reference {
    let r: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    Reference {
        ids: r["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect(),
        logits: r["logits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect(),
        argmax: r["argmax"].as_u64().unwrap() as usize,
    }
}

fn max_abs(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn argmax(v: &[f32]) -> usize {
    v.iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0
}

fn tiny() -> (TransformerWeights, Reference) {
    let dir = fixture("qwen3-tiny");
    let config = load_model_config(&dir).expect("config.json");
    assert!(config.qk_norm);
    assert_eq!(
        (config.head_dim, config.num_heads, config.hidden_dim),
        (16, 4, 32)
    );
    let weights = TransformerWeights::load_from_safetensors(dir.join("model.safetensors"), &config)
        .expect("weights");
    (weights, reference(dir.join("forward_reference.json")))
}

#[test]
fn qwen3_tiny_sequence_logits_match_transformers() {
    let (weights, r) = tiny();
    let got = weights
        .forward_sequence_with_diagnostics(&r.ids)
        .activations["last_token_logits"]
        .clone();
    let d = max_abs(&got, &r.logits);
    eprintln!("sequence path max abs diff = {d:e}");
    assert!(d <= ABSOLUTE_TOLERANCE, "max abs diff {d:e}");
    assert_eq!(argmax(&got), r.argmax);
}

#[test]
fn qwen3_tiny_token_by_token_logits_match_transformers() {
    let (weights, r) = tiny();
    let mut state = SequenceState {
        tokens: Vec::new(),
        layers: vec![Default::default(); weights.config.num_layers],
    };
    let mut last = Vec::new();
    for (pos, id) in r.ids.iter().enumerate() {
        last = weights.forward_token(*id, pos, &mut state);
    }
    let got = weights.compute_logits(&last);
    let d = max_abs(&got, &r.logits);
    eprintln!("forward_token path max abs diff = {d:e}");
    assert!(d <= ABSOLUTE_TOLERANCE, "max abs diff {d:e}");
}

#[test]
fn qwen3_tiny_backend_prefill_and_decode_match_transformers() {
    let (weights, r) = tiny();
    let mut backend = NativeTransformerBackend::new_reference(weights);
    let got = backend.prefill_sequence(1, &r.ids).expect("prefill");
    let d = max_abs(&got, &r.logits);
    eprintln!("backend prefill max abs diff = {d:e}");
    assert!(d <= ABSOLUTE_TOLERANCE, "prefill max abs diff {d:e}");

    // Prefill all but the last id, then decode the last id: same logits.
    let mut backend2 = NativeTransformerBackend::new_reference(tiny().0);
    backend2
        .prefill_sequence(2, &r.ids[..r.ids.len() - 1])
        .expect("prefill");
    let seq = backend2.sequences.get_mut(&2).unwrap();
    let h = NativeTransformerBackend::forward_token_impl_paged(
        &backend2.weights,
        &*backend2.tensor_backend,
        *r.ids.last().unwrap(),
        r.ids.len() - 1,
        seq,
        2,
        None,
    );
    let got2 = backend2.compute_logits(&h);
    let d2 = max_abs(&got2, &r.logits);
    eprintln!("backend decode max abs diff = {d2:e}");
    assert!(d2 <= ABSOLUTE_TOLERANCE, "decode max abs diff {d2:e}");
}

/// Negative control: without q_norm/k_norm the logits are far from the reference, so the
/// parity tests above do exercise the norm.
#[test]
fn qwen3_without_qk_norm_is_far_from_reference() {
    let (mut weights, r) = tiny();
    for layer in &mut weights.layers {
        layer.q_norm = None;
        layer.k_norm = None;
    }
    let got = weights
        .forward_sequence_with_diagnostics(&r.ids)
        .activations["last_token_logits"]
        .clone();
    let d = max_abs(&got, &r.logits);
    eprintln!("no-norm max abs diff = {d:e}");
    assert!(
        d > 100.0 * ABSOLUTE_TOLERANCE,
        "norm removal changed logits by only {d:e}"
    );
}

#[test]
fn qwen3_checkpoint_missing_q_norm_is_refused() {
    let dir = fixture("qwen3-tiny");
    let mut config = load_model_config(&dir).unwrap();
    config.qk_norm = false; // catalog without norms: file has extra tensors, still loads as Llama-style
    let llama_style =
        TransformerWeights::load_from_safetensors(dir.join("model.safetensors"), &config);
    // The Qwen3 catalog is a strict superset: a Llama-style config must not silently load a
    // Qwen3 file as correct, so the weights it builds carry no norms.
    if let Ok(w) = llama_style {
        assert!(w
            .layers
            .iter()
            .all(|l| l.q_norm.is_none() && l.k_norm.is_none()));
    }
    let cat = aien_inference_abi::llama_catalog(&load_model_config(&dir).unwrap());
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.1.self_attn.q_norm.weight" && s == &vec![16]));
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.0.self_attn.k_norm.weight" && s == &vec![16]));
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.0.self_attn.q_proj.weight" && s == &vec![64, 32]));
}

fn real_config_json() -> String {
    std::fs::read_to_string(fixture("qwen3-4b-instruct-2507-config").join("config.json")).unwrap()
}

#[test]
fn real_qwen3_4b_instruct_2507_config_is_accepted() {
    let c = model_config_from_hf_json(
        "Qwen/Qwen3-4B-Instruct-2507",
        &real_config_json(),
        Some(
            &std::fs::read_to_string(
                fixture("qwen3-4b-instruct-2507-config").join("generation_config.json"),
            )
            .unwrap(),
        ),
    )
    .expect("accepted");
    assert!(c.qk_norm && c.tie_word_embeddings);
    assert_eq!(
        (c.num_layers, c.num_heads, c.num_kv_heads, c.head_dim),
        (36, 32, 8, 128)
    );
    assert_eq!(
        (c.hidden_dim, c.intermediate_dim, c.vocab_size),
        (2560, 9728, 151936)
    );
    assert_eq!(c.rope_theta, 5_000_000.0);
    assert_eq!(c.max_sequence_length, 262144);
    assert_eq!(c.eos_token_ids, vec![151645, 151643]);
    let cat = aien_inference_abi::llama_catalog(&c);
    // embed + 36 * (norms 2 + qkvo 4 + qk_norm 2 + mlp 3) + final norm, no lm_head (tied).
    assert_eq!(cat.len(), 1 + 36 * 11 + 1);
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.35.self_attn.q_proj.weight" && s == &vec![4096, 2560]));
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.35.self_attn.o_proj.weight" && s == &vec![2560, 4096]));
    assert!(cat
        .iter()
        .any(|(n, s)| n == "model.layers.0.self_attn.q_norm.weight" && s == &vec![128]));
}

#[test]
fn qwen3_unsupported_config_fields_are_refused_by_name() {
    let base: Value = serde_json::from_str(&real_config_json()).unwrap();
    let refuse = |edit: &dyn Fn(&mut serde_json::Map<String, Value>), needle: &str| {
        let mut v = base.clone();
        edit(v.as_object_mut().unwrap());
        let err = model_config_from_hf_json("m", &v.to_string(), None).unwrap_err();
        assert!(err.contains(needle), "error {err:?} should name {needle:?}");
    };
    refuse(
        &|m| {
            m.remove("head_dim");
        },
        "head_dim",
    );
    refuse(
        &|m| {
            m.insert(
                "rope_parameters".into(),
                serde_json::json!({"rope_type": "default"}),
            );
        },
        "rope_parameters",
    );
    refuse(
        &|m| {
            m.insert("some_new_field".into(), Value::Bool(true));
        },
        "some_new_field",
    );
    refuse(
        &|m| {
            m.insert("use_sliding_window".into(), Value::Bool(true));
        },
        "use_sliding_window",
    );
    refuse(
        &|m| {
            m.insert("sliding_window".into(), Value::from(4096));
        },
        "sliding_window",
    );
    refuse(
        &|m| {
            m.insert(
                "layer_types".into(),
                serde_json::json!(["linear_attention"]),
            );
        },
        "layer_types",
    );
    refuse(
        &|m| {
            m.insert("attention_bias".into(), Value::Bool(true));
        },
        "attention_bias",
    );
    refuse(
        &|m| {
            m.insert("hidden_act".into(), Value::from("gelu"));
        },
        "hidden_act",
    );
    refuse(
        &|m| {
            m.insert("model_type".into(), Value::from("qwen2"));
        },
        "model_type",
    );
    refuse(
        &|m| {
            m.insert(
                "rope_scaling".into(),
                serde_json::json!({"rope_type": "yarn", "factor": 4.0}),
            );
        },
        "rope_scaling",
    );
    refuse(
        &|m| {
            m.insert(
                "architectures".into(),
                serde_json::json!(["Qwen3MoeForCausalLM"]),
            );
        },
        "architectures",
    );
}

#[test]
fn llama_head_dim_rule_is_unchanged() {
    // A Llama-architecture config whose heads * head_dim != hidden is still refused.
    let json = r#"{"architectures":["LlamaForCausalLM"],"hidden_size":32,"intermediate_size":64,
      "num_hidden_layers":1,"num_attention_heads":4,"num_key_value_heads":2,"head_dim":16,
      "vocab_size":10,"rms_norm_eps":1e-5,"max_position_embeddings":16,"eos_token_id":2}"#;
    let err = model_config_from_hf_json("m", json, None).unwrap_err();
    assert!(err.contains("must equal hidden_size"), "{err}");
}

#[test]
fn gb10_backend_refuses_qwen3_and_not_llama() {
    let q = model_config_from_hf_json(
        "Qwen/Qwen3-4B-Instruct-2507",
        &real_config_json(),
        Some("{\"eos_token_id\":[151645]}"),
    )
    .unwrap();
    let msg = omega_model_refusal(&q).expect("Qwen3 must be refused on the GB10 engine");
    assert!(
        msg.contains("head_dim 128") && msg.contains("Qwen3"),
        "{msg}"
    );
    let llama = aien_inference_abi::ModelConfig::tinyllama_1_1b();
    assert!(omega_model_refusal(&llama).is_none());
}

#[test]
#[ignore = "needs the real Qwen3-4B-Instruct-2507 weights in AIEN_QWEN3_DIR"]
fn real_qwen3_4b_instruct_2507_matches_transformers() {
    let dir = std::env::var("AIEN_QWEN3_DIR")
        .expect("AIEN_QWEN3_DIR must name a Qwen3-4B-Instruct-2507 model directory");
    let dir = PathBuf::from(dir);
    let r: Value = serde_json::from_str(
        &std::fs::read_to_string(
            fixture("qwen3-4b-instruct-2507-config").join("real_forward_reference.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let ids: Vec<u32> = r["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    let top_ids: Vec<usize> = r["top_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as usize)
        .collect();
    let top_logits: Vec<f32> = r["top_logits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    let config = load_model_config(&dir).expect("config");
    let weights = TransformerWeights::load_from_safetensors(
        dir.join("model.safetensors.index.json"),
        &config,
    )
    .expect("weights");
    let got =
        weights.forward_sequence_with_diagnostics(&ids).activations["last_token_logits"].clone();
    assert_eq!(got.len(), r["vocab"].as_u64().unwrap() as usize);
    let ours: Vec<f32> = top_ids.iter().map(|i| got[*i]).collect();
    let d = max_abs(&ours, &top_logits);
    eprintln!(
        "real model: max abs diff over the reference top-20 = {d:e}, argmax {} vs {}",
        argmax(&got),
        top_ids[0]
    );
    assert_eq!(argmax(&got), top_ids[0]);
    assert!(d <= 5e-3, "max abs diff {d:e}");
}

/// Batch decode (`forward_decode_batch_with_logits`, the third q/k-norm site): two sequences
/// with the reference prompt must reproduce the transformers logits, and a third, different
/// sequence in the same batch must equal its own single-sequence decode.
#[test]
fn qwen3_tiny_batch_decode_matches_transformers_and_single_sequence() {
    let (weights, r) = tiny();
    let n = r.ids.len();
    let other: Vec<u32> = r.ids.iter().rev().copied().collect();

    let mut batch = NativeTransformerBackend::new_reference(weights.clone());
    for (id, prompt) in [(1u64, &r.ids), (2, &r.ids), (3, &other)] {
        batch
            .prefill_sequence(id, &prompt[..n - 1])
            .expect("prefill");
        // The decode step consumes the pending token at position n - 1.
        batch.pending_prefill_token.insert(id, prompt[n - 1]);
    }
    let (_out, logits) = batch
        .forward_decode_batch_with_logits(&[1, 2, 3])
        .expect("batch decode");
    assert_eq!(logits.len(), 3);
    for row in &logits[..2] {
        let d = max_abs(row, &r.logits);
        eprintln!("batch decode vs transformers max abs diff = {d:e}");
        assert!(d <= ABSOLUTE_TOLERANCE, "batch max abs diff {d:e}");
    }

    let mut single = NativeTransformerBackend::new_reference(weights);
    single
        .prefill_sequence(9, &other[..n - 1])
        .expect("prefill");
    let seq = single.sequences.get_mut(&9).unwrap();
    let h = NativeTransformerBackend::forward_token_impl_paged(
        &single.weights,
        &*single.tensor_backend,
        other[n - 1],
        n - 1,
        seq,
        9,
        None,
    );
    let want = single.compute_logits(&h);
    let d = max_abs(&logits[2], &want);
    eprintln!("batch row 3 vs single-sequence max abs diff = {d:e}");
    assert!(
        d <= ABSOLUTE_TOLERANCE,
        "batch vs single max abs diff {d:e}"
    );
    assert!(
        max_abs(&logits[2], &logits[0]) > 1e-3,
        "third sequence must differ from the first"
    );
}
