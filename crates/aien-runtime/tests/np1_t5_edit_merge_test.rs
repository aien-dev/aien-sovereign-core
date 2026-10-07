//! NEXT-PHASE-1 v7 T5: an edit-mode reply never drops an existing line.
//!
//! v6 T5 FAILed T5-K and Q2: the model (GB10 and the CPU reference agree,
//! reply sha256 cd8e3b30...) answered the edit goal with only the changed
//! part, `filename: CHANGELOG.md` / `## 0.1.0` / `- add contact file`, ending
//! on eos after 16 of 96 tokens, and the Skill handed that fragment on as the
//! complete new file. These tests run the exact v6 seed and reply through the
//! Skill's proposal path on the CPU.
use aien_runtime::spine::{
    check_file_proposal, edit_proposal, merge_edit_reply, propose_task_with_retries,
    task_prompt_and_target, Generation, COMPOSE_MAX_ATTEMPTS, COMPOSE_SKILL_BUDGET,
};

/// The v6 pre-seed, byte for byte (seed sha256 29e905c6...).
const SEED: &str = include_str!("../../../docs/campaigns/next-phase-1/seed-v6/T5/CHANGELOG.md");
/// The v6 T5 reply, byte for byte (GB10 and CPU reference).
const V6_REPLY: &str = include_str!(
    "../../../docs/campaigns/next-phase-1/replies/cd8e3b30c007e2f0bfe66277688dbec833d55a7bbefc47103203f2615125d094.txt"
);
const GOAL: &str =
    "Add the line \"- add contact file\" under the \"## 0.1.0\" heading in CHANGELOG.md.";

fn seeded() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("CHANGELOG.md"), SEED).unwrap();
    let ws = std::fs::canonicalize(ws).unwrap();
    (d, ws)
}

/// Runs `replies` (one per attempt) through the Skill's proposal path for
/// the T5 goal on the seeded workspace.
fn run(replies: &[&str]) -> (Result<String, String>, Vec<aien_runtime::ProposalAttempt>) {
    let (_d, ws) = seeded();
    let (prompt, target) = task_prompt_and_target(GOAL, &ws);
    assert!(prompt.contains(SEED), "the model is shown the whole seed");
    let target = target.expect("edit mode");
    assert_eq!(target, ("CHANGELOG.md".to_string(), SEED.to_string()));
    let replies: Vec<String> = replies.iter().map(|s| s.to_string()).collect();
    let n = std::sync::atomic::AtomicUsize::new(0);
    let proposer = move |_p: &str, _l: std::time::Duration| {
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Generation {
            text: replies[k.min(replies.len() - 1)].clone(),
            tokens: 16,
            finish_reason: Some("eos".into()),
            ..Default::default()
        })
    };
    propose_task_with_retries(
        &proposer,
        &prompt,
        Some((target.0.as_str(), target.1.as_str())),
        COMPOSE_SKILL_BUDGET,
        std::time::Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    )
}

fn lines(s: &str) -> Vec<&str> {
    s.lines().collect()
}

/// ACCEPTANCE-v6 T5-K: every non-empty seed line is a line of the content.
/// T5-N: `- add contact file` follows `## 0.1.0` before the next heading.
fn t5_rows(content: &str) -> (bool, bool) {
    let c = lines(content);
    let k = lines(SEED)
        .iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| c.iter().any(|x| x.trim_end() == l.trim_end()));
    let n = c.iter().position(|l| *l == "## 0.1.0").is_some_and(|h| {
        c[h + 1..]
            .iter()
            .take_while(|l| !l.starts_with('#'))
            .any(|l| *l == "- add contact file")
    });
    (k, n)
}

