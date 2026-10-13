//! Golden vectors: `tests/golden/kv_record_v1.json`. The file pins the
//! namespace derivation, the chunk key chain and the 256-byte header bytes so
//! the native C implementation (aienos N1) can check itself against the same
//! file (CUTS "Pin"). Regenerate deliberately with
//! `AIEN_KV_REGEN_GOLDEN=1 cargo test -p aien-kv-store --test golden_tests regenerate_golden`;
//! then bump `tests/golden/README.md` and the aienos `CONTRACT.lock`.

mod common;

use aien_kv_store::{
    chunk_chain, validate_record, BackendFormat, ChunkKey, KvDtype, LayoutStrides, NamespaceFields,
    RecordHeader, RopeScaling,
};
use common::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/kv_record_v1.json"
);

fn ns_to_json(f: &NamespaceFields) -> Value {
    json!({
        "model_sha256": hex(&f.model_sha256),
        "model_digest_kind": f.model_digest_kind,
        "model_id": f.model_id,
        "num_layers": f.num_layers,
        "num_kv_heads": f.num_kv_heads,
        "head_dim": f.head_dim,
        "block_size": f.block_size,
        "rope_theta_f32_bits": f.rope_theta.to_bits(),
        "rope_scaling": f.rope_scaling.map(|r| json!({
            "factor_f64_bits": r.factor.to_bits(),
            "low_freq_factor_f64_bits": r.low_freq_factor.to_bits(),
            "high_freq_factor_f64_bits": r.high_freq_factor.to_bits(),
            "original_max_position_embeddings": r.original_max_position_embeddings,
        })),
        "qk_norm": f.qk_norm,
        "dtype": f.dtype as u8,
        "layout": {
            "head_stride": f.layout.head_stride,
            "token_stride": f.layout.token_stride,
            "kv_plane_stride": f.layout.kv_plane_stride,
            "layer_stride": f.layout.layer_stride,
            "block_stride": f.layout.block_stride,
            "element_bytes": f.layout.element_bytes,
        },
        "backend_format": f.backend_format as u8,
        "chunk_tokens": f.chunk_tokens,
        "sharing_namespace": f.sharing_namespace,
    })
}

fn u(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or_else(|| panic!("missing u64 {k}"))
}

fn parse_ns(v: &Value) -> NamespaceFields<'_> {
    let l = &v["layout"];
    NamespaceFields {
        model_sha256: unhex32(v["model_sha256"].as_str().unwrap()),
        model_digest_kind: v["model_digest_kind"].as_str().unwrap(),
        model_id: v["model_id"].as_str().unwrap(),
        num_layers: u(v, "num_layers") as u32,
        num_kv_heads: u(v, "num_kv_heads") as u32,
        head_dim: u(v, "head_dim") as u32,
        block_size: u(v, "block_size") as u32,
        rope_theta: f32::from_bits(u(v, "rope_theta_f32_bits") as u32),
        rope_scaling: match &v["rope_scaling"] {
            Value::Null => None,
            r => Some(RopeScaling {
                factor: f64::from_bits(u(r, "factor_f64_bits")),
                low_freq_factor: f64::from_bits(u(r, "low_freq_factor_f64_bits")),
                high_freq_factor: f64::from_bits(u(r, "high_freq_factor_f64_bits")),
                original_max_position_embeddings: u(r, "original_max_position_embeddings"),
            }),
        },
        qk_norm: v["qk_norm"].as_bool().unwrap(),
        dtype: KvDtype::from_u8(u(v, "dtype") as u8).unwrap(),
        layout: LayoutStrides {
            head_stride: u(l, "head_stride"),
            token_stride: u(l, "token_stride"),
            kv_plane_stride: u(l, "kv_plane_stride"),
            layer_stride: u(l, "layer_stride"),
            block_stride: u(l, "block_stride"),
            element_bytes: u(l, "element_bytes"),
        },
        backend_format: BackendFormat::from_u8(u(v, "backend_format") as u8).unwrap(),
        chunk_tokens: u(v, "chunk_tokens") as u32,
        sharing_namespace: v["sharing_namespace"].as_str().unwrap(),
    }
}

