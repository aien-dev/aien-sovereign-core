//! Local UNIX domain socket client for AienRuntimeSpine
//! Used by aien-cli to communicate with the in-process runtime daemon.

use crate::control::{
    ControlCommand, ControlEnvelope, ControlResponse, LaunchSwarmReq, RuntimeStatusReport,
};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub struct AienRuntimeClient {
    socket_path: PathBuf,
}

impl AienRuntimeClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn default_socket_path() -> PathBuf {
        if let Ok(sock) = std::env::var("AIEN_RUNTIME_SOCK") {
            if !sock.trim().is_empty() {
                return PathBuf::from(sock.trim());
            }
        }
        PathBuf::from("/tmp/aien-runtime.sock")
    }

    pub fn default_client() -> Self {
        Self::new(Self::default_socket_path())
    }

    pub async fn is_alive(&self) -> bool {
        self.get_status().await.is_ok()
    }

    pub async fn send_command(&self, command: ControlCommand) -> Result<ControlResponse, String> {
        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            format!(
                "Failed to connect to AIEN runtime socket at {}: {}",
                self.socket_path.display(),
                e
            )
        })?;

        let (reader, mut writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        let envelope = ControlEnvelope {
            protocol_version: 1,
            request_id: now.as_millis() as u64,
            operation_id: now.as_nanos(),
            operator_session: 1,
            command,
        };

        let mut payload = serde_json::to_string(&envelope)
            .map_err(|e| format!("Failed to serialize ControlEnvelope: {}", e))?;
        payload.push('\n');

        writer
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("Failed to send control command: {}", e))?;

        let mut line = String::new();
        buf_reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Failed to read response from runtime socket: {}", e))?;

        serde_json::from_str::<ControlResponse>(line.trim())
            .map_err(|e| format!("Failed to parse ControlResponse: {}", e))
    }

    pub async fn get_status(&self) -> Result<RuntimeStatusReport, String> {
        match self.send_command(ControlCommand::GetRuntimeStatus).await? {
            ControlResponse::Status(report) => Ok(report),
            ControlResponse::Error(e) => Err(e),
            other => Err(format!(
                "Unexpected response for GetRuntimeStatus: {:?}",
                other
            )),
        }
    }

    pub async fn launch_swarm(&self, req: LaunchSwarmReq) -> Result<u64, String> {
        match self.send_command(ControlCommand::LaunchSwarm(req)).await? {
            ControlResponse::SwarmAccepted { swarm_id, .. } => Ok(swarm_id),
            ControlResponse::Error(e) => Err(e),
            other => Err(format!("Unexpected response for LaunchSwarm: {:?}", other)),
        }
    }

    pub async fn cancel_swarm(&self, swarm_id: u64) -> Result<(), String> {
        match self
            .send_command(ControlCommand::CancelSwarm(swarm_id))
            .await?
        {
            ControlResponse::SwarmCancelled { .. } => Ok(()),
            ControlResponse::Error(e) => Err(e),
            other => Err(format!("Unexpected response for CancelSwarm: {:?}", other)),
        }
    }

    pub async fn inspect_swarm(&self, swarm_id: u64) -> Result<RuntimeStatusReport, String> {
        match self
            .send_command(ControlCommand::InspectSwarm(swarm_id))
            .await?
        {
            ControlResponse::Status(report) => Ok(report),
            ControlResponse::Error(e) => Err(e),
            other => Err(format!("Unexpected response for InspectSwarm: {:?}", other)),
        }
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        match self.send_command(ControlCommand::Shutdown).await? {
            ControlResponse::ShutdownAck => Ok(()),
            ControlResponse::Error(e) => Err(e),
            other => Err(format!("Unexpected response for Shutdown: {:?}", other)),
        }
    }
}
