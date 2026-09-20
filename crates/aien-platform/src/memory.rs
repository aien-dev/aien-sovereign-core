use crate::PlatformError;
use alloc::string::String;
use alloc::vec::Vec;

pub struct RecallQuery<'a> {
    pub text: &'a str,
    pub space: &'a str,
    pub limit: usize,
}

pub struct MemoryEntity {
    pub id: [u8; 16],
    pub canonical_name: String,
    pub content: String,
    pub score: f32,
}

pub struct RecallResult {
    pub entities: Vec<MemoryEntity>,
}

pub trait MemoryService {
    fn recall(&self, query: RecallQuery<'_>) -> Result<RecallResult, PlatformError>;
}
