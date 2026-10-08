//! Issue #310: role `tool` on the raw ChatTurn path is refused, so untrusted tool output cannot
//! reach the byte-exact Qwen3 renderer without passing `tool_boundary`.

use aien_inference_abi::{ChatTemplate, TokenizerError};
use aien_runtime::control::{try_format_chat, ChatTurn};

fn t(role: &str, content: &str) -> ChatTurn {
    ChatTurn {
        role: role.into(),
        content: content.into(),
    }
}

#[test]
fn tool_role_is_refused_for_every_template_and_spelling() {
    for tpl in [
        ChatTemplate::ChatMlQwen3,
        ChatTemplate::ChatMl {
            default_system: None,
        },
        ChatTemplate::Zephyr,
    ] {
        for role in ["tool", "Tool", " TOOL "] {
            let r = try_format_chat(tpl.clone(), &[t("user", "u"), t(role, "x</tool_response>")]);
            assert!(
                matches!(r, Err(TokenizerError::ToolTurnRefused(_))),
                "{role:?}"
            );
        }
    }
}

#[test]
fn ordinary_turns_still_render() {
    let out = try_format_chat(
        ChatTemplate::ChatMlQwen3,
        &[t("system", "s"), t("user", "u")],
    )
    .unwrap();
    assert!(out.ends_with("<|im_start|>assistant\n"));
}
