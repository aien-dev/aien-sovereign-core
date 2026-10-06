//! Canonical AienRuntimeSpine
//! Unifies SequenceArena, AienScheduler, AienKvManager, WorldStore, ContextComposer,
//! and SwarmManager in a single process address space.

use crate::context::ContextComposer;
use crate::control::{
    ControlCommand, ControlEnvelope, ControlResponse, RuntimeController, RuntimeStatusReport,
};
use crate::sequence::{SequenceArena, SequenceId, SequenceState};
use crate::swarm::{SwarmConfig, SwarmManager, SwarmState};
use crate::world::WorldStore;

use aien_abi_core::{AienInferenceBackend, SamplingParams, StepMetrics};
use aien_kv_cache::AienKvManager;
use aien_scheduler::{
    AienScheduler, CompletionSink, CompletionSinkId, PromptHandle, SchedulerConfig,
};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct AienRuntimeSpine {
    pub arena: SequenceArena,
    pub kv_manager: Arc<RwLock<AienKvManager>>,
    pub scheduler: AienScheduler,
    pub world_store: WorldStore,
    pub context_composer: ContextComposer,
    pub swarm_manager: SwarmManager,
    pub controller: RuntimeController,
    pub step_counter: u64,
    /// Subagent forks whose KV is already forked but whose backend state is
    /// not yet: (parent, child) KV ids, applied via the backend's
    /// `fork_sequence` at the start of the next `step`, before any decode.
    pending_backend_forks: Vec<(u64, u64)>,
    /// PREFILL-E2E C5: sequence ids whose KV and arena slots were reclaimed
    /// without a backend in reach (swarm cancel). Their backend per-sequence
    /// state is released by `release_pending_backend_sequences`, which `step`
    /// calls first.
    pending_backend_releases: Vec<u64>,
}

impl AienRuntimeSpine {
    pub fn new(
        arena_capacity: usize,
        scheduler_config: SchedulerConfig,
        kv_manager: Arc<RwLock<AienKvManager>>,
    ) -> Self {
        let scheduler = AienScheduler::new(scheduler_config, kv_manager.clone());
        Self {
            arena: SequenceArena::new(arena_capacity),
            kv_manager,
            scheduler,
            world_store: WorldStore::new(),
            context_composer: ContextComposer::new(),
            swarm_manager: SwarmManager::new(),
            controller: RuntimeController::new(),
            step_counter: 0,
            pending_backend_forks: Vec::new(),
            pending_backend_releases: Vec::new(),
        }
    }

    /// Submits structured InferenceWork with PromptHandle and optional completion sink.
    pub fn submit_inference_work(
        &mut self,
        work: aien_platform::queue::InferenceWork,
        prompt: PromptHandle,
        sampling_params: Option<SamplingParams>,
        sink_id: Option<CompletionSinkId>,
    ) -> Result<(), String> {
        self.scheduler
            .submit_work(work, prompt, sampling_params, sink_id)
            .map(|_| ())
    }

    /// Submits a prompt ticket to the scheduler for execution, constructing default InferenceWork.
    pub fn submit_work(
        &mut self,
        prompt: PromptHandle,
        sampling_params: SamplingParams,
        priority: u8,
        sink_id: Option<CompletionSinkId>,
    ) -> Result<u64, String> {
        self.step_counter += 1;
        let seq_id = self.step_counter;
        let plat_priority = match priority {
            0 => aien_platform::Priority::Background,
            1 => aien_platform::Priority::Normal,
            2 => aien_platform::Priority::Interactive,
            _ => aien_platform::Priority::Realtime,
        };
        let work = aien_platform::queue::InferenceWork {
            sequence: seq_id,
            model: aien_platform::ModelHandle(1),
            kv: aien_platform::queue::KvHandle(seq_id),
            priority: plat_priority,
            deadline: None,
            branch_parent: None,
            next_token_budget: sampling_params.max_tokens as u32,
        };
        let assigned = self
            .scheduler
            .submit_work(work, prompt, Some(sampling_params), sink_id)?;
        Ok(assigned.to_u64())
    }

    /// Registers a completion sink for streaming output events.
    pub fn register_completion_sink(&mut self, sink: Arc<dyn CompletionSink>) -> CompletionSinkId {
        self.scheduler.register_completion_sink(sink)
    }

    /// Zero-copy subagent sequence branching with completion sink routing.
    pub fn fork_subagent(
        &mut self,
        parent_id: u64,
        child_id: u64,
        sink_id: Option<CompletionSinkId>,
    ) -> Result<(), String> {
        let ids = self
            .scheduler
            .fork_subagent_with_sink_ids(parent_id, child_id, sink_id)?;
        // The backend is not reachable here; its state is forked at the start
        // of the next step, before the child can be decoded.
        self.pending_backend_forks.push(ids);
        Ok(())
    }

    /// Executes runtime steps in a closed loop until all active requests complete or max_steps is reached.
    pub async fn run_until_complete<B: AienInferenceBackend>(
        &mut self,
        backend: &mut B,
        max_steps: usize,
    ) -> Result<Vec<StepMetrics>, String> {
        let mut all_metrics = Vec::new();
        for _ in 0..max_steps {
            if self.scheduler.waiting_count() == 0
                && self.scheduler.running_count() == 0
                && self.scheduler.preempted_count() == 0
            {
                break;
            }
            if let Some(metrics) = self.step(backend).await? {
                all_metrics.push(metrics);
            }
        }
        Ok(all_metrics)
    }

