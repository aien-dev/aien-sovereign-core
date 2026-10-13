//! RED STUB (cut C4): signatures only; the implementation lands in the green commit.

use alloc::string::String;

pub const FLAG_FOREIGN: u32 = 1;
pub const FLAG_EVICTING: u32 = 2;
pub const FLAG_INDETERMINATE: u32 = 4;
pub const INDEX_ENTRY_BYTES: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct IndexEntry {
    pub key: [u8; 32],
    pub phys_bytes: u64,
    pub mtime_ns: u64,
    pub payload_len: u32,
    pub flags: u32,
}

impl IndexEntry {
    pub fn is_foreign(&self) -> bool {
        self.flags & FLAG_FOREIGN != 0
    }
    pub fn is_evicting(&self) -> bool {
        self.flags & FLAG_EVICTING != 0
    }
    pub fn is_indeterminate(&self) -> bool {
        self.flags & FLAG_INDETERMINATE != 0
    }
}

#[derive(Debug, Default)]
pub struct Index {}

#[allow(unused_variables)]
impl Index {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        unimplemented!("C4 red stub")
    }
    pub fn is_empty(&self) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn get(&self, key: &[u8; 32]) -> Option<IndexEntry> {
        unimplemented!("C4 red stub")
    }
    pub fn contains(&self, key: &[u8; 32]) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn total_bytes(&self) -> u64 {
        unimplemented!("C4 red stub")
    }
    pub fn evicting(&self) -> (u64, u64) {
        unimplemented!("C4 red stub")
    }
    pub fn evictable(&self) -> (u64, u64) {
        unimplemented!("C4 red stub")
    }
    pub fn is_busy(&self, key: &[u8; 32]) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn insert(&mut self, e: IndexEntry) -> Option<IndexEntry> {
        unimplemented!("C4 red stub")
    }
    pub fn remove(&mut self, key: &[u8; 32]) -> Option<IndexEntry> {
        unimplemented!("C4 red stub")
    }
    pub fn mark_evicting(&mut self, key: &[u8; 32]) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn clear_evicting(&mut self, key: &[u8; 32]) {
        unimplemented!("C4 red stub")
    }
    pub fn busy_inc(&mut self, key: &[u8; 32]) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn busy_dec(&mut self, key: &[u8; 32]) {
        unimplemented!("C4 red stub")
    }
    pub fn touch(&mut self, key: &[u8; 32], mtime_ns: u64) {
        unimplemented!("C4 red stub")
    }
    pub fn set_indeterminate(&mut self, key: &[u8; 32], on: bool) {
        unimplemented!("C4 red stub")
    }
    pub fn pick_victim(&self) -> Option<[u8; 32]> {
        unimplemented!("C4 red stub")
    }
    pub fn audit(&self) -> Result<(), String> {
        unimplemented!("C4 red stub")
    }
}
