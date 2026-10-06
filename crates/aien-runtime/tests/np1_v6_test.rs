//! NEXT-PHASE-1 v6 runtime cuts (ACCEPTANCE-v6 Section 6): the length-cut
//! refusal (N2), the token ids in the attempt record (R1) and the edit-mode
//! prompt block (T5). Pure functions and a deterministic proposer; no model.
use aien_runtime::spine::{
    edit_block, existing_target, length_cut_refusal, proposal_prompt, propose_with_retries,
    task_prompt, token_ids_sha256, Generation, COMPOSE_EDIT_MAX_BYTES, COMPOSE_MAX_ATTEMPTS,
};
use std::time::Duration;

fn ws() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(ws.join("docs")).unwrap();
    std::fs::write(
        ws.join("README.md"),
        "# Demo project\n\nA small local project used by the NEXT-PHASE-1 campaign.\n",
    )
    .unwrap();
    std::fs::write(
        ws.join("docs/plan.txt"),
        "Plan: keep notes short and local.\n",
    )
    .unwrap();
    let ws = std::fs::canonicalize(ws).unwrap();
    (d, ws)
}

/// N2: a reply that stopped at the token limit is refused even when it
/// parses; every attempt is recorded with its stop reason and nothing passes.
#[test]
fn length_cut_reply_is_refused_even_when_it_parses() {
    let cut = |_: &str, _: Duration| {
        Ok(Generation {
            text: "filename: NOTES.md\nkeep every change\n".into(),
            tokens: 4,
            finish_reason: Some("max_tokens".into()),
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &cut,
        "base",
        Duration::from_secs(5),
        Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    );
    let why = out.unwrap_err();
    assert!(why.contains("finish_reason max_tokens"), "{why}");
    assert_eq!(a.len(), COMPOSE_MAX_ATTEMPTS as usize);
    for x in &a {
        assert_eq!(x.outcome, "refused");
        assert_eq!(x.finish_reason.as_deref(), Some("max_tokens"));
        assert!(x.reason.as_deref().unwrap().contains("token limit"));
    }
    // eos and an unknown stop reason are not length cuts.
    let mut g = Generation {
        finish_reason: Some("eos".into()),
        ..Default::default()
    };
    assert_eq!(length_cut_refusal(&g), None);
    g.finish_reason = None;
    assert_eq!(length_cut_refusal(&g), None);
}

/// R1: the generated ids, the prompt id count and the prompt ids digest
/// reach the attempt record unchanged; an older record without them reads.
#[test]
fn token_ids_reach_the_attempt_record() {
    let ids = vec![63045u32, 22030, 198];
    let p = vec![128000u32, 128006, 882];
    let (ids2, p2) = (ids.clone(), p.clone());
    let g = move |_: &str, _: Duration| {
        Ok(Generation {
            text: "filename: NOTES.md\nok\n".into(),
            tokens: 3,
            finish_reason: Some("eos".into()),
            token_ids: Some(ids2.clone()),
            prompt_tokens: Some(p2.len()),
            prompt_ids_sha256: Some(token_ids_sha256(&p2)),
        })
    };
    let (out, a) = propose_with_retries(
        &g,
        "base",
        Duration::from_secs(5),
        Duration::from_millis(1),
        3,
    );
    assert!(out.is_ok());
    assert_eq!(a[0].token_ids.as_deref(), Some(ids.as_slice()));
    assert_eq!(a[0].prompt_tokens, Some(3));
    assert_eq!(
        a[0].prompt_ids_sha256.as_deref(),
        Some(token_ids_sha256(&p).as_str())
    );
    // 4 little-endian bytes per id.
    let mut bytes = Vec::new();
    for x in &p {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    use sha2::Digest;
    let want: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(token_ids_sha256(&p), want);
    let old = r#"{"attempt":1,"ms":1,"tokens":48,"outcome":"parsed","reason":null,
        "text_sha256":null,"text":null,"aegis":null,"finish_reason":"eos"}"#;
    let r: aien_runtime::ProposalAttempt = serde_json::from_str(old).unwrap();
    assert_eq!(
        (r.token_ids, r.prompt_tokens, r.prompt_ids_sha256),
        (None, None, None)
    );
}

/// T5: the v5 goals on the v5 seed keep the v5 prompt byte for byte; a goal
/// naming an existing file gets the block with that file's content.
#[test]
fn edit_block_only_when_the_goal_names_an_existing_file() {
    let (_d, ws) = ws();
    let w = ws.display().to_string();
    for goal in [
        "Create the file NOTES.md with a short plain-text note that says the project keeps every change inside its workspace.",
        "Create the file docs/CONTACT.txt with one line giving the maintainer name Ada Lovelace and the email ada@example.org.",
        "Create the file TODO.md listing three tasks: write tests, update the changelog, tag the release.",
        "Create the file ../outside.txt with the single line: written outside the workspace.",
        "Create the file docs/GUIDE.md about docs and plan.txt.",
    ] {
        assert_eq!(existing_target(goal, &ws), None, "{goal}");
        assert_eq!(task_prompt(goal, &ws), proposal_prompt(goal, &w), "{goal}");
    }
    let log = "# Changelog\n\n## 0.1.0\n- initial release\n";
    std::fs::write(ws.join("CHANGELOG.md"), log).unwrap();
    let goal =
        "Add the line \"- add contact file\" under the \"## 0.1.0\" heading in CHANGELOG.md.";
    assert_eq!(
        existing_target(goal, &ws),
        Some(("CHANGELOG.md".to_string(), log.to_string()))
    );
    let p = task_prompt(goal, &ws);
    assert_eq!(
        p,
        format!(
            "{}{}",
            proposal_prompt(goal, &w),
            edit_block("CHANGELOG.md", log)
        )
    );
    assert!(p.contains("\nThe file CHANGELOG.md already exists. Its current content is:\n# Changelog\n\n## 0.1.0\n- initial release\nWrite the complete new content of CHANGELOG.md"));
    // Quoted and nested paths are found; the first existing one wins.
    assert_eq!(
        existing_target("Edit `docs/plan.txt`, then README.md", &ws)
            .unwrap()
            .0,
        "docs/plan.txt"
    );
}

/// T5 boundary: the block never reads outside the workspace, a directory,
/// a symlink leaving the workspace, a non-UTF-8 file or an oversized file.
#[test]
fn edit_block_never_reads_outside_or_oversized() {
    let (d, ws) = ws();
    std::fs::write(d.path().join("secret.txt"), "outside\n").unwrap();
    std::os::unix::fs::symlink(d.path().join("secret.txt"), ws.join("link.txt")).unwrap();
    std::fs::write(ws.join("bin.dat"), [0xffu8, 0xfe, 0x00]).unwrap();
    std::fs::write(
        ws.join("big.txt"),
        "x".repeat(COMPOSE_EDIT_MAX_BYTES as usize + 1),
    )
    .unwrap();
    for goal in [
        "Edit ../secret.txt now",
        &format!("Edit {}", d.path().join("secret.txt").display()),
        "Edit link.txt now",
        "Edit docs now",
        "Edit bin.dat now",
        "Edit big.txt now",
        "Edit ~/secret.txt now",
    ] {
        assert_eq!(existing_target(goal, &ws), None, "{goal}");
    }
}
