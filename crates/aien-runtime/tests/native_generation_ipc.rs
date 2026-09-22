use aien_inference_abi::MockInferenceBackend;
use aien_kv_cache::create_shared_kv_manager;
use aien_runtime::client::AienRuntimeClient;
use aien_runtime::control::GenerateReq;
use aien_runtime::server::AienRuntimeServer;
use aien_runtime::spine::AienRuntimeSpine;
use aien_scheduler::SchedulerConfig;
use tempfile::TempDir;

#[tokio::test]
async fn streams_named_generation_and_rejects_wrong_model() {
    let temp = TempDir::new().unwrap();
    let socket = temp.path().join("runtime.sock");
    let scheduler = SchedulerConfig {
        max_batch_size: 4,
        max_batch_tokens: 512,
        max_prefill_tokens: 256,
        prefill_chunk_size: 64,
        chunk_prefill: true,
        watermark_blocks: 4,
    };
    let spine = AienRuntimeSpine::new(64, scheduler, create_shared_kv_manager(128, 16));
    let server = AienRuntimeServer::new(spine, &socket);
    let handle = tokio::spawn(async move {
        server
            .run_named(MockInferenceBackend::new(1), "test-mock")
            .await
    });
    let client = AienRuntimeClient::new(&socket);
    for _ in 0..50 {
        if client.is_alive().await {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(client.is_alive().await);

    let request = GenerateReq {
        model_id: "test-mock".to_string(),
        prompt_tokens: vec![1, 2, 3],
        max_tokens: 3,
        temperature: 0.0,
    };
    let mut tokens = Vec::new();
    client
        .generate_tokens(request.clone(), |token| tokens.push(token))
        .await
        .unwrap();
    assert!(!tokens.is_empty());

    let mut wrong_model = request;
    wrong_model.model_id = "other-model".to_string();
    assert!(client.generate_tokens(wrong_model, |_| {}).await.is_err());

    client.shutdown().await.unwrap();
    handle.await.unwrap().unwrap();
}
