pub mod anthropic;
pub mod openai;

pub use anthropic::*;
pub use openai::*;

use crate::models::{ChatMessage, ProviderType};
use reqwest::Client;

pub async fn call_provider_unary(
    client: &Client,
    provider: ProviderType,
    endpoint: &str,
    model: &str,
    api_key: Option<&str>,
    messages: &[ChatMessage],
    temperature: Option<f32>,
) -> Result<(String, Option<String>, usize), String> {
    if provider == ProviderType::EmbeddedNative || endpoint.starts_with("in-process://") {
        return Err("EmbeddedNative provider must be executed via in-process EmbeddedInferenceBackend, not HTTP adapter transport".to_string());
    }

    match provider {
        ProviderType::Anthropic => {
            let key = api_key.ok_or_else(|| "Anthropic requires an API key".to_string())?;
            call_anthropic_unary(client, endpoint, model, key, messages, temperature).await
        }
        ProviderType::OpenAI
        | ProviderType::OpenRouter
        | ProviderType::Gemini
        | ProviderType::Groq
        | ProviderType::DeepSeek
        | ProviderType::Together
        | ProviderType::Mistral
        | ProviderType::Ollama
        | ProviderType::LocalMax => {
            call_openai_unary(client, endpoint, model, api_key, messages, temperature).await
        }
        ProviderType::EmbeddedNative => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedded_native_rejects_http_transport() {
        let client = Client::new();
        let msgs = vec![ChatMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
            reasoning: None,
        }];
        let res = call_provider_unary(
            &client,
            ProviderType::EmbeddedNative,
            "in-process://native-transformer",
            "tinyllama",
            None,
            &msgs,
            Some(0.0),
        )
        .await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains(
            "EmbeddedNative provider must be executed via in-process EmbeddedInferenceBackend"
        ));
    }
}
