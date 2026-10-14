//! Chip-only acceptance check of persistent KV on the omega engine (sc#505, locked by #499).
//! NOT run by `cargo test`: it loads the 30B checkpoint onto the chip. Run it only through
//! `scripts/own30b-pkv-chip-check.sh` (which builds natively and checks the chip is free):
//!
//!   OWN30B_MODEL_DIR=... cargo test --release -p aien-serve-http --test kv_persist_chip -- --ignored
//!
//! In one process: turn 1, wait for the parks, turn 2 with the RAM prefix (the uninterrupted
//! run), then forget the RAM KV and run turn 2 again so it must restore from the store. The two
//! turn 2 replies and the K/V the engine holds afterwards must be identical, byte for byte.
//! The store key is generated in memory for the run; the store lives in a private temp dir.
//! Takes the hive GPU lock (/tmp/aien-gb10.lock) around each generation, as the server does.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use aien_inference_abi::qwen3_moe_omega::WeightFormat;
use aien_serve_http::engine::{Engine, GenerateParams};
use aien_serve_http::kv_persist::{self, OmegaBuild, PersistState};
use aien_serve_http::omega_engine::{OmegaDoor, PrefillMode};

fn gen_key() -> String {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut b)
        .unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn turn(d: &OmegaDoor, prompt: &[u32], n: usize) -> Vec<u32> {
    let p = GenerateParams {
        max_new_tokens: n,
        temperature: 0.0,
        top_p: 1.0,
        top_k: 0,
        seed: 1,
        stop_token_ids: vec![],
    };
    let mut out = Vec::new();
    d.generate(prompt, &p, &mut |t| {
        out.push(t);
        true
    })
    .unwrap();
    out
}

#[test]
#[ignore = "chip-only: loads the 30B checkpoint on omega; run via scripts/own30b-pkv-chip-check.sh"]
fn restored_turn_two_equals_the_uninterrupted_turn_two_on_the_chip() {
    let model = PathBuf::from(std::env::var("OWN30B_MODEL_DIR").expect("OWN30B_MODEL_DIR"));
    let scratch = std::env::temp_dir().join(format!("pkv505-chip-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut door = OmegaDoor::load(
        &model,
        8192,
        WeightFormat::Fp8All,
        Some(2000),
        PrefillMode::default(),
    )
    .expect("omega load");
    let store = scratch.join("store");
    let key = gen_key();
    let settings = kv_persist::settings_from_lookup(|n| match n {
        "AIEN_KV_STORE_DIR" => Some(store.display().to_string()),
        "AIEN_KV_STORE_KEY" => Some(key.clone()),
        _ => None,
    })
    .unwrap();
    let env: Vec<(String, String)> = std::env::vars().collect();
    let (sha, kind) = kv_persist::checkpoint_digest(&model).unwrap();
    let identity = kv_persist::OmegaKvIdentity::new(&kv_persist::IdentityInputs {
        model_id: "own30b-chip-check",
        model_sha256: sha,
        model_digest_kind: kind,
        config: &door.config(),
        rope_scaling: kv_persist::config_has_rope_scaling(&model).unwrap(),
        weights: WeightFormat::Fp8All,
        switches: door.kv_switches(),
        build: &OmegaBuild::linked(),
        env: &env,
    });
    let (state, line) = kv_persist::open(settings, identity);
    eprintln!("{line}");
    let PersistState::On(_) = &state else {
        panic!("persistence refused: {line}")
    };
    door.set_persistence(state);
    let pk = match door.persistence() {
        PersistState::On(p) => p,
        _ => unreachable!(),
    };
    assert!(pk.wait_ready(Duration::from_secs(60)));

    // A 600-token prompt (the same sentence ids repeated), then a continuation.
    let p1: Vec<u32> = (0..600u32).map(|i| 1000 + (i * 37) % 5000).collect();
    let _ = turn(&door, &p1, 32);
    assert!(
        pk.wait_idle(Duration::from_secs(60)),
        "parks did not finish"
    );
    let h1 = door.history();
    let mut p2 = h1.clone();
    p2.extend([2000u32, 2001, 2002, 2003, 2004]);

    // Freeze the store as it was after turn 1 (the state a restarted server would find).
    pk.set_park(false);
    let t_u = turn(&door, &p2, 64);
    let reused_u = door.reused_last();
    let h_u = door.history();
    let kv_u = door.export_kv_for_check(h_u.len()).unwrap();

    door.reset_kv();
    let t_r = turn(&door, &p2, 64);
    let c = pk.counters();
    eprintln!("counters after the restored turn: {c:?}");
    assert_eq!(c.restores, 1, "turn 2 did not restore: {c:?}");
    assert_eq!(
        c.restored_tokens as usize, reused_u,
        "restored up to the uninterrupted reuse point"
    );
    assert_eq!(door.reused_last(), reused_u);
    assert_eq!(
        t_r, t_u,
        "restored continuation differs from the uninterrupted run"
    );
    assert_eq!(door.history(), h_u);
    let kv_r = door.export_kv_for_check(h_u.len()).unwrap();
    assert!(
        kv_r == kv_u,
        "the K/V after the restored turn differs from the uninterrupted one"
    );
    drop(door);
    let _ = std::fs::remove_dir_all(&scratch);
}
