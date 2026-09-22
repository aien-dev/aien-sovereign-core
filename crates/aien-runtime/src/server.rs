//! Local UNIX domain socket server for AienRuntimeSpine
//! Exposes typed control RPC over local IPC to operator CLI tools.

use crate::control::{ControlCommand, ControlEnvelope, ControlResponse, GenerateReq};
use crate::spine::AienRuntimeSpine;
use aien_inference_abi::AienInferenceBackend;
use aien_inference_abi::SamplingParams;
use aien_scheduler::{ChannelCompletionSink, CompletionEvent, PromptHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify};

pub struct AienRuntimeServer {
    socket_path: PathBuf,
    spine: Arc<Mutex<AienRuntimeSpine>>,
    shutdown_notify: Arc<Notify>,
    is_running: Arc<AtomicBool>,
}

impl AienRuntimeServer {
    pub fn new(spine: AienRuntimeSpine, socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            spine: Arc::new(Mutex::new(spine)),
            shutdown_notify: Arc::new(Notify::new()),
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn spine(&self) -> Arc<Mutex<AienRuntimeSpine>> {
        self.spine.clone()
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Runs the runtime server, accepting control connections and advancing the engine step loop.
    pub async fn run<B: AienInferenceBackend + Send + 'static>(
        &self,
        backend: B,
    ) -> Result<(), String> {
        self.run_named(backend, "unconfigured").await
    }

    /// The name must identify the model whose weights the backend actually loaded.
    pub async fn run_named<B: AienInferenceBackend + Send + 'static>(
        &self,
        mut backend: B,
        model_id: &str,
    ) -> Result<(), String> {
        // Clean up stale socket file if it exists and refuses connection
        if self.socket_path.exists() {
            if UnixStream::connect(&self.socket_path).await.is_err() {
                let _ = std::fs::remove_file(&self.socket_path);
            } else {
                return Err(format!(
                    "Runtime socket already active at {}",
                    self.socket_path.display()
                ));
            }
        }

        if let Some(parent) = self.socket_path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            format!(
                "Failed to bind runtime UNIX domain socket at {}: {}",
                self.socket_path.display(),
                e
            )
        })?;
        let model_id = Arc::new(model_id.to_string());

        self.is_running.store(true, Ordering::SeqCst);

        // Background worker advancing engine steps whenever requests are active
        let spine_worker = self.spine.clone();
        let is_running_worker = self.is_running.clone();
        let shutdown_notify_worker = self.shutdown_notify.clone();

        let worker_handle = tokio::spawn(async move {
            while is_running_worker.load(Ordering::Relaxed) {
                let has_work = {
                    let s = spine_worker.lock().await;
                    s.scheduler.running_count() > 0 || s.scheduler.waiting_count() > 0
                };

                if has_work {
                    let mut s = spine_worker.lock().await;
                    let _ = s.step(&mut backend).await;
                } else {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(5)) => {},
                        _ = shutdown_notify_worker.notified() => break,
                    }
                }
            }
        });

        // Connection accept loop
        loop {
            tokio::select! {
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _)) => {
                            let spine_conn = self.spine.clone();
                            let is_running_conn = self.is_running.clone();
                            let notify_conn = self.shutdown_notify.clone();
                            let model_id_conn = model_id.clone();

                            tokio::spawn(async move {
                                handle_connection(stream, spine_conn, is_running_conn, notify_conn, model_id_conn).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Failed to accept runtime connection: {}", e);
                        }
                    }
                }
                _ = self.shutdown_notify.notified() => {
                    break;
                }
            }
        }

        self.is_running.store(false, Ordering::SeqCst);
        let _ = worker_handle.await;

        // Clean up socket file on clean shutdown
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        Ok(())
    }
}