fn tokens_of(v: &Value) -> Vec<u32> {
    v["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_u64().unwrap() as u32)
        .collect()
}

fn payload_sha(name: &str, i: usize) -> [u8; 32] {
    Sha256::digest(format!("{name}/{i}").as_bytes()).into()
}

fn written_ns(i: usize) -> u64 {
    1_700_000_000_000_000_000 + i as u64
}

/// Derive every output of one vector from its inputs (the library under test).
fn outputs(name: &str, ns_v: &Value, tokens: &[u32]) -> Value {
    let f = parse_ns(ns_v);
    let ns = f.namespace_id().unwrap();
    let chain = chunk_chain(&ns, f.chunk_tokens, tokens).unwrap();
    let mut chunks = Vec::new();
    for (i, key) in chain.iter().enumerate() {
        let start = i * f.chunk_tokens as usize;
        let end = (start + f.chunk_tokens as usize).min(tokens.len());
        let parent = if i == 0 { None } else { Some(&chain[i - 1]) };
        let h = f
            .chunk_header(
                &ns,
                *key,
                parent,
                start as u32,
                (end - start) as u32,
                written_ns(i),
            )
            .unwrap();
        let rec = RecordHeader {
            chunk: h,
            payload_sha256: payload_sha(name, i),
        };
        chunks.push(json!({
            "index": i,
            "first_position": start,
            "token_count": end - start,
            "parent_key": hex(&h.parent_key),
            "chunk_key": hex(key.as_bytes()),
            "payload_len": h.payload_len,
            "payload_sha256_of_ascii": format!("{name}/{i}"),
            "payload_sha256": hex(&rec.payload_sha256),
            "written_unix_ns": h.written_unix_ns,
            "header_hex": hex(&rec.encode()),
        }));
    }
    json!({ "namespace_id": hex(&ns.0), "chunks": chunks })
}

fn vector_inputs() -> Vec<(&'static str, NamespaceFields<'static>, Vec<u32>)> {
    let mut small_f = sample();
    small_f.block_size = 4;
    small_f.chunk_tokens = 8;
    small_f.layout = LayoutStrides {
        head_stride: 128,
        token_stride: 1024,
        kv_plane_stride: 4096,
        layer_stride: 8192,
        block_stride: 131072,
        element_bytes: 2,
    };

    let mut native = sample();
    native.model_id = "llama-3.2-1b-instruct-q4km";
    native.model_digest_kind = "sha256-gguf";
    native.rope_scaling = None;
    native.dtype = KvDtype::Fp32;
    native.backend_format = BackendFormat::NativeF32DecodeStateV1;
    native.block_size = 16;
    native.chunk_tokens = 64;
    native.layout = LayoutStrides {
        head_stride: 256,
        token_stride: 2048,
        kv_plane_stride: 32768,
        layer_stride: 65536,
        block_stride: 1048576,
        element_bytes: 4,
    };

    let mut qk = small_f;
    qk.model_id = "qwen3-4b-instruct-2507";
    qk.rope_scaling = None;
    qk.qk_norm = true;
    qk.sharing_namespace = "";

    let real_tokens: Vec<u32> = (0..300u32).map(|i| (i * 37 + 11) % 128_256).collect();
    vec![
        ("host-small-partial-tail", small_f, (0..20).collect()),
        ("host-llama32-1b-full-and-partial", sample(), real_tokens),
        ("native-f32-partial-tail", native, (1000..1070).collect()),
        (
            "qk-norm-empty-sharing-one-full-chunk",
            qk,
            (50..58).collect(),
        ),
    ]
}

#[test]
fn regenerate_golden() {
    if std::env::var("AIEN_KV_REGEN_GOLDEN").as_deref() != Ok("1") {
        return;
    }
    let vectors: Vec<Value> = vector_inputs()
        .into_iter()
        .map(|(name, f, tokens)| {
            let ns_v = ns_to_json(&f);
            let out = outputs(name, &ns_v, &tokens);
            json!({
                "name": name,
                "namespace": ns_v,
                "tokens": tokens,
                "expected": out,
            })
        })
        .collect();
    let doc = json!({
        "format_version": 1,
        "about": "ADR 0003 sections 1 and 2.1. namespace_id = SHA-256('AIEN-PKV-NS-V1\\0' || fields); chunk_key = SHA-256('AIEN-PKV-CHUNK-V1\\0' || namespace_id || parent_key || first_position u32le || token_count u32le || tokens u32le). header_hex is the 256-byte record header. payload_sha256 is SHA-256 of the ASCII string payload_sha256_of_ascii. f32/f64 inputs are given as IEEE bit patterns.",
        "vectors": vectors,
    });
    let mut text = serde_json::to_string_pretty(&doc).unwrap();
    text.push('\n');
    std::fs::write(GOLDEN, text).unwrap();
}

#[test]
fn identity_vectors_match_golden() {
    let text = std::fs::read_to_string(GOLDEN).expect("golden file present");
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["format_version"], 1);
    let vectors = doc["vectors"].as_array().unwrap();
    assert!(
        vectors.len() >= 4,
        "golden file must carry the four vectors"
    );
    let mut checked_headers = 0;
    for v in vectors {
        let name = v["name"].as_str().unwrap();
        let tokens = tokens_of(v);
        let got = outputs(name, &v["namespace"], &tokens);
        assert_eq!(
            got["namespace_id"], v["expected"]["namespace_id"],
            "{name}: namespace_id"
        );
        let want_chunks = v["expected"]["chunks"].as_array().unwrap();
        let got_chunks = got["chunks"].as_array().unwrap();
        assert!(!want_chunks.is_empty(), "{name}: no chunks");
        assert_eq!(
            got_chunks, want_chunks,
            "{name}: chunk keys or header bytes differ"
        );

        // The stored header bytes decode back to the same fields and validate.
        let f = parse_ns(&v["namespace"]);
        let ns = f.namespace_id().unwrap();
        for c in want_chunks {
            let bytes = unhex(c["header_hex"].as_str().unwrap());
            let rec = RecordHeader::decode(&bytes).unwrap();
            assert_eq!(
                rec.encode().as_slice(),
                bytes.as_slice(),
                "{name}: re-encode"
            );
            assert_eq!(rec.chunk.namespace_id, ns);
            assert_eq!(
                rec.chunk.chunk_key,
                ChunkKey(aien_platform::ObjectId(unhex32(
                    c["chunk_key"].as_str().unwrap()
                )))
            );
            let file_size = 256 + c["payload_len"].as_u64().unwrap();
            validate_record(&bytes, file_size, &rec.chunk).unwrap();
            checked_headers += 1;
        }
    }
    assert!(checked_headers >= 8);
}
