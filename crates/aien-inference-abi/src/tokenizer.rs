//! Pure Rust tokenizer and chat template formatter for TinyLlama models.
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

/// Pure Rust wrapper around Hugging Face tokenizers for TinyLlama.
#[derive(Clone)]
pub struct TinyLlamaTokenizer {
    inner: tokenizers::Tokenizer,
}

impl TinyLlamaTokenizer {
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
        Ok(Self { inner })
    }

    /// Loads tokenizer directly from a JSON byte buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenizerError> {
        let inner = tokenizers::Tokenizer::from_bytes(bytes)
            .map_err(|e| TokenizerError::LoadError(format!("Failed to parse tokenizer bytes: {}", e)))?;
        Ok(Self { inner })
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
    pub fn encode_with_special(&self, text: &str, add_special_tokens: bool) -> Result<Vec<u32>, TokenizerError> {
        let encoding = self
            .inner
            .encode(text, add_special_tokens)
            .map_err(|e| TokenizerError::EncodeError(e.to_string()))?;
        let tokens = encoding.get_ids().to_vec();
        if tokens.len() > Self::MAX_CONTEXT_LEN {
            return Err(TokenizerError::ContextLengthExceeded {
                len: tokens.len(),
                max: Self::MAX_CONTEXT_LEN,
            });
        }
        Ok(tokens)
    }

    /// Decodes a sequence of token IDs back into recognizable UTF-8 text.
    pub fn decode(&self, tokens: &[u32]) -> Result<String, TokenizerError> {
        self.decode_opts(tokens, false)
    }

    /// Decodes tokens with option to skip special tokens.
    pub fn decode_opts(&self, tokens: &[u32], skip_special_tokens: bool) -> Result<String, TokenizerError> {
        self.inner
            .decode(tokens, skip_special_tokens)
            .map_err(|e| TokenizerError::DecodeError(e.to_string()))
    }

    /// Checks if a token ID signifies generation stopping (EOS).
    pub fn is_eos(&self, token_id: u32) -> bool {
        token_id == Self::EOS_TOKEN_ID
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
