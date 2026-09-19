pub mod adapter_engine;
pub mod emission;
pub mod geometry;
pub mod store;

pub use adapter_engine::{
    AdapterCategory, AdapterPipelineChain, AdapterPipelineReceipt, AdapterSpec, BenchmarkTelemetry,
    ConsumerModel, HardwareProfile, PrSubmissionPlan, SocraticEvaluation, SocraticInquiry,
    UpstreamEngine, build_adapter_spec, emit_adapter_pipeline_combs, evaluate_socratic_reflex,
    find_or_create_adapter, generate_pr_plan, get_catalog_adapters, list_adapter_pipeline_chains,
};
pub use emission::{emit_socratic_comb, emit_subagent_comb};
pub use geometry::{
    DEFAULT_HEX_RADIUS, DetailTier, HEX_DIRECTION_LABELS, HEX_DIRECTIONS, HexCoord,
    PlacementIntent, SQRT_3, axial_to_pixel, hex_distance, neighbors, pixel_to_axial, round_axial,
};
pub use store::{
    BoundingBox, CombStore, CreateForgeTaskInput, ForgeTask, HiveComb, HiveError, NeighborLink,
    PlaceCombInput,
};
