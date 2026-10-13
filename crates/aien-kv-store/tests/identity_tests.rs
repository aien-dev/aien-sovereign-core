// Test cases adapted (re-expressed in Rust, none copied) from LMCache
// tests/v1/distributed/test_nixl_store_dynamic_l2_adapter.py at 4d69cdc4,
// Apache-2.0, Copyright LMCache contributors:
//   test_salted_objects_recover_independently_after_restart (line 745)
// See THIRD_PARTY.md and ADR 0003 context item 24.

mod common;

use aien_kv_store::{
    chunk_chain, chunk_key, BackendFormat, KvDtype, KvError, NamespaceFields, NamespaceId,
    RopeScaling,
};
use common::*;

type Mutator = (&'static str, fn(&mut NamespaceFields<'static>));

fn mutators() -> Vec<Mutator> {
    vec![
        ("model_sha256", |f| f.model_sha256[31] ^= 1),
        ("model_digest_kind", |f| f.model_digest_kind = "blake3-file"),
        ("model_id", |f| f.model_id = "llama-3.2-3b-instruct"),
        ("num_layers", |f| f.num_layers += 1),
        ("num_kv_heads", |f| f.num_kv_heads += 1),
        ("head_dim", |f| f.head_dim += 1),
        ("block_size", |f| f.block_size = 8),
        ("rope_theta", |f| f.rope_theta = 10000.0),
        ("rope_scaling_none", |f| f.rope_scaling = None),
        ("rope_factor", |f| {
            f.rope_scaling.as_mut().unwrap().factor = 8.0
        }),
        ("rope_low", |f| {
            f.rope_scaling.as_mut().unwrap().low_freq_factor = 2.0
        }),
        ("rope_high", |f| {
            f.rope_scaling.as_mut().unwrap().high_freq_factor = 8.0
        }),
        ("rope_original_max", |f| {
            f.rope_scaling
                .as_mut()
                .unwrap()
                .original_max_position_embeddings = 4096
        }),
        ("qk_norm", |f| f.qk_norm = true),
        ("dtype", |f| f.dtype = KvDtype::Fp16),
        ("layout_head_stride", |f| f.layout.head_stride += 2),
        ("layout_token_stride", |f| f.layout.token_stride += 2),
        ("layout_kv_plane_stride", |f| f.layout.kv_plane_stride += 2),
        ("layout_layer_stride", |f| f.layout.layer_stride += 2),
        ("layout_block_stride", |f| f.layout.block_stride += 2),
        ("layout_element_bytes", |f| f.layout.element_bytes = 4),
        ("backend_format", |f| {
            f.backend_format = BackendFormat::NativeF32DecodeStateV1
        }),
        ("chunk_tokens", |f| f.chunk_tokens = 512),
        ("sharing_namespace", |f| f.sharing_namespace = "tenant-b"),
    ]
}

#[test]
fn namespace_changes_when_any_field_changes() {
    let base = sample().namespace_id().unwrap();
    let mut seen: Vec<(&str, NamespaceId)> = vec![("base", base)];
    for (name, mutate) in mutators() {
        let mut f = sample();
        mutate(&mut f);
        let id = f.namespace_id().unwrap();
        for (other, other_id) in &seen {
            assert_ne!(id, *other_id, "{name} collides with {other}");
        }
        seen.push((name, id));
    }
    // Same inputs, same id (deterministic).
    assert_eq!(base, sample().namespace_id().unwrap());
    // Every ADR 1.2 field is covered: 13 field rows expand to 24 mutators.
    assert_eq!(seen.len(), 25);
}

#[test]
fn rope_scaling_presence_is_distinct_from_zeroed_scaling() {
    let mut a = sample();
    a.rope_scaling = None;
    let mut b = sample();
    b.rope_scaling = Some(RopeScaling {
        factor: 0.0,
        low_freq_factor: 0.0,
        high_freq_factor: 0.0,
        original_max_position_embeddings: 0,
    });
    assert_ne!(a.namespace_id().unwrap(), b.namespace_id().unwrap());
}

#[test]
fn namespace_refuses_oversized_or_inconsistent_fields() {
    let mut f = sample();
    let long = "x".repeat(65);
    // Leak is fine in a test: NamespaceFields borrows with a 'static lifetime here.
    f.sharing_namespace = Box::leak(long.into_boxed_str());
    assert_eq!(
        f.namespace_id(),
        Err(KvError::FieldTooLong("sharing_namespace"))
    );

    let mut f = sample();
    f.model_digest_kind = Box::leak("k".repeat(256).into_boxed_str());
    assert_eq!(
        f.namespace_id(),
        Err(KvError::FieldTooLong("model_digest_kind"))
    );

    let mut f = sample();
    f.chunk_tokens = 250; // not a multiple of block_size 16
    assert_eq!(f.namespace_id(), Err(KvError::InvalidGeometry));
    let mut f = sample();
    f.block_size = 0;
    assert_eq!(f.namespace_id(), Err(KvError::InvalidGeometry));
    let mut f = sample();
    f.chunk_tokens = 0;
    assert_eq!(f.namespace_id(), Err(KvError::InvalidGeometry));
}

