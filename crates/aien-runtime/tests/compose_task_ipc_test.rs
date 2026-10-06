//! NEXT-PHASE-1 cut 1b: RunComposeTask over the runtime socket. No tokenizer
//! is loaded, so the real "model" Skill (the StreamTurn inference path)
//! fails and nothing commits: this proves the routing, the blocking hand-off
//! and the result record, not the model.
use aien_inference_abi::MockInferenceBackend;
use aien_kv_cache::create_shared_kv_manager;
use aien_runtime::client::AienRuntimeClient;
use aien_runtime::control::{ControlCommand, ControlResponse};
use aien_runtime::server::AienRuntimeServer;
use aien_runtime::spine::AienRuntimeSpine;
use aien_scheduler::SchedulerConfig;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_compose_task_over_socket() {
    let tmp = tempfile::tempdir().unwrap();
    // Only this test binary sets it; one test per binary avoids env races.
    std::env::set_var("AIEN_COMPOSE_DIR", tmp.path().join("compose"));
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let socket = tmp.path().join("runtime.sock");
    let cfg = SchedulerConfig {
        max_batch_size: 8,
        max_batch_tokens: 1024,
        prefill_chunk_size: 64,
        watermark_blocks: 4,
        chunk_prefill: true,
        max_prefill_tokens: 1024,
    };
    let spine = AienRuntimeSpine::new(64, cfg, create_shared_kv_manager(256, 16));
    let server = AienRuntimeServer::new(spine, &socket);
    let handle = tokio::spawn(async move { server.run(MockInferenceBackend::new(1)).await });
    let client = AienRuntimeClient::new(&socket);
    let mut alive = false;
    for _ in 0..100 {
        if client.is_alive().await {
            alive = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(alive);

    let resp = client
        .send_command(ControlCommand::RunComposeTask {
            goal: "propose one change".into(),
            workspace: ws.to_str().unwrap().into(),
        })
        .await
        .unwrap();
    match resp {
        ControlResponse::ComposeTaskResult(r) => {
            if !aien_omega_compose::LINKED {
                panic!("stub build returned a compose result");
            }
            assert!(
                !r.committed,
                "no tokenizer: the model Skill must fail, {r:?}"
            );
            assert_eq!(r.proposal, None);
            assert_eq!(r.proposer, "model:StreamTurn-path");
            assert_eq!(r.branch_count, 1);
        }
        ControlResponse::Error(e) => {
            if aien_omega_compose::LINKED {
                panic!("linked build returned {e}");
            }
            assert!(e.contains("not linked"), "{e}");
        }
        other => panic!("unexpected {other:?}"),
    }
    let _ = client.send_command(ControlCommand::Shutdown).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}