    /// Advances the engine by one transactional step.
    pub async fn step<B: AienInferenceBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<Option<StepMetrics>, String> {
        self.step_counter += 1;

        // PREFILL-E2E C5: backend state of sequences cancelled since the last
        // step; their KV is already freed. CancelSwarm also frees them in the
        // scheduler arena, so queued copies are stale and never re-admitted.
        self.release_pending_backend_sequences(backend)?;

        // PREFILL-E2E C4: backend state for subagent forks made since the
        // last step, before the scheduler can decode the children.
        for (parent, child) in std::mem::take(&mut self.pending_backend_forks) {
            backend.fork_sequence(parent, child)?;
        }

        // PREFILL-GATE: run the model prefill over each pending swarm root
        // prompt BEFORE any branch can be scheduled, then share the computed
        // root blocks with the branches. Branches never see root blocks that
        // have not passed the completion fence.
        let root_prefill_tokens = self.prefill_pending_swarm_roots(backend).await?;

        // Execute scheduler step via backend
        let step_result = self.scheduler.step(backend).await?;

        if step_result.is_none() && root_prefill_tokens > 0 {
            return Ok(Some(StepMetrics {
                prefill_tokens_processed: root_prefill_tokens,
                active_kv_blocks: self.kv_manager.read().allocated_block_count(),
                ..StepMetrics::default()
            }));
        }

        if let Some((outputs, mut metrics)) = step_result {
            metrics.prefill_tokens_processed += root_prefill_tokens;
            // Generational SequenceId validation on step completion
            for output in &outputs {
                match output {
                    aien_abi_core::DecodeOutput::Token {
                        request_id,
                        token_id: _,
                        logprob: _,
                    } => {
                        let seq_id = SequenceId::from_u64(*request_id);
                        if let Some(record) = self.arena.get_mut(seq_id) {
                            record.generated_tokens += 1;
                        }
                    }
                    aien_abi_core::DecodeOutput::Finished {
                        request_id,
                        reason: _,
                        total_tokens: _,
                    } => {
                        let seq_id = SequenceId::from_u64(*request_id);
                        if let Some(record) = self.arena.get_mut(seq_id) {
                            record.state = SequenceState::Completed;
                        }
                        self.arena.free(seq_id);
                        let reclaimed_root = {
                            let mut kv = self.kv_manager.write();
                            self.swarm_manager.note_sequence_finished(
                                seq_id,
                                &mut self.arena,
                                &mut kv,
                                &mut self.world_store,
                            )
                        };
                        // PREFILL-E2E C5 (bullet 12): drop the backend's
                        // per-sequence state. Safe order: this runs after
                        // execute_step returned, so the backend no longer
                        // reads this sequence's KV (the scheduler already
                        // freed it), and the KV lock is released first. The
                        // root goes last, once its final branch finished:
                        // branches hold their own copy of the root's tokens
                        // (fork_sequence) and never read the root's state.
                        backend.release_sequence(*request_id)?;
                        if let Some(root) = reclaimed_root {
                            backend.release_sequence(root.as_u64())?;
                        }
                    }
                }
            }
            Ok(Some(metrics))
        } else {
            Ok(None)
        }
    }

    /// PREFILL-E2E C5: releases the backend per-sequence state of every
    /// sequence reclaimed without a backend in reach (swarm cancel). Runs at
    /// the start of each `step`; callers that stop stepping after a cancel
    /// call it directly. Returns the number of ids released.
    pub fn release_pending_backend_sequences<B: AienInferenceBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<usize, String> {
        let ids = std::mem::take(&mut self.pending_backend_releases);
        // A cancelled sequence must not be forked from or into afterwards.
        self.pending_backend_forks
            .retain(|(parent, child)| !ids.contains(parent) && !ids.contains(child));
        for &id in &ids {
            backend.release_sequence(id)?;
        }
        Ok(ids.len())
    }

    /// Prefills every pending swarm root through the scheduler's completion
    /// fence and, once the root is PrefillReady, shares its blocks with the
    /// branches. Returns the number of root prompt tokens prefilled.
    async fn prefill_pending_swarm_roots<B: AienInferenceBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<usize, String> {
        let mut prefilled = 0usize;
        for (swarm_id, root_kv_id, prompt) in self.swarm_manager.pending_root_prefills() {
            if let Some(m) = self
                .scheduler
                .prefill_detached(backend, root_kv_id, &prompt)
                .await?
            {
                prefilled += m.prefill_tokens_processed;
            }
            {
                let mut kv = self.kv_manager.write();
                self.swarm_manager.fork_branches_from_ready_root(
                    swarm_id,
                    &mut self.arena,
                    &mut kv,
                )?;
            }
            // PREFILL-E2E C4 (bullets 6-8): fork the backend's per-sequence
            // state (tokens, position, last token) for each branch right after
            // the physical KV fork and before any branch can be scheduled for
            // decode. The KV lock is released first: the backend reads the
            // same shared KV manager to check the parent is PrefillReady.
            // PREFILL-E2E C6 (bullet 11): the fork hook also hands each
            // branch its own sampling params (the same ones its scheduler
            // request carries), so branches decode with temperature and a
            // per-branch seed instead of all taking the argmax.
            let (branches, sampling): (Vec<u64>, SamplingParams) = self
                .swarm_manager
                .get_swarm(swarm_id)
                .map(|s| {
                    (
                        s.branch_sequences.iter().map(|b| b.as_u64()).collect(),
                        branch_sampling_params(&s.config),
                    )
                })
                .unwrap_or_else(|| (Vec::new(), SamplingParams::default()));
            for child in branches {
                backend.fork_sequence_with_sampling(root_kv_id, child, &sampling)?;
            }
        }
        Ok(prefilled)
    }

    /// Launches a swarm of agents sharing an immutable root World and physical KV blocks.
    /// Branch KV is shared from the root only after the root prompt prefill
    /// completes (next `step`); until then branches cannot be decoded.
    pub fn launch_swarm(
        &mut self,
        config: SwarmConfig,
        prompt_tokens: &[u32],
    ) -> Result<u64, String> {
        let mut kv = self.kv_manager.write();
        let swarm_id = self.swarm_manager.launch_swarm(
            config,
            &mut self.arena,
            &mut kv,
            &mut self.world_store,
            prompt_tokens,
            self.step_counter,
        )?;

        // Enqueue branch requests into scheduler
        if let Some(swarm) = self.swarm_manager.get_swarm(swarm_id) {
            for &child_seq in &swarm.branch_sequences {
                let req = aien_abi_core::SequenceRequest {
                    request_id: child_seq.as_u64(),
                    prompt_tokens: prompt_tokens.to_vec(),
                    sampling_params: branch_sampling_params(&swarm.config),
                    arrival_time_ns: self.step_counter,
                    priority: swarm.config.priority,
                };
                self.scheduler.submit_request(req);
            }
        }

        Ok(swarm_id)
    }

