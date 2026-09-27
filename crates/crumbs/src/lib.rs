//! Crumbline: the Crumb v1 curriculum substrate for AIEN.
//!
//! AIEN receives observations only. Everything that explains a crumb (its
//! family, provenance, difficulty, population, decoy status, truth status and
//! held-out answers) stays on the sealed side of a process boundary.
//!
//! This crate knows nothing about any particular learner. A learner speaks the
//! protocol in `protocol`, submits candidates in the CPG1 format of `program`,
//! and receives one verdict byte per submission.

pub mod audit;
pub mod canon;
pub mod conformance;
pub mod digest;
pub mod experiment;
pub mod gates;
pub mod gen;
pub mod ledger;
pub mod mixer;
pub mod program;
pub mod promotion;
pub mod protocol;
pub mod provenance;
pub mod sealed;
pub mod session;
pub mod trace;
pub mod verify;
pub mod visible;
