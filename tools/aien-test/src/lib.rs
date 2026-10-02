//! AIEN-TEST Slice A (ADR 0028): one gate, host pool, serial.
//!
//! Tests report observations; this crate derives the verdict and writes an
//! immutable, content-addressed receipt. See `manifest`, `verdict`,
//! `evidence`, `process`, `runner`.

pub mod cli;
pub mod evidence;
pub mod manifest;
pub mod process;
pub mod runner;
pub mod verdict;

#[cfg(test)]
pub mod testutil;