    /// Handles a typed operator command with idempotency verification.
    pub fn handle_control_command(&mut self, envelope: ControlEnvelope) -> ControlResponse {
        if self
            .controller
            .is_operation_processed(envelope.operation_id)
        {
            return ControlResponse::Error(format!(
                "Operation {} already processed",
                envelope.operation_id
            ));
        }

        let resp = match envelope.command {
            ControlCommand::LaunchSwarm(req) => {
                let config = SwarmConfig {
                    model_handle: req.model_handle,
                    branch_count: req.branch_count,
                    max_active_sequences: req.max_active_sequences,
                    max_tokens_per_branch: req.max_tokens_per_branch,
                    root_world_id: req.root_world_id,
                    priority: req.priority,
                };
                match self.launch_swarm(config, &req.prompt_tokens) {
                    Ok(swarm_id) => {
                        self.controller
                            .mark_operation_processed(envelope.operation_id);
                        ControlResponse::SwarmAccepted {
                            swarm_id,
                            operation_id: envelope.operation_id,
                        }
                    }
                    Err(e) => ControlResponse::Error(e),
                }
            }
            ControlCommand::CancelSwarm(swarm_id) => {
                let mut kv = self.kv_manager.write();
                match self.swarm_manager.cancel_swarm(
                    swarm_id,
                    &mut self.arena,
                    &mut kv,
                    &mut self.world_store,
                ) {
                    Ok(released) => {
                        // KV, arena slots and branch worlds are reclaimed;
                        // backend state follows on the next step (C5).
                        // The scheduler keeps its own arena, with the same ids
                        // (submit_request inserts them explicitly). Free them
                        // there too, so queued or running copies turn stale
                        // and are never admitted or decoded again.
                        for &id in &released {
                            if let Ok(sid) = aien_scheduler::sequence::SequenceId::from_u64(id) {
                                self.scheduler.arena_mut().free_sequence(sid);
                            }
                        }
                        self.pending_backend_releases.extend(released);
                        self.controller
                            .mark_operation_processed(envelope.operation_id);
                        ControlResponse::SwarmCancelled { swarm_id }
                    }
                    Err(e) => ControlResponse::Error(e),
                }
            }
            ControlCommand::GetRuntimeStatus => ControlResponse::Status(self.status_report()),
            ControlCommand::InspectSwarm(swarm_id) => match self.swarm_status_report(swarm_id) {
                Some(report) => ControlResponse::Status(report),
                None => ControlResponse::Error(format!("Swarm {} not found", swarm_id)),
            },
            ControlCommand::StreamTurn { .. } => ControlResponse::Error(
                "StreamTurn is handled on the socket connection, not as a one-shot command".into(),
            ),
            ControlCommand::RunComposeTask { .. }
            | ControlCommand::ComposeNote { .. }
            | ControlCommand::ComposeRecall { .. }
            | ControlCommand::RecoverComposeHome
            | ControlCommand::ComposeEffectIntent { .. }
            | ControlCommand::ComposeEffectAck { .. }
            | ControlCommand::ComposeReconcile { .. }
            | ControlCommand::ComposeControl { .. } => ControlResponse::Error(
                "compose commands are handled on the socket connection, not as one-shot commands"
                    .into(),
            ),
            ControlCommand::Shutdown => {
                self.controller
                    .mark_operation_processed(envelope.operation_id);
                ControlResponse::ShutdownAck
            }
        };

        resp
    }

    /// Status scoped to one swarm. Sequence, swarm, and world counts cover only
    /// this swarm; KV block and page fields describe the shared runtime pool.
    pub fn swarm_status_report(&self, swarm_id: u64) -> Option<RuntimeStatusReport> {
        let swarm = self.swarm_manager.get_swarm(swarm_id)?;
        let running = swarm.state == SwarmState::Running;
        let active_sequences = if running {
            swarm
                .branch_sequences
                .len()
                .saturating_sub(swarm.finished_branches.len())
        } else {
            0
        };
        let active_worlds = swarm
            .branch_worlds
            .iter()
            .filter(|&&w| self.world_store.get_world(w).is_some())
            .count();
        Some(RuntimeStatusReport {
            active_sequences,
            active_swarms: usize::from(running),
            active_worlds,
            ..self.status_report()
        })
    }

    pub fn status_report(&self) -> RuntimeStatusReport {
        let kv = self.kv_manager.read();
        let metrics = kv.metrics();
        let free = kv.free_block_count();
        let allocated = kv.allocated_block_count();
        let total = free + allocated;
        // Honest proxy until the GB10 telemetry hook lands: KV memory pressure
        // as a percentage. This is memory pressure, not SM activity.
        let pressure_pct = if total > 0 {
            (allocated as f32 / total as f32) * 100.0
        } else {
            0.0
        };

        RuntimeStatusReport {
            active_sequences: self.arena.active_count(),
            active_swarms: self.swarm_manager.active_swarm_count(),
            active_worlds: self.world_store.active_world_count(),
            free_kv_blocks: free,
            total_kv_blocks: total,
            shared_kv_pages: metrics.shared_pages,
            cow_faults: metrics.cow_faults,
            gpu_utilization_pct: pressure_pct,
        }
    }
}

/// Sampling params of every swarm branch: submitted with the branch's
/// scheduler request (`launch_swarm`) and handed to the backend by the fork
/// hook (`prefill_pending_swarm_roots`), so both sides agree.
fn branch_sampling_params(config: &SwarmConfig) -> SamplingParams {
    SamplingParams {
        temperature: 0.7,
        top_p: 0.95,
        max_tokens: config.max_tokens_per_branch,
        stop_token_ids: vec![2],
    }
}

// ---------------------------------------------------------------------------
// NEXT-PHASE-1 cut 1b: the compose bridge (omega COMPOSITION-2 via
// aien-omega-compose). One composition home per process, opened on first use
// and kept open: the Cortex journal takes an exclusive writer lock.
// ---------------------------------------------------------------------------

use crate::control::{
    ComposeNoteReport, ComposeRecallReport, ComposeRecordView, ComposeRecoverReport,
    ComposeTaskReport, ProposalAttempt,
};
use crate::cortex_mark::{self, Mark, MarkRefusal, Plan};
use aien_omega_compose::{hex, note_bytes, Compose, ComposeError, NoteKind, RootKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One model reply: the text and how many tokens were generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    pub text: String,
    pub tokens: usize,
    /// How generation stopped: "eos" | "max_tokens" | "aborted" | "preempted"
    /// (ACCEPTANCE-v5 Q3); None when unknown.
    pub finish_reason: Option<String>,
}

/// The "model" Skill's work: prompt text and a wall limit in, one reply
/// out. The server installs the real inference path (the one `StreamTurn`
/// uses); tests install a deterministic proposer.
pub type ComposeProposer =
    Arc<dyn Fn(&str, std::time::Duration) -> Result<Generation, String> + Send + Sync>;

/// The one Skill name; its procedure digest is sha256 of this name.
pub const COMPOSE_MODEL_SKILL: &str = "aien.model.propose-file-change";

