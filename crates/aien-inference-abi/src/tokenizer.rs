//! Pure Rust tokenizer and chat template formatter for the Llama model family
//! (TinyLlama zephyr-style chat, Llama 3 instruct chat).
//! Wraps Hugging Face tokenizers crate without Python runtime dependencies.

use std::fmt;
use std::path::Path;

/// Loud error enum for tokenizer operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenizerError {
    LoadError(String),
    EncodeError(String),
    DecodeError(String),
    ContextLengthExceeded { len: usize, max: usize },
}

impl fmt::Display for TokenizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoadError(e) => write!(f, "Failed to load tokenizer: {}", e),
            Self::EncodeError(e) => write!(f, "Tokenization encoding failed: {}", e),
            Self::DecodeError(e) => write!(f, "Tokenization decoding failed: {}", e),
            Self::ContextLengthExceeded { len, max } => {
                write!(
                    f,
                    "Sequence length {} exceeds maximum context length {}",
                    len, max
                )
            }
        }
    }
}

impl std::error::Error for TokenizerError {}

/// The chat layout a model was fine-tuned on. Selected from the model's own chat template
/// text (no template engine): each variant renders the one layout that template produces
/// for plain user/system/assistant turns ending in a generation prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatTemplate {
    /// TinyLlama-Chat (zephyr): `<|user|>\n{content}</s>\n<|assistant|>\n`.
    Zephyr,
    /// Llama 3 instruct: `<|start_header_id|>user<|end_header_id|>\n\n{content}<|eot_id|>`
    /// then `<|start_header_id|>assistant<|end_header_id|>\n\n`. No system block is added
    /// (the Hugging Face template's default "Cutting Knowledge Date / Today Date" block
    /// depends on the wall clock, so it is left out).
    Llama3,
}

impl ChatTemplate {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Zephyr => "zephyr (TinyLlama chat)",
            Self::Llama3 => "llama3 (Llama 3 instruct)",
        }
    }

    /// Recognizes the layout from a Jinja chat template's text.
    pub fn detect(template_text: &str) -> Result<Self, TokenizerError> {
        if template_text.contains("<|start_header_id|>") && template_text.contains("<|eot_id|>") {
            Ok(Self::Llama3)
        } else if template_text.contains("<|user|>") && template_text.contains("<|assistant|>") {
            Ok(Self::Zephyr)
        } else {
            Err(TokenizerError::LoadError(
                "unsupported chat template: neither zephyr (<|user|>) nor llama3 \
                 (<|start_header_id|>) markers found"
                    .to_string(),
            ))
        }
    }

    /// Renders `(role, content)` turns. Contents are trimmed and empty turns skipped; roles
    /// other than system and assistant are user turns. Generation continues from an
    /// assistant header unless the last turn is a non-empty assistant turn. The BOS token
    /// is not part of the text: `encode` adds exactly one through the tokenizer's
    /// post-processor.
    pub fn render(&self, turns: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (role, content) in turns {
            let body = content.trim();
            if body.is_empty() {
                continue;
            }
            let role = match role.trim().to_ascii_lowercase().as_str() {
                "system" => "system",
                "assistant" => "assistant",
                _ => "user",
            };
            match self {
                Self::Zephyr => out.push_str(&format!("<|{role}|>\n{body}</s>\n")),
                Self::Llama3 => out.push_str(&format!(
                    "<|start_header_id|>{role}<|end_header_id|>\n\n{body}<|eot_id|>"
                )),
            }
        }
        let ends_in_assistant = turns.last().is_some_and(|(role, content)| {
            role.trim().eq_ignore_ascii_case("assistant") && !content.trim().is_empty()
        });
        if !ends_in_assistant {
            out.push_str(match self {
                Self::Zephyr => "<|assistant|>\n",
                Self::Llama3 => "<|start_header_id|>assistant<|end_header_id|>\n\n",
            });
        }
        out
    }
}

