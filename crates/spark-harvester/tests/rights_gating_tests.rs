use aien_provenance::{GrantPermissions, ProvenanceError, SourceGrant};
use p256::ecdsa::SigningKey;
use spark_harvester::{BundleKind, ExtractedPair, RightsGate};
use std::io::ErrorKind;

fn create_test_pair() -> ExtractedPair {
    ExtractedPair {
        prompt: "Explain copy-on-write paged attention".to_string(),
        reasoning: Some("Analyze memory refcounting and block sharing mechanics".to_string()),
        completion: "CoW allocates physical pages only when divergent tokens are written to a shared block.".to_string(),
        provider: "sovereign-lab".to_string(),
        model: "atlas-omni-reasoning".to_string(),
        token_count: 32,
        quality_score: 0.98,
    }
}

#[test]
fn test_rights_gated_training_bundle_admitted_with_valid_grant() {
    let grant = SourceGrant::new(
        "https://hf.co/datasets/aien/sovereign-math".to_string(),
        "Apache-2.0".to_string(),
        GrantPermissions::TRAIN | GrantPermissions::DISTILL,
        "Atlas Rights Authority".to_string(),
    );

    let pairs = vec![create_test_pair()];
    let bundle = RightsGate::validate_and_bundle(
        &grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
    )
    .expect("Valid training grant must be accepted");

    assert_eq!(bundle.bundle_kind, BundleKind::Training);
    assert_eq!(bundle.records.len(), 1);
    assert_ne!(bundle.merkle_root, aien_protocol_types::Digest32::ZERO);
    assert_eq!(bundle.records[0].source_uri, "https://hf.co/datasets/aien/sovereign-math");
}

#[test]
fn test_unapproved_license_rejected() {
    let grant = SourceGrant::new(
        "https://example.com/proprietary-dataset".to_string(),
        "CC-BY-NC-4.0".to_string(),
        GrantPermissions::TRAIN,
        "Untrusted Third Party".to_string(),
    );

    let pairs = vec![create_test_pair()];
    let res = RightsGate::validate_and_bundle(
        &grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
    );

    match res {
        Err(ProvenanceError::IncompatibleLicense { license }) => {
            assert_eq!(license, "CC-BY-NC-4.0");
        }
        other => panic!("Expected IncompatibleLicense, got {:?}", other),
    }
}

#[test]
fn test_missing_permission_rejected() {
    let grant = SourceGrant::new(
        "https://hf.co/datasets/aien/eval-set".to_string(),
        "MIT".to_string(),
        GrantPermissions::EVALUATE, // Only EVALUATE, not TRAIN
        "Atlas Rights Authority".to_string(),
    );

    let pairs = vec![create_test_pair()];
    let res = RightsGate::validate_and_bundle(
        &grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
    );

    match res {
        Err(ProvenanceError::PermissionDenied { required, granted }) => {
            assert_eq!(required, GrantPermissions::TRAIN);
            assert_eq!(granted, GrantPermissions::EVALUATE);
        }
        other => panic!("Expected PermissionDenied, got {:?}", other),
    }
}

#[test]
fn test_expired_grant_rejected() {
    let mut grant = SourceGrant::new(
        "https://hf.co/datasets/aien/expired-set".to_string(),
        "MIT".to_string(),
        GrantPermissions::TRAIN,
        "Atlas Rights Authority".to_string(),
    );
    grant.expires_at_epoch_sec = Some(1000); // Far in the past

    let pairs = vec![create_test_pair()];
    let res = RightsGate::validate_and_bundle(
        &grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
    );

    match res {
        Err(ProvenanceError::GrantExpired(expires_at)) => {
            assert_eq!(expires_at, 1000);
        }
        other => panic!("Expected GrantExpired, got {:?}", other),
    }
}

#[test]
fn test_cryptographic_signature_verification() {
    let signing_key = SigningKey::from_slice(&[33u8; 32]).expect("valid key");
    let verifying_key = *signing_key.verifying_key();

    let mut grant = SourceGrant::new(
        "https://hf.co/datasets/aien/signed-set".to_string(),
        "MIT".to_string(),
        GrantPermissions::TRAIN,
        "Atlas Rights Authority".to_string(),
    );
    grant.sign(&signing_key);

    let pairs = vec![create_test_pair()];
    let bundle = RightsGate::validate_and_bundle_with_key(
        &grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
        Some(&verifying_key),
    )
    .expect("Signed grant must be accepted");
    assert_eq!(bundle.records.len(), 1);

    // Tampering with grant fails signature verification
    let mut tampered_grant = grant.clone();
    tampered_grant.license_spdx = "Apache-2.0".to_string();
    let tamper_res = RightsGate::validate_and_bundle_with_key(
        &tampered_grant,
        &pairs,
        BundleKind::Training,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
        Some(&verifying_key),
    );
    match tamper_res {
        Err(ProvenanceError::SignatureVerificationFailed(_)) => {}
        other => panic!("Expected SignatureVerificationFailed, got {:?}", other),
    }
}

#[test]
fn test_evidence_bundle_quarantine_enforced() {
    let grant = SourceGrant::new(
        "https://hf.co/datasets/aien/benchmark-evidence".to_string(),
        "MIT".to_string(),
        GrantPermissions::EVALUATE,
        "Atlas Rights Authority".to_string(),
    );

    let pairs = vec![create_test_pair()];
    let bundle = RightsGate::validate_and_bundle(
        &grant,
        &pairs,
        BundleKind::Evidence,
        &["Apache-2.0", "MIT", "SRCL-1.0"],
    )
    .expect("Evidence grant must be accepted");

    let tmp = tempfile::tempdir().unwrap();

    // 1. Attempting to export evidence bundle to training path fails closed
    let train_path = tmp.path().join("train_dataset.jsonl");
    let err = RightsGate::export_bundle(&bundle, &train_path).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::PermissionDenied);

    let sft_path = tmp.path().join("sft.jsonl");
    let err_sft = RightsGate::export_bundle(&bundle, &sft_path).unwrap_err();
    assert_eq!(err_sft.kind(), ErrorKind::PermissionDenied);

    let distill_path = tmp.path().join("distill_dataset.jsonl");
    let err_distill = RightsGate::export_bundle(&bundle, &distill_path).unwrap_err();
    assert_eq!(err_distill.kind(), ErrorKind::PermissionDenied);

    // 2. Exporting to path without quarantine/evidence directory or name fails closed
    let general_path = tmp.path().join("output.jsonl");
    let err_general = RightsGate::export_bundle(&bundle, &general_path).unwrap_err();
    assert_eq!(err_general.kind(), ErrorKind::PermissionDenied);

    // 3. Exporting to quarantine evidence path succeeds
    let evidence_path = tmp.path().join("evidence_audit.jsonl");
    let count = RightsGate::export_bundle(&bundle, &evidence_path).expect("Evidence export must succeed");
    assert_eq!(count, 1);
    assert!(evidence_path.exists());

    let quarantine_dir = tmp.path().join("quarantine");
    std::fs::create_dir_all(&quarantine_dir).unwrap();
    let q_path = quarantine_dir.join("records.jsonl");
    let q_count = RightsGate::export_bundle(&bundle, &q_path).expect("Quarantine export must succeed");
    assert_eq!(q_count, 1);
    assert!(q_path.exists());
}
