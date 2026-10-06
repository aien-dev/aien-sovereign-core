//! NEXT-PHASE-1 v3: the proposal request goes through the model's own chat
//! template. Source: TinyLlama-1.1B-Chat-v1.0 tokenizer_config.json
//! (sha256 7b41ba7d0eb91e77914ca3dafde559ea3e19878769b7e68409e89bed5222e77a),
//! field `chat_template`, rendered with the Hugging Face settings
//! (trim_blocks, lstrip_blocks): per message `'<|' + role + '|>\n' + content
//! + eos_token` followed by the newline after the expression, then
//! `'<|assistant|>'` and its newline when a generation prompt is asked for.

use aien_inference_abi::TinyLlamaTokenizer;
use aien_runtime::control::{format_tinyllama_chat, ChatTurn};

fn turn(role: &str, content: &str) -> ChatTurn {
    ChatTurn {
        role: role.into(),
        content: content.into(),
    }
}

/// The template as the tokenizer_config.json jinja renders it, by hand.
fn rendered_by_template(messages: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (role, content) in messages {
        out.push_str(&format!("<|{role}|>\n{content}</s>\n"));
    }
    out.push_str("<|assistant|>\n");
    out
}

const V2_PROMPT: &str = "Goal: Create the file NOTES.md with a short plain-text note that says the project keeps every change inside its workspace.\nAuthorized workspace: /tmp/ws\nTop-level entries: README.md, docs\nPropose exactly one file change inside the workspace.\nAnswer in exactly this format and nothing else:\nfilename: <relative path>\n<the complete new file content>";

#[test]
fn streamturn_formatting_matches_the_model_template() {
    let ours = format_tinyllama_chat(&[turn("user", V2_PROMPT)]);
    assert_eq!(ours, rendered_by_template(&[("user", V2_PROMPT)]));
    let with_system = format_tinyllama_chat(&[turn("system", "S"), turn("user", "U")]);
    assert_eq!(
        with_system,
        rendered_by_template(&[("system", "S"), ("user", "U")])
    );
}

/// With the real tokenizer (AIEN_TOKENIZER_PATH): BOS first, and `</s>` in the
/// rendered text becomes the single EOS token id 2, not the characters.
#[test]
fn template_encodes_bos_and_one_eos_token() {
    let Ok(path) = std::env::var("AIEN_TOKENIZER_PATH") else {
        eprintln!("AIEN_TOKENIZER_PATH not set: token check skipped");
        return;
    };
    let tok = TinyLlamaTokenizer::from_file(path).expect("tokenizer");
    let ids = tok
        .encode(&format_tinyllama_chat(&[turn("user", V2_PROMPT)]))
        .expect("encode");
    eprintln!("template ids ({}): {:?}", ids.len(), ids);
    assert_eq!(ids[0], TinyLlamaTokenizer::BOS_TOKEN_ID);
    let eos: Vec<usize> = ids
        .iter()
        .enumerate()
        .filter(|(_, &t)| t == TinyLlamaTokenizer::EOS_TOKEN_ID)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(eos.len(), 1, "exactly one EOS (the user turn end)");
}

/// Prompt sizes behind the ACCEPTANCE-v3 budget arithmetic (printed; the
/// assertion only guards the 2-chunk assumption: at most 256 prompt tokens).
#[test]
fn proposal_prompt_token_counts() {
    let Ok(path) = std::env::var("AIEN_TOKENIZER_PATH") else {
        eprintln!("AIEN_TOKENIZER_PATH not set: token count skipped");
        return;
    };
    let tok = TinyLlamaTokenizer::from_file(path).expect("tokenizer");
    let ws = "/home/drakestapleton/.claude/jobs/9bfe8553/tmp/np1-v3run/ws";
    let goal = "Create the file NOTES.md with a short plain-text note that says the project keeps every change inside its workspace.";
    let base = aien_runtime::spine::proposal_prompt(goal, ws, "README.md, docs");
    let retry = aien_runtime::spine::retry_prompt(
        &base,
        "no filename line: the first line must be 'filename: <relative path>'",
    );
    for (name, p) in [("base", &base), ("retry", &retry)] {
        let n = tok
            .encode(&format_tinyllama_chat(&[turn("user", p)]))
            .expect("encode")
            .len();
        eprintln!("{name} prompt tokens: {n}");
        assert!(n <= 256);
    }
}
