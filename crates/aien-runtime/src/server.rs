//! Local UNIX domain socket server for AienRuntimeSpine
//! Exposes typed control RPC over local IPC to operator CLI tools.

use crate::control::{ControlCommand, ControlEnvelope, ControlResponse};
use crate::spine::AienRuntimeSpine;
use aien_inference_abi::{AienInferenceBackend, SamplingParams, TinyLlamaTokenizer};
use aien_scheduler::{ChannelCompletionSink, CompletionEvent};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify};

pub struct AienRuntimeServer {
    socket_path: PathBuf,
    spine: Arc<Mutex<AienRuntimeSpine>>,
    shutdown_notify: Arc<Notify>,
    is_running: Arc<AtomicBool>,
    tokenizer: Arc<RwLock<Option<TinyLlamaTokenizer>>>,
}

impl AienRuntimeServer {
    pub fn new(spine: AienRuntimeSpine, socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            spine: Arc::new(Mutex::new(spine)),
            shutdown_notify: Arc::new(Notify::new()),
            is_running: Arc::new(AtomicBool::new(false)),
            tokenizer: Arc::new(RwLock::new(None)),
        }
    }

    /// Installs the tokenizer that `StreamTurn` uses to encode prompts and decode tokens.
    pub fn set_tokenizer(&self, tokenizer: TinyLlamaTokenizer) {
        *self.tokenizer.write().expect("tokenizer lock") = Some(tokenizer);
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
        mut backend: B,
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

                            let tokenizer_conn = self.tokenizer.clone();
                            tokio::spawn(async move {
                                handle_connection(
                                    stream,
                                    spine_conn,
                                    is_running_conn,
                                    notify_conn,
                                    tokenizer_conn,
                                )
                                .await;
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

async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response: &ControlResponse,
) -> bool {
    match serde_json::to_string(response) {
        Ok(mut serialized) => {
            serialized.push('\n');
            writer.write_all(serialized.as_bytes()).await.is_ok()
        }
        Err(_) => false,
    }
}

fn text_suffix(previous: &str, decoded: &str) -> String {
    decoded.strip_prefix(previous).unwrap_or("").to_string()
}

async fn stream_turn(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    spine: Arc<Mutex<AienRuntimeSpine>>,
    tokenizer: Arc<RwLock<Option<TinyLlamaTokenizer>>>,
    messages: Vec<crate::control::ChatTurn>,
    max_tokens: usize,
    temperature: f32,
) {
    let tokenizer = {
        let guard = tokenizer.read().expect("tokenizer lock");
        guard.clone()
    };
    let Some(tokenizer) = tokenizer else {
        let _ = write_response(
            writer,
            &ControlResponse::Error(
                "tokenizer is not loaded; native chat cannot encode the prompt".into(),
            ),
        )
        .await;
        return;
    };
    let prompt = crate::control::format_tinyllama_chat(&messages);
    let tokens = match tokenizer.encode(&prompt) {
        Ok(tokens) => tokens,
        Err(error) => {
            let _ = write_response(
                writer,
                &ControlResponse::Error(format!("tokenizer encode failed: {error}")),
            )
            .await;
            return;
        }
    };
    let (sink, mut events) = ChannelCompletionSink::channel();
    let submitted = {
        let mut spine = spine.lock().await;
        let sink_id = spine.register_completion_sink(std::sync::Arc::new(sink));
        let sampling = SamplingParams {
            temperature,
            top_p: 0.95,
            max_tokens: max_tokens.max(1),
            stop_token_ids: vec![TinyLlamaTokenizer::EOS_TOKEN_ID],
        };
        spine.submit_work(
            std::sync::Arc::from(tokens.as_slice()),
            sampling,
            2,
            Some(sink_id),
        )
    };
    if let Err(error) = submitted {
        let _ = write_response(writer, &ControlResponse::Error(error)).await;
        return;
    }

    let mut produced = Vec::new();
    let mut text = String::new();
    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(120), events.recv()).await;
        match next {
            Ok(Some(CompletionEvent::Token { token, .. })) => {
                produced.push(token);
                let decoded = tokenizer
                    .decode_opts(&produced, true)
                    .unwrap_or_else(|_| text.clone());
                let delta = text_suffix(&text, &decoded);
                if decoded.len() >= text.len() && decoded.starts_with(&text) {
                    text = decoded;
                }
                if !delta.is_empty()
                    && !write_response(writer, &ControlResponse::TurnDelta { text: delta }).await
                {
                    return;
                }
            }
            Ok(Some(CompletionEvent::Finished { total_tokens, .. })) => {
                let _ = write_response(
                    writer,
                    &ControlResponse::TurnFinished { text, total_tokens },
                )
                .await;
                return;
            }
            Ok(Some(CompletionEvent::Error { message, .. })) => {
                let _ = write_response(writer, &ControlResponse::Error(message)).await;
                return;
            }
            Ok(None) | Err(_) => {
                let _ = write_response(
                    writer,
                    &ControlResponse::Error(
                        "native runtime stopped streaming before the turn finished".into(),
                    ),
                )
                .await;
                return;
            }
        }
    }
}

async fn handle_connection(
    stream: UnixStream,
    spine: Arc<Mutex<AienRuntimeSpine>>,
    is_running: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    tokenizer: Arc<RwLock<Option<TinyLlamaTokenizer>>>,
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

        let parsed = serde_json::from_str::<ControlEnvelope>(trimmed);
        let envelope = match parsed {
            Ok(envelope) => envelope,
            Err(e) => {
                if !write_response(
                    &mut writer,
                    &ControlResponse::Error(format!("Invalid control envelope JSON: {}", e)),
                )
                .await
                {
                    break;
                }
                line.clear();
                continue;
            }
        };

        if let ControlCommand::StreamTurn {
            messages,
            max_tokens,
            temperature,
        } = envelope.command
        {
            stream_turn(
                &mut writer,
                spine.clone(),
                tokenizer.clone(),
                messages,
                max_tokens,
                temperature,
            )
            .await;
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
        if !write_response(&mut writer, &response).await {
            break;
        }
        line.clear();
    }
}
