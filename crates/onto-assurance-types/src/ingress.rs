//! Run Ingress types — unified entry point for starting agent runs.
//!
//! These types decouple "who started the run" from "how the run executes".
//! Conversation becomes one of many sources, not the only one.

use serde::{Deserialize, Serialize};

/// Who or what initiated this run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActorRef {
    /// A human user, typically via Chat or CLI.
    Human(String),
    /// An autonomous agent or service principal.
    Agent(String),
    /// An internal service (e.g. OntoLoop, OntoFlow, cron).
    Service(String),
    /// A physical device or IoT actor.
    Device(String),
}

impl RuntimeActorRef {
    pub fn human(id: impl Into<String>) -> Self { Self::Human(id.into()) }
    pub fn agent(id: impl Into<String>) -> Self { Self::Agent(id.into()) }
    pub fn service(id: impl Into<String>) -> Self { Self::Service(id.into()) }
    pub fn device(id: impl Into<String>) -> Self { Self::Device(id.into()) }

    pub fn kind_str(&self) -> &str {
        match self {
            Self::Human(_) => "human",
            Self::Agent(_) => "agent",
            Self::Service(_) => "service",
            Self::Device(_) => "device",
        }
    }
}

impl std::fmt::Display for RuntimeActorRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Human(id) => write!(f, "human:{}", id),
            Self::Agent(id) => write!(f, "agent:{}", id),
            Self::Service(id) => write!(f, "service:{}", id),
            Self::Device(id) => write!(f, "device:{}", id),
        }
    }
}

/// Where the run request came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// Traditional chat conversation (WebUI, Slack, etc.)
    Conversation,
    /// CLI `ironclaw run --message "..."`
    Cli,
    /// HTTP API (programmatic access)
    Http,
    /// Direct programmatic invocation (ontotest, harnesses)
    Direct,
    /// OntoLoop Attempt cycle
    OntoLoop,
    /// OntoFlow workflow step
    OntoFlow,
    /// External device event
    DeviceEvent,
    /// Unknown or unspecified source
    Other(String),
}

impl SessionSource {
    pub fn is_conversation(&self) -> bool { matches!(self, Self::Conversation) }
    pub fn is_autonomous(&self) -> bool { matches!(self, Self::OntoLoop | Self::OntoFlow) }
}

/// A unified request to start a new agent run, regardless of source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunRequest {
    /// Who is starting this run.
    pub actor: RuntimeActorRef,

    /// Where the request came from.
    pub source: SessionSource,

    /// The input to the agent (prompt, task description, structured intent).
    pub input: InputEnvelope,

    /// Optional project scope. When set, the run operates within this project.
    pub project_id: Option<String>,

    /// Optional reference to a parent work item (for OntoLoop / OntoFlow).
    pub parent_work_item: Option<String>,
}

/// What the agent should work on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEnvelope {
    /// Primary objective or prompt.
    pub objective: String,

    /// Optional structured requirements.
    pub requirements: Vec<String>,

    /// Optional additional context.
    pub context: Option<String>,

    /// Optional model override.
    pub model: Option<String>,

    /// Maximum iterations for this run.
    pub max_iterations: Option<u32>,
}

impl InputEnvelope {
    pub fn new(objective: impl Into<String>) -> Self {
        Self {
            objective: objective.into(),
            requirements: vec![],
            context: None,
            model: None,
            max_iterations: None,
        }
    }

    pub fn with_requirements(mut self, reqs: Vec<String>) -> Self {
        self.requirements = reqs;
        self
    }
}

/// Result returned after a run completes (or fails to start).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRunResult {
    /// The ID of the created run. None if the run failed to start.
    pub run_id: Option<String>,

    /// The source that was used.
    pub source: SessionSource,

    /// Human-readable status.
    pub status: RunIngressStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunIngressStatus {
    /// Run was created and is now executing.
    Started,
    /// Run request was rejected (e.g. invalid input, missing authorization).
    Rejected,
    /// Run could not be created due to an internal error.
    Failed,
}

#[derive(Debug, Clone)]
pub enum RunIngressError {
    InvalidInput(String),
    Unauthorized(String),
    UnsupportedSource(SessionSource),
    Internal(String),
}

impl std::fmt::Display for RunIngressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid input: {}", m),
            Self::Unauthorized(m) => write!(f, "actor not authorized: {}", m),
            Self::UnsupportedSource(s) => write!(f, "source not supported: {:?}", s),
            Self::Internal(m) => write!(f, "internal error: {}", m),
        }
    }
}

impl std::error::Error for RunIngressError {}

impl StartRunRequest {
    /// Short label for the source, used in synthetic run IDs.
    pub fn source_label(&self) -> &str {
        match self.source {
            SessionSource::Conversation => "conv",
            SessionSource::Cli => "cli",
            SessionSource::Http => "http",
            SessionSource::Direct => "direct",
            SessionSource::OntoLoop => "loop",
            SessionSource::OntoFlow => "tmp",
            SessionSource::DeviceEvent => "dev",
            SessionSource::Other(_) => "other",
        }
    }
}
