//! Audio server backend abstraction for PipeWire (Linux) and CoreAudio (macOS).

pub mod pulse;

use async_trait::async_trait;
pub use pulse::PulseManager;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("Failed to connect to audio server: {0}")]
    Connection(String),
    #[error("Node or sink not found: {0}")]
    NotFound(String),
    #[error("Routing error: {0}")]
    Routing(String),
}

#[async_trait]
pub trait AudioBackend: Send + Sync {
    async fn resolve_effective_sink(&self, sink_name: &str) -> Result<String, BackendError>;
}