/// The red-then-green test: the exact v6 reply keeps all three seed lines,
/// byte-identical and in order, with the new line under the heading.
#[test]
fn v6_t5_reply_keeps_every_seed_line() {
    assert_eq!(SEED, "# Changelog\n\n## 0.1.0\n- initial release\n");
    assert_eq!(
        V6_REPLY,
        "filename: CHANGELOG.md\n## 0.1.0\n- add contact file"
    );
    let (out, attempts) = run(&[V6_REPLY]);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(attempts[0].outcome, "parsed");
    // The attempt keeps the model's own reply; the proposal is the merge.
    assert_eq!(attempts[0].text.as_deref(), Some(V6_REPLY));
    let p = check_file_proposal(&out.expect("proposal")).unwrap();
    assert_eq!(p.path, "CHANGELOG.md");
    assert_eq!(
        p.content,
        "# Changelog\n\n## 0.1.0\n- add contact file\n- initial release\n"
    );
    assert_eq!(t5_rows(&p.content), (true, true));
    // Every seed line survives byte-identical, in its original order.
    let c = lines(&p.content);
    let mut at = 0;
    for l in lines(SEED) {
        at += c[at..].iter().position(|x| *x == l).expect(l) + 1;
    }
}

/// A whole-file reply that keeps every line (what the v6 prompt asks for)
/// is proposed byte for byte: the merge adds nothing and drops nothing.
#[test]
fn whole_file_reply_is_kept_byte_for_byte() {
    let whole =
        "filename: CHANGELOG.md\n# Changelog\n\n## 0.1.0\n- add contact file\n- initial release\n";
    let (out, _) = run(&[whole]);
    assert_eq!(out.unwrap(), whole);
}

/// A fragment that puts the new line above an existing line lands right
/// before that line, not at the top of the file.
#[test]
fn new_line_above_its_anchor_lands_before_it() {
    let (out, _) = run(&["filename: CHANGELOG.md\n- add contact file\n- initial release\n"]);
    let p = check_file_proposal(&out.unwrap()).unwrap();
    assert_eq!(
        p.content,
        "# Changelog\n\n## 0.1.0\n- add contact file\n- initial release\n"
    );
}

/// A reply sharing no line with the file has no place in it: refused (and
/// retried) instead of replacing the file.
#[test]
fn reply_without_an_anchor_is_refused_and_retried() {
    let (out, attempts) = run(&["filename: CHANGELOG.md\n- add contact file\n", V6_REPLY]);
    assert_eq!(attempts[0].outcome, "refused", "{attempts:?}");
    assert!(
        attempts[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("shares no line"),
        "{attempts:?}"
    );
    assert_eq!(attempts[1].outcome, "parsed");
    assert_eq!(
        t5_rows(&check_file_proposal(&out.unwrap()).unwrap().content),
        (true, true)
    );
}

/// A reply that only repeats existing lines changes nothing: refused.
#[test]
fn reply_that_changes_nothing_is_refused() {
    let (out, attempts) = run(&["filename: CHANGELOG.md\n## 0.1.0\n"]);
    assert!(out.is_err());
    assert!(attempts.iter().all(|a| a
        .reason
        .as_deref()
        .unwrap_or("")
        .contains("changes nothing")));
}

/// A reply naming another file keeps the v5 whole-file meaning; without an
/// edit target the proposal is the reply.
#[test]
fn other_path_and_no_target_keep_the_v5_proposal() {
    let other = "filename: NOTES.md\nhello\n";
    assert_eq!(
        edit_proposal(other, Some(("CHANGELOG.md", SEED))).unwrap(),
        other
    );
    assert_eq!(edit_proposal(V6_REPLY, None).unwrap(), V6_REPLY);
    let (out, _) = run(&[other]);
    assert_eq!(out.unwrap(), other);
}

/// Merge unit cases: seed lines keep their exact bytes (trailing spaces
/// included); a seed without a final newline gains one.
#[test]
fn merge_keeps_seed_bytes() {
    let prior = "a  \nb\nc";
    assert_eq!(
        merge_edit_reply(prior, "b\nnew\n").unwrap(),
        "a  \nb\nnew\nc\n"
    );
    assert_eq!(
        merge_edit_reply(prior, "a\nb\nc\nd\n").unwrap(),
        "a  \nb\nc\nd\n"
    );
}
