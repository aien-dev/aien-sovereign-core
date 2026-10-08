//! ALLEN persona profile v1 (aien-architecture#159).
//!
//! A small, versioned, user-editable profile (display name, tone, verbosity,
//! plain-language switch, a few working preferences) bound to an engaged ALLEN
//! identity. This crate is the ONLY writer of the profile store
//! `<home>.allen-profile/`; `aien-allen` stays resolve-only and the subject
//! (kind 24) is never touched.
//!
//! ENFORCED here: the schema and its limits, the identity binding, the
//! compare-and-swap revision rule, the crash-safe write order, and the fact
//! that no profile field can carry a permission. ADVISORY: how a model
//! responds to the rendered [`PersonaContext`]; it is text in a prompt.
pub mod context;
pub mod refusal;
pub mod schema;
pub mod store;

pub use context::{PersonaContext, PersonaState, CONTEXT_MAX_BYTES};
pub use refusal::ProfileRefusal;
pub use schema::{
    Author, Changes, HistoryEntry, Identity, Persona, PrefChange, Profile, Scope, Tone, Verbosity,
    WorkingPref, DEFAULT_NAME, PROFILE_SCHEMA,
};
pub use store::{profile_dir, Store, WriteStep};
