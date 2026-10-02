//! AIEN-TEST (ADR 0028), Slices A to C: gate manifests, verdict derivation,
//! immutable evidence receipts, a runner for one gate or a whole graph of
//! gates with resource pools, GPU locks, dependency handling and `why`, and
//! the digest cache that reuses a result only for an identical experiment.
//!
//! Tests report observations; this crate derives the verdict and writes an
//! immutable, content-addressed receipt. See `manifest`, `verdict`,
//! `evidence`, `process`, `runner`, `graph`, `resources`, `why`, `cache`.

pub mod cache;
pub mod cli;
pub mod evidence;
pub mod graph;
pub mod manifest;
pub mod process;
pub mod resources;
pub mod runner;
pub mod verdict;
pub mod why;

#[cfg(test)]
pub mod testutil;