/// Composition home: `$AIEN_COMPOSE_DIR`, else `$AIEN_RUNTIME_STATE_DIR/compose`,
/// else `$XDG_STATE_HOME/aien-runtime/compose`, else
/// `$HOME/.local/state/aien-runtime/compose`. Never /tmp.
pub fn compose_dir_from_env() -> Result<PathBuf, String> {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    if let Some(d) = get("AIEN_COMPOSE_DIR") {
        return Ok(PathBuf::from(d.trim()));
    }
    if let Some(d) = get("AIEN_RUNTIME_STATE_DIR") {
        return Ok(PathBuf::from(d.trim()).join("compose"));
    }
    if let Some(d) = get("XDG_STATE_HOME") {
        return Ok(PathBuf::from(d.trim()).join("aien-runtime/compose"));
    }
    if let Some(h) = get("HOME") {
        return Ok(PathBuf::from(h.trim()).join(".local/state/aien-runtime/compose"));
    }
    Err(
        "no AIEN_COMPOSE_DIR, AIEN_RUNTIME_STATE_DIR, XDG_STATE_HOME or HOME for the compose home"
            .into(),
    )
}

/// The provisioned machine root: 32 random bytes kept beside the home
/// (`<home>.machine-root`, mode 0600), created once. STOPGAP until AIENOS
/// machine provisioning owns the AienMachineId root.
fn machine_root(dir: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut name = dir.as_os_str().to_owned();
    name.push(".machine-root");
    let path = PathBuf::from(name);
    if let Ok(b) = std::fs::read(&path) {
        if b.len() == 32 {
            return Ok(b);
        }
        return Err(format!(
            "{} holds {} bytes, expected 32",
            path.display(),
            b.len()
        ));
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("create {}: {e}", p.display()))?;
    }
    let mut root = vec![0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut root))
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    std::io::Write::write_all(&mut f, &root)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    f.sync_all()
        .map_err(|e| format!("sync {}: {e}", path.display()))?;
    Ok(root)
}

fn proposal_handle(text: &str) -> u64 {
    let d = Sha256::digest(text.as_bytes());
    let h = u64::from_le_bytes(d[..8].try_into().expect("8 bytes"));
    if h == 0 {
        1
    } else {
        h
    }
}

/// A named refusal for an rxc_host open error: the operator learns what is
/// wrong and that `RecoverComposeHome` is the remedy (never a silent repair).
fn refusal(dir: &Path, e: &ComposeError) -> String {
    let why = match e {
        ComposeError::Code { code: -3, .. } => {
            "Cortex journal tail torn (E_TORN, CX_ERR_TORN); nothing was cut. Run RecoverComposeHome (aien compose recover)"
                .to_string()
        }
        ComposeError::Code { code: -11, detail } => format!(
            "Cortex journal behind its J-Space anchor (E_REPLAY, RX_ERR_REPLAY {detail}). Run RecoverComposeHome (aien compose recover)"
        ),
        other => other.to_string(),
    };
    format!("compose home {} refused: {why}", dir.display())
}

/// A refusal by the Cortex record mark (ACCEPTANCE-v3 2.2), in the same form.
fn mark_refusal(dir: &Path, r: &MarkRefusal) -> String {
    format!("compose home {} refused: {r}", dir.display())
}

/// One proposed file change: a path relative to the workspace and the
/// complete new content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileProposal {
    pub path: String,
    pub content: String,
}

fn bare(s: &str) -> &str {
    s.trim()
        .trim_matches(|c| matches!(c, '`' | '"' | '\'' | '*'))
        .trim()
}

/// A path the workspace may hold: relative, at most 255 bytes, letters,
/// digits and `._-/`, no empty, `.` or `..` component, not absolute, not `~`.
fn check_relative_path(p: &str) -> Result<(), String> {
    if p.is_empty() {
        return Err("empty path on the filename line".into());
    }
    if p.starts_with('/') || p.starts_with('~') || p.split('/').any(|c| c == "..") {
        return Err(format!("path {p:?} is outside the workspace"));
    }
    let ok = p.len() <= 255
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
        && p.split('/').all(|c| !c.is_empty() && c != ".");
    if ok {
        Ok(())
    } else {
        Err(format!("path {p:?} is not a plain relative path"))
    }
}

/// The model's answer in the fixed proposal template: the first nonempty
/// line is `filename: <relative path>` (case-insensitive key; quotes,
/// backticks and asterisks around it or the path are ignored), everything
/// after it is the complete content (blank lines around it and one
/// surrounding code fence are dropped; it ends with one newline).
/// Err names why the reply is refused. Deterministic: the Skill, the AEGIS
/// contract, the authorization and the write all use this one reading.
pub fn check_file_proposal(text: &str) -> Result<FileProposal, String> {
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .ok_or("empty reply")?;
    let first = bare(lines[at]);
    const KEY: &str = "filename:";
    if first.len() < KEY.len() || !first[..KEY.len()].eq_ignore_ascii_case(KEY) {
        return Err("no filename line: the first line must be 'filename: <relative path>'".into());
    }
    let path = bare(&first[KEY.len()..]).to_string();
    check_relative_path(&path)?;
    let mut body: Vec<&str> = lines[at + 1..].to_vec();
    while body.first().is_some_and(|l| l.trim().is_empty()) {
        body.remove(0);
    }
    if body
        .first()
        .is_some_and(|l| l.trim_start().starts_with("```"))
    {
        body.remove(0);
        if let Some(end) = body.iter().position(|l| l.trim_start().starts_with("```")) {
            body.truncate(end);
        }
    }
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    if body.iter().all(|l| l.trim().is_empty()) {
        return Err("empty content after the filename line".into());
    }
    let mut content = body.join("\n");
    content.push('\n');
    Ok(FileProposal { path, content })
}

/// `check_file_proposal` without the reason.
pub fn parse_file_proposal(text: &str) -> Option<FileProposal> {
    check_file_proposal(text).ok()
}

/// At most this many proposals per task (ACCEPTANCE-v2 Section 3b).
pub const COMPOSE_MAX_ATTEMPTS: u32 = 3;
/// Wall budget for all attempts of one task: rx_compose_run's 30 s
/// quiescence wait (not raised) less a 1 s margin (ACCEPTANCE-v3 Section 3b).
pub const COMPOSE_SKILL_BUDGET: std::time::Duration = std::time::Duration::from_secs(29);
/// Time one full attempt needs, from the v3 measurement: 173-token retry
/// prompt prefill (2 839 + 896 ms) + 47 decode steps x 166.6 ms + 60 ms
/// = 11 625 ms, rounded up (ACCEPTANCE-v3 Section 3b). Attempt k > 1 starts
/// only if at least this much budget is left.
pub const COMPOSE_ATTEMPT_BUDGET: std::time::Duration = std::time::Duration::from_millis(12_000);

