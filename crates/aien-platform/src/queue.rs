pub type SequenceId = u64;
pub type Ticks = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Background = 0,
    Normal = 1,
    Interactive = 2,
    Realtime = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelHandle(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KvHandle(pub u64);

/// The fundamental scheduling unit of SparkOS: tokens are the workload, not processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceWork {
    pub sequence: SequenceId,
    pub model: ModelHandle,
    pub kv: KvHandle,
    pub priority: Priority,
    pub deadline: Option<Ticks>,
    pub branch_parent: Option<SequenceId>,
    pub next_token_budget: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    Full,
    Empty,
    InvalidSequence(SequenceId),
}

impl core::fmt::Display for QueueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Full => write!(f, "WorkQueue error: queue capacity exceeded"),
            Self::Empty => write!(f, "WorkQueue error: queue empty"),
            Self::InvalidSequence(id) => write!(f, "WorkQueue error: invalid sequence id {}", id),
        }
    }
}

pub trait WorkQueue {
    fn push(&self, work: InferenceWork) -> Result<(), QueueError>;
    fn pop(&self) -> Option<InferenceWork>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
