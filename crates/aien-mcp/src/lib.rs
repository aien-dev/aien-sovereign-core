//! MCP session owner for AIEN.
//!
//! `aien-mcp` keeps the live session. J-Space receives a [`CapabilitySnapshot`]
//! and may invoke only speculation-safe tools. An irreversible `tools/call`
//! runs only when the Effect Broker presents an [`AuthorizedEffect`].

mod broker;
mod effect;
mod error;
pub mod memory;
mod snapshot;
mod wire;

#[cfg(test)]
mod lane_tests;

#[cfg(test)]
pub(crate) use effect::authorize_for_test;

pub use broker::{EffectLane, McpBroker, SpeculativeLane, SpeculativeToolCall};
pub use effect::{AuthorizedEffect, EffectIntent, EffectReceipt, ToolResult};
pub use error::Error;
pub use snapshot::CapabilitySnapshot;
pub use wire::{CallOutcome, McpWire};
