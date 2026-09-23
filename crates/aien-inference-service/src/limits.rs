use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceLimits {
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: Option<f32>,
    pub stop_sequences: Vec<String>,
}

impl Default for InferenceLimits {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            temperature: 0.7,
            top_p: None,
            stop_sequences: Vec::new(),
        }
    }
}
