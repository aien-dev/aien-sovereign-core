//! The runtime socket accepts operator commands (launch, cancel, shutdown), so
//! it must be reachable only by its owner.

use aien_inference_abi::MockInferenceBackend;
use aien_kv_cache::create_shared_kv_manager;
use aien_runtime::client::AienRuntimeClient;
use aien_runtime::server::AienRuntimeServer;
use aien_runtime::spine::AienRuntimeSpine;
use aien_scheduler::SchedulerConfig;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[tokio::test]
async fn runtime_socket_is_owner_only() {
    let dir = std::env::temp_dir().join(format!("aien-sock-perm-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::env::set_var("AIEN_RUNTIME_STATE_DIR", &dir);
    let socket_path = dir.join("runtime.sock");

    let scheduler_config = SchedulerConfig {
        max_batch_size: 8,
        max_batch_tokens: 512,
        prefill_chunk_size: 64,
        watermark_blocks: 4,
        chunk_prefill: true,
        max_prefill_tokens: 512,
    };
    let spine = AienRuntimeSpine::new(64, scheduler_config, create_shared_kv_manager(128, 16));
    let server = AienRuntimeServer::new(spine, &socket_path);
    let handle = tokio::spawn(async move { server.run(MockInferenceBackend::new(1)).await });

    // Wait until the server answers: the file exists from bind(), but the
    // mode is only final once the server is serving.
    let client = AienRuntimeClient::new(&socket_path);
    for _ in 0..250 {
        if client.is_alive().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mode = std::fs::metadata(&socket_path)
        .ok()
        .map(|meta| meta.permissions().mode() & 0o777);
    handle.abort();
    let _ = std::fs::remove_file(&socket_path);

    assert_eq!(
        mode,
        Some(0o600),
        "runtime socket must be owner read/write only"
    );
}