/// Assistant-response prefix of the production RunComposeTask template: the
/// assistant turn starts with it and the model generates the path and the
/// content after it (ACCEPTANCE-v4 Section 2(1)). The reply the parser reads
/// is this prefix followed by the generated text.
///
/// NEXT-PHASE-1 v5 engine cut: no trailing space. A separate space token after
/// the colon was measured harmful; the model's own tokenization puts the space
/// in front of the path.
pub const COMPOSE_ASSISTANT_PREFIX: &str = "filename:";

/// The fixed proposal template of the production RunComposeTask path.
pub fn proposal_prompt(goal: &str, workspace: &str) -> String {
    format!(
        "Goal: {goal}\nAuthorized workspace: {workspace}\n\
         Propose exactly one file change inside the workspace.\n\
         Answer in exactly this format and nothing else:\n\
         filename: <relative path>\n\
         <the complete new file content>"
    )
}

/// The one correction line added to attempt k > 1.
pub fn retry_prompt(base: &str, reason: &str) -> String {
    format!(
        "{base}\nYour previous answer was refused ({reason}). \
         Start your answer with the line \"filename: <relative path>\"."
    )
}

/// Runs up to `max_attempts` proposals within `budget`: attempt 1 always
/// starts; attempt k > 1 starts only if the remaining budget is at least
/// `attempt_budget` (the measured cost of one full attempt), and gets the
/// remaining budget as its limit. Returns the first reply that passes
/// `check_file_proposal` (or the last refusal reason) and every attempt made.
pub fn propose_with_retries(
    proposer: &(dyn Fn(&str, std::time::Duration) -> Result<Generation, String> + Send + Sync),
    base: &str,
    budget: std::time::Duration,
    attempt_budget: std::time::Duration,
    max_attempts: u32,
) -> (Result<String, String>, Vec<ProposalAttempt>) {
    let start = std::time::Instant::now();
    let mut attempts: Vec<ProposalAttempt> = Vec::new();
    let mut last_reason = "no attempt made".to_string();
    let need_ms = (attempt_budget.as_millis() as u64).max(1);
    for k in 1..=max_attempts {
        let remaining = budget.saturating_sub(start.elapsed());
        if k > 1 && (remaining.as_millis() as u64) < need_ms {
            last_reason = format!(
                "{last_reason}; no attempt {k}: {} ms left < {need_ms} ms per-attempt budget",
                remaining.as_millis()
            );
            break;
        }
        let prompt = if k == 1 {
            base.to_string()
        } else {
            retry_prompt(base, &last_reason)
        };
        let t0 = std::time::Instant::now();
        let out = proposer(&prompt, remaining);
        let last_ms = t0.elapsed().as_millis() as u64;
        let mut a = ProposalAttempt {
            attempt: k,
            ms: last_ms,
            tokens: 0,
            outcome: String::new(),
            reason: None,
            text_sha256: None,
            text: None,
            aegis: None,
            finish_reason: None,
        };
        match out {
            Ok(g) => {
                a.tokens = g.tokens;
                a.finish_reason = g.finish_reason.clone();
                a.text_sha256 = Some(hex(&Sha256::digest(g.text.as_bytes())));
                a.text = Some(g.text.clone());
                match check_file_proposal(&g.text) {
                    Ok(_) => {
                        a.outcome = "parsed".into();
                        attempts.push(a);
                        return (Ok(g.text), attempts);
                    }
                    Err(e) => {
                        a.outcome = "refused".into();
                        a.reason = Some(e.clone());
                        last_reason = e;
                    }
                }
            }
            Err(e) => {
                a.outcome = if e.contains("exceeded") {
                    "timeout"
                } else {
                    "error"
                }
                .into();
                a.reason = Some(e.clone());
                last_reason = e;
            }
        }
        attempts.push(a);
    }
    (Err(last_reason), attempts)
}

pub(crate) fn record_view(
    compose: &mut Compose,
    r: &aien_omega_compose::Record,
) -> ComposeRecordView {
    let host = r.subject == aien_omega_compose::SUBJECT_HOST;
    let note = if !host {
        None
    } else if r.tag == aien_omega_compose::ffi::RXC_HOST_TAG_REPAIR_TAIL {
        Some("repair_tail".to_string())
    } else {
        NoteKind::from_tag(r.tag).map(|k| k.name().to_string())
    };
    let text = if host && NoteKind::from_tag(r.tag).is_some() {
        compose
            .payload(r.id)
            .ok()
            .and_then(|p| note_bytes(&p))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    } else {
        None
    };
    ComposeRecordView {
        id: r.id,
        cls: r.cls,
        kind: r.kind,
        subject: r.subject,
        tag: r.tag,
        links: r.links.to_vec(),
        digest: hex(&r.digest),
        verified: r.verified == 1,
        note,
        text,
    }
}

pub(crate) struct ComposeHome {
    pub(crate) compose: Compose,
    pub(crate) machine_id: String,
    prompts: Arc<parking_lot::Mutex<HashMap<u64, String>>>,
    /// What the model Skill returned per task: the text, or why it gave
    /// none, and every attempt it made.
    proposals: Arc<parking_lot::Mutex<HashMap<u64, SkillOutput>>>,
    /// The Cortex record mark (ACCEPTANCE-v3 2.1): where it lives, the raw
    /// machine id it binds, and the last mark written (or verified).
    mark_path: PathBuf,
    machine_raw: [u8; 32],
    mark: Option<Mark>,
    /// seq of the last mark this home knows of (0 = none yet).
    mark_seq: u64,
}

impl ComposeHome {
    /// Write a new record mark when the journal holds more records than the
    /// last mark (ACCEPTANCE-v3 2.1). Called only after appends returned,
    /// so the mark never runs ahead of the journal. Returns the mark written.
    pub(crate) fn advance_mark(&mut self) -> Result<Option<Mark>, String> {
        let n = self
            .compose
            .info()
            .map_err(|e| format!("record mark: info: {e}"))?
            .records;
        if self.mark.as_ref().is_some_and(|m| m.records >= n) {
            return Ok(None);
        }
        let digest = if n == 0 {
            [0u8; 32]
        } else {
            self.compose
                .record(n)
                .map_err(|e| format!("record mark: record {n}: {e}"))?
                .digest
        };
        let m = Mark {
            machine_id: self.machine_raw,
            seq: self.mark_seq + 1,
            records: n,
            digest,
        };
        cortex_mark::write(&self.mark_path, &m)?;
        self.mark_seq = m.seq;
        self.mark = Some(m.clone());
        Ok(Some(m))
    }
}

