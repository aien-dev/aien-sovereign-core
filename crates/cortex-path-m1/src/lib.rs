//! Deterministic claim-path rescoring for the Cortex Milestone 1 retrieval experiment.
//!
//! Production `recall_entities` is called and not modified. This crate does not insert
//! candidates, promote claims, or persist paths.

pub mod config;
pub mod fixture;
pub mod load;
pub mod metrics;
pub mod run;
pub mod score;
