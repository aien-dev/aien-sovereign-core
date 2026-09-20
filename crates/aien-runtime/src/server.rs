//! Local UNIX domain socket server for AienRuntimeSpine
//! Exposes typed control RPC over local IPC to operator CLI tools.

use crate::control::{ControlCommand, ControlEnvelope, ControlResponse};
use crate::spine::AienRuntimeSpine;
use aien_inference_abi::AienInferenceBackend;
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

                            tokio::spawn(async move {
                                handle_connection(stream, spine_conn, is_running_conn, notify_conn).await;
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

        if let Ok(mut serialized) = serde_json::to_string(&resp) {
            serialized.push('\n');
            if writer.write_all(serialized.as_bytes()).await.is_err() {
                break;
            }
        }
        line.clear();
    }
}