/// Pure Rust wrapper around Hugging Face tokenizers with the model's chat template and
/// end-of-sequence set. `from_file`/`from_bytes` give the TinyLlama defaults;
/// `from_model_dir` reads them from the model directory.
#[derive(Clone)]
pub struct ChatTokenizer {
    inner: tokenizers::Tokenizer,
    template: ChatTemplate,
    stop_token_ids: Vec<u32>,
    max_context_len: usize,
}

/// The name this tokenizer had while it served TinyLlama only.
pub type TinyLlamaTokenizer = ChatTokenizer;

fn read_json(path: &Path) -> Result<Option<serde_json::Value>, TokenizerError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| {
            TokenizerError::LoadError(format!("{} is not valid JSON: {}", path.display(), e))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(TokenizerError::LoadError(format!(
            "Failed to read {}: {}",
            path.display(),
            e
        ))),
    }
}

/// `eos_token_id` of a Hugging Face config as a list (it is a number or an array).
pub fn eos_token_ids_from_json(value: &serde_json::Value) -> Option<Vec<u32>> {
    match value.get("eos_token_id")? {
        serde_json::Value::Number(n) => n.as_u64().map(|id| vec![id as u32]),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| v.as_u64().map(|id| id as u32))
            .collect(),
        _ => None,
    }
}

impl ChatTokenizer {
    /// Beginning of Sequence token ID pinned for TinyLlama (<s>).
    pub const BOS_TOKEN_ID: u32 = 1;
    /// End of Sequence token ID pinned for TinyLlama (</s>).
    pub const EOS_TOKEN_ID: u32 = 2;
    /// Unknown / Padding token ID pinned for TinyLlama (<unk>).
    pub const UNK_TOKEN_ID: u32 = 0;
    /// Maximum context window supported by TinyLlama-1.1B.
    pub const MAX_CONTEXT_LEN: usize = 2048;

