//! Library target for spark-supervisor so integration tests and tools can drive the
//! supervisor in-process. The `spark-supervisor` binary is a thin CLI over it.

pub mod config;
pub mod proc_telemetry;
pub mod supervisor;
