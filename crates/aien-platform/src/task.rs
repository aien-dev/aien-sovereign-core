use crate::PlatformError;
use alloc::string::String;
use alloc::vec::Vec;

pub struct TaskRequest<'a> {
    pub capability_token: u64,
    pub command_tag: &'a str,
    pub payload: &'a [u8],
    pub timeout_ticks: Option<u64>,
}

pub struct TaskResult {
    pub exit_code: i32,
    pub output: Vec<u8>,
    pub error_log: Option<String>,
}

pub trait TaskExecutor {
    fn execute(&self, task: TaskRequest<'_>) -> Result<TaskResult, PlatformError>;
}
