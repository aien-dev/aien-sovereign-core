//! aien-closure: the `verify-closure` bridge (Verified Crumb stage 5).
//!
//! Rebuilds the internal dependency graph of a workspace and checks that every
//! internal edge is declared in a `closure.toml` manifest and backed by a
//! verified aien-proof receipt. Fails closed: any finding is a non-zero exit.

pub mod declare;
pub mod digest;
pub mod error;
pub mod graph;
pub mod manifest;
pub mod verify;

pub use error::{Code, Finding};
pub use verify::{verify, Options, Outcome};
