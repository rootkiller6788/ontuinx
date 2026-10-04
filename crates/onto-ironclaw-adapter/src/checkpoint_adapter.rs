//! checkpoint_adapter — bridges Onto CheckpointPort to OntoRuntime RunStateStorePort.
//!
//! Stub: works standalone. OntoRuntime integration wraps `RunStateStorePort`.

use async_trait::async_trait;
use onto_assurance_runtime::ports::{CheckpointError, CheckpointPort};
use onto_assurance_types::ids::RunId;

pub struct StubCheckpointPort;

#[async_trait]
impl CheckpointPort for StubCheckpointPort {
    async fn save(&self, _id: RunId, _s: &serde_json::Value) -> Result<String, CheckpointError> { Ok("stub".into()) }
    async fn load(&self, _id: RunId) -> Result<Option<serde_json::Value>, CheckpointError> { Ok(None) }
}
