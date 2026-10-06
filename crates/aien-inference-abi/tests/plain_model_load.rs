//! A base (non-chat) model directory has no chat template: it loads with an explicit
//! "no chat template" state, plain encode/decode work, and chat rendering is refused.
//! An existing but unrecognized template is still refused at load.

use aien_inference_abi::tokenizer::{ChatTemplate, ChatTokenizer};
use std::path::{Path, PathBuf};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/openwaldo-byte");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aien-plain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["tokenizer.json", "config.json", "generation_config.json"] {
        std::fs::copy(Path::new(DIR).join(f), dir.join(f)).unwrap();
    }
    dir
}

#[test]
fn model_dir_without_template_loads_as_plain() {
    let tok = ChatTokenizer::from_model_dir(Path::new(DIR), None).expect("plain model loads");
    assert_eq!(tok.template(), ChatTemplate::None);
    assert!(tok.stop_token_ids().contains(&2));
    let ids = tok.encode("hi").expect("plain encode");
    assert!(!ids.is_empty());
    assert!(tok
        .decode_opts(&ids, true)
        .expect("plain decode")
        .contains("hi"));
}

#[test]
fn chat_render_on_plain_model_is_refused() {
    let tok = ChatTokenizer::from_model_dir(Path::new(DIR), None).unwrap();
    let err = tok
        .try_format_chat(&[("user", "hello")])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no chat template"), "{err}");
    let err = ChatTemplate::None
        .try_render(&[("user", "hello")])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no chat template"), "{err}");
}

#[test]
fn unrecognized_template_still_refused_at_load() {
    let dir = scratch("unrec");
    std::fs::write(
        dir.join("tokenizer_config.json"),
        r#"{"chat_template": "{{ messages }}"}"#,
    )
    .unwrap();
    let err = ChatTokenizer::from_model_dir(&dir, None)
        .err()
        .expect("refused")
        .to_string();
    assert!(err.contains("unsupported chat template"), "{err}");
    std::fs::write(dir.join("tokenizer_config.json"), "{}").unwrap();
    std::fs::write(dir.join("chat_template.jinja"), "{{ messages }}").unwrap();
    assert!(ChatTokenizer::from_model_dir(&dir, None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Plain prompt format: the text is passed to the tokenizer as-is, nothing is wrapped
/// around it. Whatever the model's own tokenizer.json adds (BOS via its post-processor)
/// is added, exactly as `from_file` + `encode` does; the OpenWALDO byte tokenizer has
/// none, so "hi" is its two bytes (h=104, i=105, ids = byte + 3).
#[test]
fn plain_prompt_is_text_encoded_as_is() {
    let tok = ChatTokenizer::from_model_dir(Path::new(DIR), None).unwrap();
    let direct = ChatTokenizer::from_file(format!("{DIR}/tokenizer.json")).unwrap();
    assert_eq!(tok.encode("hi").unwrap(), direct.encode("hi").unwrap());
    assert_eq!(tok.encode("hi").unwrap(), vec![107, 108]);
}
