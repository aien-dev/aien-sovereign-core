//! Persistent KV benchmark harness (cut C9, sc#485; ADR 0003 sections 10,
//! 15.1 and 16.5). `report` is the output contract (labels, required metrics,
//! NOT_RUN rows, no mixed cache states); `run` measures rows on the host CPU
//! reference backend through the real runtime spine.

pub mod report;
pub mod run;
