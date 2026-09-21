use super::limits::InferenceLimits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub limits: InferenceLimits,
    pub stream: bool,
}

impl InferenceRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            system_prompt: None,
            limits: InferenceLimits::default(),
            stream: false,
        }
    }

    pub fn with_system_prompt(mut self, sys: impl Into<String>) -> Self {
        self.system_prompt = Some(sys.into());
        self
    }

    pub fn with_max_tokens(mut self, max: usize) -> Self {
        self.limits.max_tokens = max;
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.limits.temperature = temp;
        self
    }
}
