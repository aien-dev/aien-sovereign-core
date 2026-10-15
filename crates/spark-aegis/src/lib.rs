// spark-aegis: Sovereign Defensive Boundary and Containment Engine
// Pure Native Rust Systems Architecture

pub mod audit;
pub mod byte_scan;
pub mod containment;
pub mod doctrine;
pub mod pr_triage;
pub mod scanner;

pub use doctrine::{
    all_invariants, AegisInvariant, ContainmentAction, ContainmentActionKind, DOCTRINE_STATEMENT,
};