type SkillOutput = (Result<String, String>, Vec<ProposalAttempt>);

/// Owns the composition home of this process.
pub struct ComposeBridge {
    dir: PathBuf,
    proposer: ComposeProposer,
    proposer_label: String,
    home: std::sync::Mutex<Option<ComposeHome>>,
    /// Set when the start-up reconcile failed or was refused (ACCEPTANCE-v3
    /// 2.5): effect commands refuse until an operator reconcile succeeds.
    reconcile_failed: std::sync::Mutex<Option<String>>,
}

impl ComposeBridge {
    pub fn new(dir: PathBuf, proposer: ComposeProposer, proposer_label: &str) -> Self {
        Self {
            dir,
            proposer,
            proposer_label: proposer_label.to_string(),
            home: std::sync::Mutex::new(None),
            reconcile_failed: std::sync::Mutex::new(None),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn open_home(&self) -> Result<ComposeHome, String> {
        self.open_home_marked(None)
    }

    /// Open the home and check its Cortex record mark (ACCEPTANCE-v3 2.2).
    /// `rebuilt_from` = the seq of a mark RecoverComposeHome just set aside:
    /// the home then opens without a mark and gets a fresh one.
    fn open_home_marked(&self, rebuilt_from: Option<u64>) -> Result<ComposeHome, String> {
        let root = machine_root(&self.dir)?;
        let mark_path = cortex_mark::mark_path(&self.dir);
        let existing = match rebuilt_from {
            Some(_) => None,
            None => cortex_mark::read(&mark_path)
                .map_err(|why| mark_refusal(&self.dir, &MarkRefusal::Damaged(why)))?,
        };
        let journal_present = std::fs::metadata(self.dir.join("cortex.cx"))
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        // rxc_host_open appends nothing: `info.records` is the read-only probe
        // count, so the count check runs before the open appends anything.
        let (mut compose, info) =
            Compose::open(&self.dir, RootKind::Provisioned, &root, 0xA1E4_0001)
                .map_err(|e: ComposeError| refusal(&self.dir, &e))?;
        let plan = cortex_mark::plan(
            existing.as_ref(),
            &info.machine_id,
            info.records,
            journal_present,
        )
        .map_err(|r| mark_refusal(&self.dir, &r))?;
        let prompts: Arc<parking_lot::Mutex<HashMap<u64, String>>> = Arc::default();
        let proposals: Arc<parking_lot::Mutex<HashMap<u64, SkillOutput>>> = Arc::default();
        let (pr, pp, proposer) = (prompts.clone(), proposals.clone(), self.proposer.clone());
        compose
            .register_skill(COMPOSE_MODEL_SKILL, None, 10, move |task| {
                let prompt = pr.lock().get(&task).cloned()?;
                // Fixed template + bounded automatic retry (ACCEPTANCE-v2 3a, 3b),
                // measured per-attempt budget (ACCEPTANCE-v3 3b).
                let (out, attempts) = propose_with_retries(
                    proposer.as_ref(),
                    &prompt,
                    COMPOSE_SKILL_BUDGET,
                    COMPOSE_ATTEMPT_BUDGET,
                    COMPOSE_MAX_ATTEMPTS,
                );
                let h = out.as_ref().ok().map(|t| proposal_handle(t));
                pp.lock().insert(task, (out, attempts));
                h
            })
            .map_err(|e| format!("compose register skill: {e}"))?;
        let pv = proposals.clone();
        compose
            .set_verify(move |task, result| {
                // AEGIS contract: the result names exactly the proposal text
                // the Skill recorded for this task, and that text parses as one
                // file change (a relative path and nonempty content).
                pv.lock().get(&task).is_some_and(|(t, _)| {
                    let Ok(t) = t else { return false };
                    proposal_handle(t) == result && parse_file_proposal(t).is_some()
                })
            })
            .map_err(|e| format!("compose set verify: {e}"))?;
        // Open the composition now, so a home behind its anchor is refused here
        // with its name (E_REPLAY), not inside the first run.
        compose.info().map_err(|e| refusal(&self.dir, &e))?;
        // The digest check needs the open home (the open appended the five
        // start-up records; ACCEPTANCE-v3 G1, G3).
        if let Plan::Verify(m) = &plan {
            if m.records > 0 {
                let at = compose.record(m.records).ok().map(|r| r.digest);
                cortex_mark::check_digest(m, at).map_err(|r| mark_refusal(&self.dir, &r))?;
            }
        }
        let mut home = ComposeHome {
            compose,
            machine_id: hex(&info.machine_id),
            prompts,
            proposals,
            mark_path,
            machine_raw: info.machine_id,
            mark: existing.clone(),
            mark_seq: existing
                .as_ref()
                .map_or(rebuilt_from.unwrap_or(0), |m| m.seq),
        };
        let written = home.advance_mark().map_err(|e| {
            format!(
                "compose home {} refused: record mark update failed (E_MARK_WRITE): {e}",
                self.dir.display()
            )
        })?;
        let event = match rebuilt_from {
            Some(_) => written.as_ref().map(|w| {
                format!(
                    "Cortex mark: mark rebuilt by RecoverComposeHome (seq {}, records {})",
                    w.seq, w.records
                )
            }),
            None => cortex_mark::open_event(&plan, info.records, written.as_ref()),
        };
        if let Some(line) = event {
            println!("{line}");
        }
        Ok(home)
    }

    /// Start-up reconcile failed or was refused: effect commands refuse
    /// until an operator reconcile succeeds (ACCEPTANCE-v3 2.5).
    pub fn set_reconcile_failed(&self, why: String) {
        if let Ok(mut g) = self.reconcile_failed.lock() {
            *g = Some(why);
        }
    }

    /// Why effect commands are refused, if they are.
    pub fn reconcile_failed(&self) -> Option<String> {
        match self.reconcile_failed.lock() {
            Ok(g) => g.clone(),
            Err(_) => Some("reconcile state lock poisoned".into()),
        }
    }

    pub(crate) fn clear_reconcile_failed(&self) {
        if let Ok(mut g) = self.reconcile_failed.lock() {
            *g = None;
        }
    }

    /// Blocking: runs inference inside the Skill. Call from a blocking thread.
    pub fn run_task(&self, goal: &str, workspace: &str) -> ControlResponse {
        match self.run_task_inner(goal, workspace) {
            Ok(r) => ControlResponse::ComposeTaskResult(Box::new(r)),
            Err(e) => ControlResponse::Error(e),
        }
    }

    fn run_task_inner(&self, goal: &str, workspace: &str) -> Result<ComposeTaskReport, String> {
        if goal.trim().is_empty() {
            return Err("RunComposeTask: empty goal".into());
        }
        let ws = std::fs::canonicalize(workspace)
            .map_err(|e| format!("RunComposeTask: workspace {workspace}: {e}"))?;
        if !ws.is_dir() {
            return Err(format!(
                "RunComposeTask: workspace {} is not a directory",
                ws.display()
            ));
        }
        let prompt = proposal_prompt(goal, &ws.display().to_string());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("clock: {e}"))?;
        let mut h = Sha256::new();
        h.update(goal.as_bytes());
        h.update([0u8]);
        h.update(ws.as_os_str().as_encoded_bytes());
        h.update(now.as_nanos().to_le_bytes());
        let d = h.finalize();
        let task = u64::from_le_bytes(d[..8].try_into().expect("8 bytes")) | 1;

        let mut guard = self
            .home
            .lock()
            .map_err(|_| "compose home lock poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(self.open_home()?);
        }
        let home = guard.as_mut().expect("opened above");
        home.prompts.lock().insert(task, prompt);
        let run = home.compose.run(task, now.as_micros() as u64);
        home.prompts.lock().remove(&task);
        let output = home.proposals.lock().remove(&task);
        // A run appends whether or not it commits: move the record mark first.
        home.advance_mark()
            .map_err(|e| format!("compose run: record mark update failed (E_MARK_WRITE): {e}"))?;
        let r = run.map_err(|e| format!("compose run: {e}"))?;
        let committed = r.committed == 1;
        let (out, mut proposal_attempts) =
            output.unwrap_or((Err("the Skill did not run".into()), Vec::new()));
        // The attempt handed to AEGIS is the parsed one; record its verdict.
        if let Some(a) = proposal_attempts.iter_mut().find(|a| a.outcome == "parsed") {
            a.aegis = Some(if committed { "pass" } else { "fail" }.into());
        }
        let (proposal, uncommitted_proposal, proposer_error) = match out {
            Ok(t) if committed => (Some(t), None, None),
            Ok(t) => (None, Some(t), None),
            Err(e) => (None, None, Some(e)),
        };
        let parsed = proposal.as_deref().and_then(parse_file_proposal);
        Ok(ComposeTaskReport {
            compose_dir: self.dir.display().to_string(),
            machine_id: home.machine_id.clone(),
            task,
            outcome: r.outcome,
            committed,
            branch_count: r.n_branches,
            branches_reclaimed: r.branches_reclaimed,
            winner: (r.winner != aien_omega_compose::NONE).then_some(r.winner),
            aegis_pass_mask: r.aegis_pass_mask,
            cx_goal: r.cx_goal,
            cx_candidates: r.cx_candidate.iter().copied().filter(|&x| x != 0).collect(),
            cx_evidence: r.cx_evidence,
            cx_promotion: r.cx_promotion,
            cx_admissions: r.cx_admission.iter().copied().filter(|&x| x != 0).collect(),
            winner_digest: hex(&r.winner_digest),
            record_digest: hex(&r.record_digest),
            proposal_path: parsed.as_ref().map(|p| p.path.clone()),
            proposal_content_sha256: parsed
                .as_ref()
                .map(|p| hex(&Sha256::digest(p.content.as_bytes()))),
            proposal_sha256: proposal
                .as_ref()
                .map(|t| hex(&Sha256::digest(t.as_bytes()))),
            proposal,
            uncommitted_proposal,
            proposer_error,
            proposal_attempts,
            proposer: self.proposer_label.clone(),
        })
    }

    pub(crate) fn with_home<T>(
        &self,
        f: impl FnOnce(&mut ComposeHome) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self
            .home
            .lock()
            .map_err(|_| "compose home lock poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(self.open_home()?);
        }
        let home = guard.as_mut().expect("opened above");
        let r = f(home);
        // Ok or not, the closure may have appended: the mark follows.
        if let Err(e) = home.advance_mark() {
            return Err(format!(
                "record mark update failed after the operation (E_MARK_WRITE): {e}"
            ));
        }
        r
    }

    /// S1 / S4 / S5: one operator record through the composition's writer.
    pub fn note(&self, kind: &str, text: &str, links: &[u64]) -> ControlResponse {
        let k = match kind {
            "constraint" => NoteKind::Constraint,
            "authorization" => NoteKind::Authorization,
            "effect" => NoteKind::Effect,
            other => {
                return ControlResponse::Error(format!(
                    "ComposeNote: kind {other:?} (constraint, authorization, effect)"
                ))
            }
        };
        if links.len() > 4 {
            return ControlResponse::Error("ComposeNote: at most 4 links".into());
        }
        let mut l = [0u64; 4];
        l[..links.len()].copy_from_slice(links);
        let r = self.with_home(|home| {
            let id = home
                .compose
                .note(k, l, text.as_bytes())
                .map_err(|e| format!("ComposeNote: {e}"))?;
            let rec = home
                .compose
                .record(id)
                .map_err(|e| format!("ComposeNote: re-read {id}: {e}"))?;
            Ok(ComposeNoteReport {
                machine_id: home.machine_id.clone(),
                id,
                kind: k.name().to_string(),
                digest: hex(&rec.digest),
                text_sha256: hex(&Sha256::digest(text.as_bytes())),
                links: links.to_vec(),
            })
        });
        match r {
            Ok(r) => ControlResponse::ComposeNoted(r),
            Err(e) => ControlResponse::Error(e),
        }
    }

    /// S6 / S8: host records plus the cited ids, digests re-checked.
    pub fn recall(&self, ids: &[u64], prefix: Option<u64>) -> ControlResponse {
        let r = self.with_home(|home| {
            let info = home
                .compose
                .info()
                .map_err(|e| format!("ComposeRecall: {e}"))?;
            let (host_recs, _) = home
                .compose
                .recall(aien_omega_compose::SUBJECT_HOST, 4096)
                .map_err(|e| format!("ComposeRecall: host records: {e}"))?;
            let host = host_recs
                .iter()
                .map(|r| record_view(&mut home.compose, r))
                .collect();
            let (mut cited, mut missing) = (Vec::new(), Vec::new());
            for &id in ids {
                match home.compose.record(id) {
                    Ok(r) => cited.push(record_view(&mut home.compose, &r)),
                    Err(_) => missing.push(id),
                }
            }
            let prefix_digest = match prefix {
                Some(n) if n <= info.records => {
                    let mut h = Sha256::new();
                    for id in 1..=n {
                        let r = home
                            .compose
                            .record(id)
                            .map_err(|e| format!("ComposeRecall: record {id}: {e}"))?;
                        h.update(r.digest);
                    }
                    Some(hex(&h.finalize()))
                }
                _ => None,
            };
            Ok(ComposeRecallReport {
                compose_dir: self.dir.display().to_string(),
                machine_id: home.machine_id.clone(),
                records_total: info.records,
                host,
                cited,
                missing,
                prefix,
                prefix_digest,
            })
        });
        match r {
            Ok(r) => ControlResponse::ComposeRecalled(Box::new(r)),
            Err(e) => ControlResponse::Error(e),
        }
    }

    /// Operator repair: close this process's handle, run rxc_host_recover,
    /// then check the Cortex record mark (ACCEPTANCE-v3 2.3): a mark that is
    /// damaged, ahead of the records kept, or names a different record is
    /// kept as `<mark>.lost-<seq>` (never deleted), the home is reopened with
    /// a fresh mark and one host `constraint` record names the repair.
    pub fn recover(&self) -> ControlResponse {
        let mut guard = match self.home.lock() {
            Ok(g) => g,
            Err(_) => return ControlResponse::Error("compose home lock poisoned".into()),
        };
        *guard = None;
        let root = match machine_root(&self.dir) {
            Ok(r) => r,
            Err(e) => return ControlResponse::Error(format!("RecoverComposeHome: {e}")),
        };
        let mark_path = cortex_mark::mark_path(&self.dir);
        let old_mark = cortex_mark::read(&mark_path);
        let r = match Compose::recover(&self.dir, RootKind::Provisioned, &root) {
            Ok(r) => r,
            Err(e) => {
                return ControlResponse::Error(format!(
                    "RecoverComposeHome {}: {e}",
                    self.dir.display()
                ))
            }
        };
        let mut report = ComposeRecoverReport {
            compose_dir: self.dir.display().to_string(),
            repaired: r.repaired == 1,
            tail_torn: r.tail_torn == 1,
            cause: r.cause,
            cut_lo: r.cut_lo,
            cut_hi: r.cut_hi,
            records_kept: r.records_kept,
            dropped_records: r.dropped_records,
            anchor_records: r.anchor_records,
            repair_record: r.event_id,
            cut_bytes_kept: r.cut_bytes_kept,
            cut_sha256: hex(&r.cut_sha256),
            opens: r.opens == 1,
            open_rc: r.open_rc,
            rolled_back: r.rolled_back,
            recovered_completed: r.recovered_completed,
            mark_lost: 0,
            mark_kept_as: None,
            mark_repair_record: 0,
        };
        // (old mark if readable, records lost, why) when the mark must go.
        let set_aside: Option<(Option<Mark>, u64, &str)> = match old_mark {
            Err(_) => Some((None, 0, "damaged")),
            Ok(None) => None,
            Ok(Some(m)) if m.records > r.records_kept => {
                let lost = m.records - r.records_kept;
                Some((Some(m), lost, "ahead of the journal"))
            }
            Ok(Some(m)) => match self.open_home() {
                Ok(home) => {
                    *guard = Some(home);
                    None
                }
                Err(e) if e.contains("E_MARK_DIGEST") => {
                    Some((Some(m), 0, "names a different record"))
                }
                // Refused for another reason: reported as before, mark kept.
                Err(_) => None,
            },
        };
        let Some((old, lost, why)) = set_aside else {
            return ControlResponse::ComposeRecovered(Box::new(report));
        };
        let old_seq = old.as_ref().map(|m| m.seq);
        let kept_as = cortex_mark::lost_path(&mark_path, old_seq);
        if let Err(e) = std::fs::rename(&mark_path, &kept_as) {
            return ControlResponse::Error(format!(
                "RecoverComposeHome: keep the record mark as {}: {e}",
                kept_as.display()
            ));
        }
        let mut home = match self.open_home_marked(Some(old_seq.unwrap_or(0))) {
            Ok(h) => h,
            Err(e) => {
                return ControlResponse::Error(format!(
                    "RecoverComposeHome: record mark kept as {}; the home still refuses: {e}",
                    kept_as.display()
                ))
            }
        };
        let text = serde_json::json!({
            "repair": "cortex-mark",
            "why": why,
            "mark_records": old.as_ref().map(|m| m.records),
            "journal_records": r.records_kept,
            "lost": lost,
            "old_seq": old_seq,
            "old_digest": old.as_ref().map(|m| hex(&m.digest)),
            "kept_as": kept_as.display().to_string(),
        })
        .to_string();
        let id = match home
            .compose
            .note(NoteKind::Constraint, [0; 4], text.as_bytes())
            .and_then(|id| home.compose.record(id).map(|_| id))
        {
            Ok(id) => id,
            Err(e) => {
                return ControlResponse::Error(format!(
                    "RecoverComposeHome: record mark kept as {}; repair record failed: {e}",
                    kept_as.display()
                ))
            }
        };
        if let Err(e) = home.advance_mark() {
            return ControlResponse::Error(format!(
                "RecoverComposeHome: repair record #{id} written; record mark update failed (E_MARK_WRITE): {e}"
            ));
        }
        report.opens = true;
        report.mark_lost = lost;
        report.mark_kept_as = Some(kept_as.display().to_string());
        report.mark_repair_record = id;
        *guard = Some(home);
        ControlResponse::ComposeRecovered(Box::new(report))
    }

    /// Recall: the Cortex record by id, digest re-checked (cut 2 uses this
    /// to prove cited ids survive a restart).
    pub fn record_digest(&self, id: u64) -> Result<String, String> {
        let mut guard = self
            .home
            .lock()
            .map_err(|_| "compose home lock poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(self.open_home()?);
        }
        let home = guard.as_mut().expect("opened above");
        home.compose
            .record(id)
            .map(|r| hex(&r.digest))
            .map_err(|e| format!("compose record {id}: {e}"))
    }
}
