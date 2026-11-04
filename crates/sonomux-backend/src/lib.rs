//! Audio server backend abstraction for PipeWire (Linux) and CoreAudio (macOS).

#[cfg(target_os = "macos")]
pub mod coreaudio;
#[cfg(target_os = "macos")]
pub mod coreaudio_channels;
#[cfg(target_os = "macos")]
pub mod coreaudio_routing;
#[cfg(target_os = "macos")]
pub mod coreaudio_tap;
pub mod pulse;

use async_trait::async_trait;
pub use pulse::PulseManager;

/// The audio backend for the current OS.
#[cfg(target_os = "macos")]
pub use coreaudio::CoreAudioManager as Backend;
#[cfg(not(target_os = "macos"))]
pub use pulse::PulseManager as Backend;
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