    /// Loads tokenizer directly from a local tokenizer.json file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, TokenizerError> {
        let p = path.as_ref();
        let inner = tokenizers::Tokenizer::from_file(p).map_err(|e| {
            TokenizerError::LoadError(format!("Failed to load from {}: {}", p.display(), e))
        })?;
        Ok(Self::tinyllama(inner))
    }

    /// Loads tokenizer directly from a JSON byte buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenizerError> {
        let inner = tokenizers::Tokenizer::from_bytes(bytes).map_err(|e| {
            TokenizerError::LoadError(format!("Failed to parse tokenizer bytes: {}", e))
        })?;
        Ok(Self::tinyllama(inner))
    }

    fn tinyllama(inner: tokenizers::Tokenizer) -> Self {
        Self {
            inner,
            template: ChatTemplate::Zephyr,
            stop_token_ids: vec![Self::EOS_TOKEN_ID],
            max_context_len: Self::MAX_CONTEXT_LEN,
        }
    }

    /// Loads `tokenizer_json` (default `<dir>/tokenizer.json`) with the chat template and
    /// end-of-sequence set of the model directory `dir`:
    /// - template: detected from `chat_template` in `tokenizer_config.json`, else from
    ///   `chat_template.jinja`;
    /// - stop set: `eos_token_id` of `generation_config.json`, else of `config.json`;
    /// - context limit: `max_position_embeddings` of `config.json` (default 2048).
    pub fn from_model_dir(
        dir: &Path,
        tokenizer_json: Option<&Path>,
    ) -> Result<Self, TokenizerError> {
        let default_json = dir.join("tokenizer.json");
        let mut tokenizer = Self::from_file(tokenizer_json.unwrap_or(&default_json))?;
        let template_text = match read_json(&dir.join("tokenizer_config.json"))?
            .and_then(|c| c.get("chat_template").and_then(|t| t.as_str()).map(str::to_string))
        {
            Some(text) => text,
            None => std::fs::read_to_string(dir.join("chat_template.jinja")).map_err(|e| {
                TokenizerError::LoadError(format!(
                    "no chat template in {} (tokenizer_config.json chat_template or chat_template.jinja): {}",
                    dir.display(),
                    e
                ))
            })?,
        };
        tokenizer.template = ChatTemplate::detect(&template_text)?;
        let config = read_json(&dir.join("config.json"))?;
        let stop = read_json(&dir.join("generation_config.json"))?
            .as_ref()
            .and_then(eos_token_ids_from_json)
            .or_else(|| config.as_ref().and_then(eos_token_ids_from_json))
            .ok_or_else(|| {
                TokenizerError::LoadError(format!(
                    "no eos_token_id in {}/generation_config.json or config.json",
                    dir.display()
                ))
            })?;
        if stop.is_empty() {
            return Err(TokenizerError::LoadError(format!(
                "empty eos_token_id in {}",
                dir.display()
            )));
        }
        tokenizer.stop_token_ids = stop;
        if let Some(max) = config
            .as_ref()
            .and_then(|c| c.get("max_position_embeddings"))
            .and_then(|v| v.as_u64())
        {
            tokenizer.max_context_len = max as usize;
        }
        Ok(tokenizer)
    }

    /// The chat layout this tokenizer renders.
    pub fn template(&self) -> ChatTemplate {
        self.template
    }

    /// Token ids that end generation (the model's `eos_token_id` set).
    pub fn stop_token_ids(&self) -> &[u32] {
        &self.stop_token_ids
    }

    /// Renders `(role, content)` turns with this model's chat template.
    pub fn format_chat(&self, turns: &[(&str, &str)]) -> String {
        self.template.render(turns)
    }

    /// Formats a conversation according to the canonical TinyLlama chat template:
    /// `<|system|>\n{system}</s>\n<|user|>\n{user}</s>\n<|assistant|>\n`
    pub fn format_chat_prompt(&self, system: Option<&str>, user: &str) -> String {
        Self::format_prompt(system, user)
    }

    /// Static formatter for the canonical TinyLlama chat template.
    pub fn format_prompt(system: Option<&str>, user: &str) -> String {
        match system {
            Some(sys) if !sys.trim().is_empty() => {
                format!(
                    "<|system|>\n{}</s>\n<|user|>\n{}</s>\n<|assistant|>\n",
                    sys.trim(),
                    user.trim()
                )
            }
            _ => {
                format!("<|user|>\n{}</s>\n<|assistant|>\n", user.trim())
            }
        }
    }

    /// Encodes text into token IDs including configured special tokens.
    pub fn encode(&self, text: &str) -> Result<Vec<u32>, TokenizerError> {
        self.encode_with_special(text, true)
    }

    /// Encodes text with explicit control over special tokens insertion.
    pub fn encode_with_special(
        &self,
        text: &str,
        add_special_tokens: bool,
    ) -> Result<Vec<u32>, TokenizerError> {
        let encoding = self
            .inner
            .encode(text, add_special_tokens)
            .map_err(|e| TokenizerError::EncodeError(e.to_string()))?;
        let tokens = encoding.get_ids().to_vec();
        if tokens.len() > self.max_context_len {
            return Err(TokenizerError::ContextLengthExceeded {
                len: tokens.len(),
                max: self.max_context_len,
            });
        }
        Ok(tokens)
    }

    /// Decodes a sequence of token IDs back into recognizable UTF-8 text.
    pub fn decode(&self, tokens: &[u32]) -> Result<String, TokenizerError> {
        self.decode_opts(tokens, false)
    }

    /// Decodes tokens with option to skip special tokens.
    pub fn decode_opts(
        &self,
        tokens: &[u32],
        skip_special_tokens: bool,
    ) -> Result<String, TokenizerError> {
        self.inner
            .decode(tokens, skip_special_tokens)
            .map_err(|e| TokenizerError::DecodeError(e.to_string()))
    }

    /// Checks if a token ID signifies generation stopping (EOS).
    pub fn is_eos(&self, token_id: u32) -> bool {
        self.stop_token_ids.contains(&token_id)
    }

    /// Checks if a token ID signifies sequence beginning (BOS).
    pub fn is_bos(&self, token_id: u32) -> bool {
        token_id == Self::BOS_TOKEN_ID
    }

    /// Checks if a token is a terminal or stop token.
    pub fn is_stop_token(&self, token_id: u32) -> bool {
        token_id == Self::EOS_TOKEN_ID || token_id == Self::UNK_TOKEN_ID
    }

    /// Returns the vocabulary size of the underlying model.
    pub fn vocab_size(&self) -> usize {
        self.inner.get_vocab_size(true)
    }

    /// Returns the token ID for a given string token if present in vocabulary.
    pub fn token_to_id(&self, token: &str) -> Option<u32> {
        self.inner.token_to_id(token)
    }

    /// Returns the string representation for a given token ID.
    pub fn id_to_token(&self, id: u32) -> Option<String> {
        self.inner.id_to_token(id)
    }

    /// Returns a reference to the underlying tokenizers::Tokenizer.
    pub fn inner(&self) -> &tokenizers::Tokenizer {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinned_special_tokens() {
        assert_eq!(TinyLlamaTokenizer::BOS_TOKEN_ID, 1);
        assert_eq!(TinyLlamaTokenizer::EOS_TOKEN_ID, 2);
        assert_eq!(TinyLlamaTokenizer::UNK_TOKEN_ID, 0);
        assert_eq!(TinyLlamaTokenizer::MAX_CONTEXT_LEN, 2048);
    }

    #[test]
    fn test_chat_template_with_system() {
        let system = "You are a sovereign AI assistant.";
        let user = "Explain the role of an operating system in one sentence.";
        let formatted = TinyLlamaTokenizer::format_prompt(Some(system), user);

        let expected = "<|system|>\nYou are a sovereign AI assistant.</s>\n<|user|>\nExplain the role of an operating system in one sentence.</s>\n<|assistant|>\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_chat_template_without_system() {
        let user = "Hello TinyLlama!";
        let formatted = TinyLlamaTokenizer::format_prompt(None, user);

        let expected = "<|user|>\nHello TinyLlama!</s>\n<|assistant|>\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_chat_template_with_empty_system() {
        let user = "Hello TinyLlama!";
        let formatted = TinyLlamaTokenizer::format_prompt(Some("   "), user);

        let expected = "<|user|>\nHello TinyLlama!</s>\n<|assistant|>\n";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_stop_and_eos_token_checks() {
        // We can construct a mock tokenizer instance or test static methods
        assert_eq!(TinyLlamaTokenizer::EOS_TOKEN_ID, 2);
        assert_eq!(TinyLlamaTokenizer::BOS_TOKEN_ID, 1);
        assert_eq!(TinyLlamaTokenizer::UNK_TOKEN_ID, 0);
    }

    #[test]
    fn test_synthetic_tokenizer_roundtrip() {
        // Construct a minimal BPE tokenizer JSON for testing encode and decode
        let minimal_json = r#"{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [
                {"id": 0, "content": "<unk>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
                {"id": 1, "content": "<s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
                {"id": 2, "content": "</s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}
            ],
            "normalizer": null,
            "pre_tokenizer": {"type": "Whitespace"},
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "WordLevel",
                "unk_token": "<unk>",
                "vocab": {
                    "<unk>": 0,
                    "<s>": 1,
                    "</s>": 2,
                    "hello": 3,
                    "world": 4
                }
            }
        }"#;

        let tokenizer = TinyLlamaTokenizer::from_bytes(minimal_json.as_bytes()).unwrap();
        assert_eq!(tokenizer.vocab_size(), 5);
        assert_eq!(tokenizer.token_to_id("hello"), Some(3));
        assert_eq!(tokenizer.token_to_id("world"), Some(4));
        assert_eq!(tokenizer.id_to_token(1), Some("<s>".to_string()));
        assert_eq!(tokenizer.id_to_token(2), Some("</s>".to_string()));

        let tokens = tokenizer.encode("hello world").unwrap();
        assert_eq!(tokens, vec![3, 4]);

        let decoded = tokenizer.decode(&tokens).unwrap();
        assert_eq!(decoded, "hello world");
        assert!(tokenizer.is_eos(2));
        assert!(!tokenizer.is_eos(3));
        assert!(tokenizer.is_bos(1));
        assert!(tokenizer.is_stop_token(2));
        assert!(tokenizer.is_stop_token(0));
    }
}
