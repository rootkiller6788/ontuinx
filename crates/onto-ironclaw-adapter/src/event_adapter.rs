//! event_adapter — bridges Onto EventSinkPort to OntoRuntime EventSink.
//!
//! Stub: works standalone. OntoRuntime integration requires constructing
//! ironclaw_events::RuntimeEvent (20+ fields) which is best done with
//! the factory methods available inside the OntoRuntime workspace.

use async_trait::async_trait;
use onto_assurance_runtime::ports::{EventSinkError, EventSinkPort};

pub struct StubEventSinkPort;

#[async_trait]
impl EventSinkPort for StubEventSinkPort {
    async fn emit(&self, _t: &str, _p: &serde_json::Value) -> Result<(), EventSinkError> { Ok(()) }
}
