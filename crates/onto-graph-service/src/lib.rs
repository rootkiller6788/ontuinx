//! onto-graph-service — library target (for client-side proto reuse).

pub mod merkle;
pub mod pg_store;
pub mod ring;
pub mod snapshot;
pub mod snapshot_query_service;
pub mod graph_risk_service;
pub mod query_service;
pub mod projector;

// Re-export generated proto types for client-side use
pub use snapshot_query_service::ocg_snapshot;
pub use graph_risk_service::ocg_risk;
pub use query_service::ocg_query;
