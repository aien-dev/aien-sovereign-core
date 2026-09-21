use super::events::InferenceEvent;
use super::request::InferenceRequest;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait InferenceService: Send + Sync {
    fn model_id(&self) -> String;
    fn endpoint(&self) -> String;
    async fn generate(&self, request: &InferenceRequest) -> Result<String, String>;
    async fn stream_events(
        &self,
        request: &InferenceRequest,
    ) -> Result<mpsc::Receiver<InferenceEvent>, String>;
}