async fn handle_connection(
    stream: UnixStream,
    spine: Arc<Mutex<AienRuntimeSpine>>,
    is_running: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    model_id: Arc<String>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    while let Ok(n) = buf_reader.read_line(&mut line).await {
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        let resp = match serde_json::from_str::<ControlEnvelope>(trimmed) {
            Ok(envelope) => {
                if let ControlCommand::Generate(req) = envelope.command.clone() {
                    let result = stream_generation(&mut writer, &spine, &model_id, req).await;
                    if let Err(error) = result {
                        let _ = write_response(&mut writer, &ControlResponse::Error(error)).await;
                    }
                    line.clear();
                    continue;
                }
                let is_shutdown = matches!(envelope.command, ControlCommand::Shutdown);
                let response = {
                    let mut s = spine.lock().await;
                    s.handle_control_command(envelope)
                };
                if is_shutdown {
                    is_running.store(false, Ordering::SeqCst);
                    shutdown_notify.notify_waiters();
                }
                response
            }
            Err(e) => ControlResponse::Error(format!("Invalid control envelope JSON: {}", e)),
        };

        if write_response(&mut writer, &resp).await.is_err() {
            break;
        }
        line.clear();
    }
}

async fn write_response<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    response: &ControlResponse,
) -> Result<(), String> {
    let mut serialized = serde_json::to_string(response).map_err(|e| e.to_string())?;
    serialized.push('\n');
    writer
        .write_all(serialized.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

async fn stream_generation<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    spine: &Arc<Mutex<AienRuntimeSpine>>,
    model_id: &str,
    req: GenerateReq,
) -> Result<(), String> {
    if model_id == "unconfigured" || req.model_id != model_id {
        return Err(format!(
            "Runtime model is '{}', requested '{}'",
            model_id, req.model_id
        ));
    }
    if req.prompt_tokens.is_empty() || req.max_tokens == 0 || req.max_tokens > 4096 {
        return Err(
            "Generation requires prompt tokens and max_tokens between 1 and 4096".to_string(),
        );
    }
    if !req.temperature.is_finite() || !(0.0..=2.0).contains(&req.temperature) {
        return Err("Generation temperature must be between 0 and 2".to_string());
    }
    let (sink, mut events) = ChannelCompletionSink::channel();
    let (sink_id, seq_id) = {
        let mut runtime = spine.lock().await;
        let sink_id = runtime.register_completion_sink(Arc::new(sink));
        let prompt: PromptHandle = Arc::from(req.prompt_tokens);
        let sampling = SamplingParams {
            temperature: req.temperature,
            top_p: 1.0,
            max_tokens: req.max_tokens,
            stop_token_ids: vec![2],
        };
        let seq_id = match runtime.submit_work(prompt, sampling, 2, Some(sink_id)) {
            Ok(seq_id) => seq_id,
            Err(error) => {
                runtime
                    .scheduler
                    .completion_router_mut()
                    .unregister(sink_id);
                return Err(error);
            }
        };
        (sink_id, seq_id)
    };
    write_response(
        writer,
        &ControlResponse::GenerationStarted {
            model_id: model_id.to_string(),
            seq_id,
        },
    )
    .await?;
    let result = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(120), events.recv()).await {
            Ok(Some(CompletionEvent::Token { seq_id: id, token })) if id.to_u64() == seq_id => {
                if let Err(e) =
                    write_response(writer, &ControlResponse::GenerationToken { seq_id, token })
                        .await
                {
                    break Err(e);
                }
            }
            Ok(Some(CompletionEvent::Finished {
                seq_id: id,
                total_tokens,
                ..
            })) if id.to_u64() == seq_id => {
                break write_response(
                    writer,
                    &ControlResponse::GenerationFinished {
                        seq_id,
                        total_tokens,
                    },
                )
                .await;
            }
            Ok(Some(CompletionEvent::Error {
                seq_id: id,
                message,
            })) if id.to_u64() == seq_id => break Err(message),
            Ok(Some(_)) => continue,
            Ok(None) => break Err("Generation channel closed before completion".to_string()),
            Err(_) => break Err("Generation timed out".to_string()),
        }
    };
    spine
        .lock()
        .await
        .scheduler
        .completion_router_mut()
        .unregister(sink_id);
    result
}
