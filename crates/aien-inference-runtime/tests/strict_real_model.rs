//! GB10-4: strict real-model execution gate (AIEN 0.1 commissioning).
//!
//! Non-ignored tests prove the production build fails closed: no checkpoint
//! on disk means a refusal, not reference weights. The ignored gate runs the
//! real TinyLlama checkpoint on the GB10 backend and writes the acceptance
//! receipt (checkpoint sha256, tokenizer sha256, backend identity, model
//! config, fallback_count). Run on the Spark:
//!   AIEN_E2E_CHECKPOINT=~/models/TinyLlama-1.1B-Chat-v1.0 \
//!   AIEN_STRICT_RECEIPT=/path/receipt.json \
//!   cargo test -p aien-inference-runtime --release --test strict_real_model -- --ignored --nocapture

use aien_inference_abi::strict::{self, StrictModelReceipt};
use aien_inference_abi::ExecutionSurface;
use aien_inference_runtime::model::EmbeddedModel;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha256_file_hex(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut h = Sha256::new();
    h.update(&bytes);
    format!("{:x}", h.finalize())
}

#[test]
fn production_build_is_strict_unless_opted_in() {
    if std::env::var(strict::DEV_FALLBACK_ENV).is_ok() {
        eprintln!(
            "dev fallback opted in by {}; strictness not asserted",
            strict::DEV_FALLBACK_ENV
        );
        return;
    }
    assert!(
        strict::production_strict(),
        "this build has dev-fallback on; the strict gate must run on a production build"
    );
}

#[test]
fn missing_checkpoint_is_refused_not_faked() {
    let missing = PathBuf::from("/nonexistent/aien-strict-gate/model.safetensors");
    let tok = PathBuf::from("/nonexistent/aien-strict-gate/tokenizer.json");
    let err = EmbeddedModel::load_checkpoint(&missing, &tok, true, true)
        .err()
        .expect("a missing checkpoint must be an error");
    assert!(
        !err.contains("reference"),
        "refusal must not route to reference weights: {err}"
    );
}

#[test]
fn receipt_rejects_fallbacks_and_dev_builds() {
    let mut r = StrictModelReceipt {
        checkpoint_path: "x".into(),
        checkpoint_sha256: "0".repeat(64),
        tokenizer_path: "y".into(),
        tokenizer_sha256: "0".repeat(64),
        backend_identity: "BlackwellGb10Backend (NVIDIA GB10 sm_121 cuBLAS)".into(),
        model_id: "tinyllama".into(),
        model_config: serde_json::json!({}),
        fallback_count: 0,
        op_report: None,
        dev_fallback_build: false,
        verdict: String::new(),
    };
    assert!(r.verify().is_ok(), "{}", r.verdict);
    r.fallback_count = 1;
    assert!(r.verify().is_err());
    r.fallback_count = 0;
    r.dev_fallback_build = true;
    assert!(r.verify().is_err());
    r.dev_fallback_build = false;
    r.backend_identity = "ReferenceCpuBackend".into();
    assert!(r.verify().is_err());
}

#[test]
#[ignore = "needs the real checkpoint on the Spark: AIEN_E2E_CHECKPOINT + AIEN_STRICT_RECEIPT"]
fn strict_real_model_gate() {
    let ckpt = PathBuf::from(std::env::var("AIEN_E2E_CHECKPOINT").expect("AIEN_E2E_CHECKPOINT"));
    let receipt_path =
        PathBuf::from(std::env::var("AIEN_STRICT_RECEIPT").expect("AIEN_STRICT_RECEIPT"));
    let (model_path, dir) = if ckpt.is_dir() {
        (ckpt.join("model.safetensors"), ckpt.clone())
    } else {
        (
            ckpt.clone(),
            ckpt.parent().map(Path::to_path_buf).unwrap_or_default(),
        )
    };
    let tokenizer_path = dir.join("tokenizer.json");
    assert!(
        model_path.is_file(),
        "STRICT: checkpoint {} missing",
        model_path.display()
    );
    assert!(
        tokenizer_path.is_file(),
        "STRICT: tokenizer {} missing",
        tokenizer_path.display()
    );

    let surface = ExecutionSurface::detect();
    println!("STRICT_GATE surface: {}", surface.display_name());
    let mut model = EmbeddedModel::load_checkpoint(&model_path, &tokenizer_path, true, true)
        .unwrap_or_else(|e| panic!("STRICT: bind refused: {e}"));
    let backend_identity = model.transformer.tensor_backend.name().to_string();
    println!("STRICT_GATE backend: {backend_identity}");

    let prompt = "The DGX Spark is a small computer with a large";
    let out = model
        .generate(prompt, 8, 0.0)
        .unwrap_or_else(|e| panic!("STRICT: generate failed: {e}"));
    println!("STRICT_GATE output: {out:?}");
    let fallback_count = model.transformer.tensor_backend.fallback_count();

    println!(
        "STRICT_GATE {}",
        model.transformer.tensor_backend.op_report().line()
    );
    let mut receipt = StrictModelReceipt {
        checkpoint_path: model_path.display().to_string(),
        checkpoint_sha256: sha256_file_hex(&model_path),
        tokenizer_path: tokenizer_path.display().to_string(),
        tokenizer_sha256: sha256_file_hex(&tokenizer_path),
        backend_identity,
        model_id: model.config.model_id.clone(),
        model_config: serde_json::to_value(&model.config)
            .unwrap_or(serde_json::json!({"model_id": model.config.model_id})),
        fallback_count,
        op_report: Some(model.transformer.tensor_backend.op_report()),
        dev_fallback_build: strict::dev_fallback_active(),
        verdict: String::new(),
    };
    let verdict = receipt.verify();
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .expect("write receipt");
    println!(
        "STRICT_GATE verdict: {} (receipt {})",
        receipt.verdict,
        receipt_path.display()
    );
    assert!(!out.trim().is_empty(), "STRICT: empty output");
    verdict.unwrap_or_else(|e| panic!("{e}"));
}
