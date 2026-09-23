//! Tool descriptors and effect classification for the AIEN capability graph.
//!
//! A tool declares what it can do. `routing_class` is the highest declared
//! effect. `speculation_safe` is true only when that class may run before a
//! winning World is chosen.

mod effects;
mod ids;
mod tool;

pub use effects::{routing_class, speculation_safe, EffectClass, ToolEffects};
pub use ids::{Digest32, EffectId, JNodeId, ProviderId, WorldId};
pub use tool::{catalog_digest, ToolDescriptor};
