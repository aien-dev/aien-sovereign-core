// spark-aegis: Sovereign Defensive Boundary and Containment Engine
// Pure Native Rust Systems Architecture with Mojo SIMD Acceleration

pub mod audit;
pub mod containment;
pub mod doctrine;
pub mod mojo_bridge;
pub mod pr_triage;
pub mod scanner;

pub use doctrine::{
    all_invariants, AegisInvariant, ContainmentAction, ContainmentActionKind, DOCTRINE_STATEMENT,
};