#[test]
fn chunk_key_chain_depends_on_parent_and_position() {
    let ns = small().namespace_id().unwrap();
    let toks: Vec<u32> = (100..108).collect();
    let k0 = chunk_key(&ns, None, 0, &toks).unwrap();

    // Parent matters.
    let k_parent_a = chunk_key(&ns, Some(&key(1)), 8, &toks).unwrap();
    let k_parent_b = chunk_key(&ns, Some(&key(2)), 8, &toks).unwrap();
    assert_ne!(k_parent_a, k_parent_b);
    // Position matters even with identical tokens and parent.
    let k_pos_8 = chunk_key(&ns, Some(&key(1)), 8, &toks).unwrap();
    let k_pos_16 = chunk_key(&ns, Some(&key(1)), 16, &toks).unwrap();
    assert_ne!(k_pos_8, k_pos_16);
    // Namespace matters.
    let other_ns = {
        let mut f = small();
        f.sharing_namespace = "tenant-b";
        f.namespace_id().unwrap()
    };
    assert_ne!(k0, chunk_key(&other_ns, None, 0, &toks).unwrap());
    // Tokens matter, and so does token count (partial vs full prefix).
    let mut toks2 = toks.clone();
    toks2[7] += 1;
    assert_ne!(k0, chunk_key(&ns, None, 0, &toks2).unwrap());
    assert_ne!(k0, chunk_key(&ns, None, 0, &toks[..7]).unwrap());
    // First chunk uses an all-zero parent.
    assert_eq!(
        k0,
        chunk_key(
            &ns,
            Some(&aien_kv_store::ChunkKey(aien_platform::ObjectId([0; 32]))),
            0,
            &toks
        )
        .unwrap()
    );
}

#[test]
fn chunk_chain_links_each_chunk_to_all_earlier_tokens() {
    let ns = small().namespace_id().unwrap();
    let toks: Vec<u32> = (0..20).collect(); // chunks of 8: 8, 8, 4 (partial tail)
    let chain = chunk_chain(&ns, 8, &toks).unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0], chunk_key(&ns, None, 0, &toks[0..8]).unwrap());
    assert_eq!(
        chain[1],
        chunk_key(&ns, Some(&chain[0]), 8, &toks[8..16]).unwrap()
    );
    assert_eq!(
        chain[2],
        chunk_key(&ns, Some(&chain[1]), 16, &toks[16..20]).unwrap()
    );

    // Changing one token in chunk 0 changes every later key.
    let mut changed = toks.clone();
    changed[0] = 999;
    let chain2 = chunk_chain(&ns, 8, &changed).unwrap();
    assert!(chain.iter().zip(&chain2).all(|(a, b)| a != b));

    // Changing a token in the tail only changes the tail.
    let mut tail = toks.clone();
    tail[19] = 999;
    let chain3 = chunk_chain(&ns, 8, &tail).unwrap();
    assert_eq!(chain[..2], chain3[..2]);
    assert_ne!(chain[2], chain3[2]);

    // A longer prompt shares the full-chunk prefix keys with a shorter one.
    let longer: Vec<u32> = (0..28).collect();
    let chain4 = chunk_chain(&ns, 8, &longer).unwrap();
    assert_eq!(chain[..2], chain4[..2]);
    // The 4-token tail of the short prompt does not match a full 8-token chunk.
    assert_ne!(chain[2], chain4[2]);

    assert!(chunk_chain(&ns, 8, &[]).unwrap().is_empty());
    assert_eq!(chunk_chain(&ns, 0, &toks), Err(KvError::InvalidGeometry));
}

/// Adapted idea: LMCache `test_salted_objects_recover_independently_after_restart`.
/// Two salts over the same tokens never share keys, and recomputing after a
/// "restart" (fresh derivation from scratch) reproduces each salt's own keys.
#[test]
fn salted_namespaces_stay_independent_after_recomputation() {
    let toks: Vec<u32> = (0..16).collect();
    let derive = |salt: &'static str| {
        let mut f = small();
        f.sharing_namespace = salt;
        let ns = f.namespace_id().unwrap();
        chunk_chain(&ns, 8, &toks).unwrap()
    };
    let a1 = derive("salt-a");
    let b1 = derive("salt-b");
    let a2 = derive("salt-a");
    let b2 = derive("salt-b");
    assert_eq!(a1, a2);
    assert_eq!(b1, b2);
    for (x, y) in a1.iter().zip(&b1) {
        assert_ne!(x, y);
    }
}
