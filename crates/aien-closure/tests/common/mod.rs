//! Fixture helpers. Receipts are built and sealed with aien-proof's own API, so
//! every id in the store is a real canonical identity.
#![allow(dead_code)]

use aien_closure::digest::source_digest;
use aien_closure::graph::{cargo_packages, graph_of};
use aien_closure::manifest::{self, Manifest};
use aien_closure::verify::{render_lock, LOCK_FILE};
use aien_proof::evidence::{
    receipt_id, store_receipt, Assertion, Mutation, Receipt, Tier, Verdict, SCHEMA, SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

pub fn fixture_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/seeded")
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("aien-closure-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copy the fixture sources only (no manifests, lock, or store).
pub fn copy_sources(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if matches!(
            name.as_str(),
            "closure.toml" | "closure.lock" | "store" | "target"
        ) {
            continue;
        }
        let to = dst.join(&name);
        if e.file_type().unwrap().is_dir() {
            copy_sources(&e.path(), &to);
        } else {
            fs::copy(e.path(), to).unwrap();
        }
    }
}

/// Knobs that change what the seeded receipts claim.
#[derive(Clone)]
pub struct Knobs {
    pub gamma_dirty: bool,
    pub gamma_tier: Tier,
    pub alpha_profile: &'static str,
}

impl Default for Knobs {
    fn default() -> Self {
        Knobs {
            gamma_dirty: false,
            gamma_tier: Tier::HostTest,
            alpha_profile: "host-v1",
        }
    }
}

fn receipt_for(name: &str, source: &str, deps: Vec<String>, tier: Tier, dirty: bool) -> Receipt {
    let mut r = Receipt {
        schema: SCHEMA.to_string(),
        version: SCHEMA_VERSION,
        id: String::new(),
        kind: "component-qualification".to_string(),
        tier,
        result: Verdict::Pass,
        timestamp: 1_786_000_000,
        repo: "https://github.com/aien-dev/aien-sovereign-core".to_string(),
        commit: "a".repeat(40),
        dirty,
        toolchain: "rustc fixture".to_string(),
        procedure: format!("cargo test -p {name}"),
        machine: "host".to_string(),
        env_class: tier.as_str().to_string(),
        input_artifacts: vec![source.to_string()],
        output_artifacts: vec![],
        assertions: vec![Assertion {
            id: "cargo_test_pass".to_string(),
            expected: "pass".to_string(),
            observed: "pass".to_string(),
            pass: true,
            source: "cargo".to_string(),
            note: String::new(),
        }],
        dependencies: deps,
        declared_mutation: Mutation::None,
        observed_mutation: Mutation::None,
        authority: String::new(),
        output_digest: blake3::hash(name.as_bytes()).to_hex().to_string(),
        external_refs: vec![],
        ledger: None,
        lease: None,
        required_features: vec![],
        reserved: String::new(),
    };
    r.id = receipt_id(&r).unwrap();
    r
}

/// Seed manifests, receipts and the lock for the gamma, beta, alpha fixture.
/// Returns component name -> receipt id.
pub fn seed(root: &Path, store: &Path, knobs: &Knobs) -> BTreeMap<String, String> {
    let packages = cargo_packages(root).unwrap();
    let graph = graph_of(&packages);
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut manifests: BTreeMap<String, Manifest> = BTreeMap::new();
    for name in ["gamma", "beta", "alpha"] {
        let pkg = packages.iter().find(|p| p.name == name).unwrap();
        let source = format!("blake3:{}", source_digest(&pkg.dir).unwrap());
        let (tier, dirty, profile) = match name {
            "gamma" => (knobs.gamma_tier, knobs.gamma_dirty, "host-v1"),
            "alpha" => (Tier::HostTest, false, knobs.alpha_profile),
            _ => (Tier::HostTest, false, "host-v1"),
        };
        let deps: BTreeMap<String, Option<String>> = pkg
            .deps
            .iter()
            .map(|d| (d.clone(), Some(ids[d].clone())))
            .collect();
        let dep_ids = deps.values().flatten().cloned().collect();
        let r = receipt_for(name, &source, dep_ids, tier, dirty);
        store_receipt(store, &r).unwrap();
        ids.insert(name.to_string(), r.id.clone());
        let m = Manifest {
            component: name.to_string(),
            profile: profile.to_string(),
            receipt: r.id,
            source,
            deps,
        };
        fs::write(pkg.dir.join(manifest::FILE_NAME), manifest::render(&m)).unwrap();
        manifests.insert(name.to_string(), m);
    }
    fs::write(root.join(LOCK_FILE), render_lock(&manifests, &graph)).unwrap();
    ids
}

/// Seed into a fresh temp copy of the fixture. Returns (root, store, ids).
pub fn seeded(knobs: &Knobs) -> (PathBuf, PathBuf, BTreeMap<String, String>) {
    let root = temp_dir("ws");
    copy_sources(&fixture_src(), &root);
    let store = root.join("store");
    let ids = seed(&root, &store, knobs);
    (root, store, ids)
}

pub fn write(root: &Path, rel: &str, text: &str) {
    fs::write(root.join(rel), text).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}
