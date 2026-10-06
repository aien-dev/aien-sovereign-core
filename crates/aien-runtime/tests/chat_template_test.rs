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
    let base = aien_runtime::spine::proposal_prompt(goal, ws);
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

/// ACCEPTANCE-v4 2(1): the bytes sent end with the assistant marker, its
/// newline and the literal prefix "filename:" (no trailing space since the v5
/// engine cut); nothing follows the colon.
#[test]
fn assistant_prefix_follows_the_assistant_marker() {
    use aien_runtime::spine::{check_file_proposal, COMPOSE_ASSISTANT_PREFIX};
    assert_eq!(COMPOSE_ASSISTANT_PREFIX, "filename:");
    let sent = format!(
        "{}{}",
        format_tinyllama_chat(&[turn("user", V2_PROMPT)]),
        COMPOSE_ASSISTANT_PREFIX
    );
    assert!(sent.starts_with("<|user|>\nGoal: "));
    assert!(sent.ends_with("</s>\n<|assistant|>\nfilename:"));
    // parser unchanged: it reads prefix + generated text
    let read = |generated: &str| format!("{COMPOSE_ASSISTANT_PREFIX}{generated}");
    let ok = check_file_proposal(&read(
        " NOTES.md\nEvery change stays inside the workspace.\n",
    ))
    .expect("path + content parses");
    assert_eq!(ok.path, "NOTES.md");
    assert!(check_file_proposal(&read("NOTES.md\n"))
        .unwrap_err()
        .contains("empty content"));
    assert!(check_file_proposal(&read("../x\ny\n"))
        .unwrap_err()
        .contains("outside the workspace"));
    if let Ok(path) = std::env::var("AIEN_TOKENIZER_PATH") {
        let tok = TinyLlamaTokenizer::from_file(path).expect("tokenizer");
        let ids = tok.encode(&sent).expect("encode");
        eprintln!(
            "prefixed prompt tokens: {} tail {:?}",
            ids.len(),
            &ids[ids.len() - 6..]
        );
        assert_eq!(&ids[ids.len() - 3..], &[13, 9507, 29901]);
    }
}

/// ACCEPTANCE-v4 2(2): the warm-up prompt spans more than one 128-token
/// prefill chunk.
#[test]
fn warm_up_prompt_spans_two_prefill_chunks() {
    let Ok(path) = std::env::var("AIEN_TOKENIZER_PATH") else {
        eprintln!("AIEN_TOKENIZER_PATH not set: warm-up length check skipped");
        return;
    };
    let tok = TinyLlamaTokenizer::from_file(path).expect("tokenizer");
    let n = tok
        .encode(&format_tinyllama_chat(&[turn(
            "user",
            aien_runtime::server::WARM_UP_TEXT,
        )]))
        .expect("encode")
        .len();
    eprintln!("warm-up prompt tokens: {n}");
    assert!(n > 128 && n <= 256, "{n}");
}

/// Llama 3 instruct layout (v5 engine cut): header, blank line, content, `<|eot_id|>`, then
/// the assistant header; no system block, no BOS in the text.
#[test]
fn llama3_template_renders_the_instruct_layout() {
    use aien_inference_abi::ChatTemplate;
    use aien_runtime::control::format_chat;
    let text = format_chat(ChatTemplate::Llama3, &[turn("user", "  Hi there \n")]);
    assert_eq!(
        text,
        "<|start_header_id|>user<|end_header_id|>\n\nHi there<|eot_id|>\
         <|start_header_id|>assistant<|end_header_id|>\n\n"
    );
    // TinyLlama keeps the zephyr layout, byte for byte.
    assert_eq!(
        format_chat(ChatTemplate::Zephyr, &[turn("user", V2_PROMPT)]),
        format_tinyllama_chat(&[turn("user", V2_PROMPT)])
    );
}

/// With a Llama 3 model directory (`AIEN_LLAMA3_DIR`): the template is detected from the
/// model's own files, encoding adds exactly one `<|begin_of_text|>` (128000), the stop set
/// is the generation_config eos list, and the compose prefix is the last two tokens.
#[test]
fn llama3_model_dir_tokenizer_has_one_bos_and_eos_list() {
    let Ok(dir) = std::env::var("AIEN_LLAMA3_DIR") else {
        eprintln!("AIEN_LLAMA3_DIR not set: Llama 3 tokenizer check skipped");
        return;
    };
    use aien_inference_abi::{ChatTemplate, ChatTokenizer};
    use aien_runtime::control::format_chat;
    use aien_runtime::spine::COMPOSE_ASSISTANT_PREFIX;
    let tok = ChatTokenizer::from_model_dir(std::path::Path::new(&dir), None).expect("tokenizer");
    assert_eq!(tok.template(), ChatTemplate::Llama3);
    assert_eq!(tok.stop_token_ids(), &[128001, 128008, 128009]);
    let text = format!(
        "{}{COMPOSE_ASSISTANT_PREFIX}",
        format_chat(tok.template(), &[turn("user", V2_PROMPT)])
    );
    let ids = tok.encode(&text).expect("encode");
    assert_eq!(ids[0], 128000);
    assert_eq!(ids.iter().filter(|&&t| t == 128000).count(), 1);
    // "<|start_header_id|>user<|end_header_id|>\n\n" after the BOS
    assert_eq!(&ids[1..5], &[128006, 882, 128007, 271]);
    // "<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\nfilename:"
    let tail = &ids[ids.len() - 7..];
    assert_eq!(&tail[..5], &[128009, 128006, 78191, 128007, 271]);
    assert_eq!(tok.decode(&tail[5..]).unwrap(), "filename:");
}
