use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("Device not found: {0}")]
    NotFound(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Channel '{0}' is not supported")]
    UnsupportedChannel(String),
    #[error("Internal device error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    Disconnected,
    /// Physical interface/dongle connected, but peripheral is inactive/off.
    Standby,
    /// Peripheral active and responding.
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceEvent {
    StatusChanged {
        id: String,
        status: DeviceStatus,
    },
    VolumeChanged {
        id: String,
        channel: String,
        level: f32,
    },
    BatteryChanged {
        id: String,
        percentage: u8,
    },
    Custom {
        id: String,
        name: String,
        payload: String,
    },
}

#[async_trait]
pub trait AudioDevice: Send + Sync {
    fn metadata(&self) -> &DeviceMetadata;
    async fn status(&self) -> DeviceStatus;
    async fn subscribe_events(&self) -> broadcast::Receiver<DeviceEvent>;
}

/// Solves ghost dongle issue for wireless peripherals by querying physical state/battery.
#[async_trait]
pub trait HasPresenceDetection: AudioDevice {
    async fn is_physically_active(&self) -> bool;
    async fn battery_level(&self) -> Option<u8>;
    async fn is_charging(&self) -> Option<bool>;
}

/// For multi-channel devices with hardware cross-point matrices.
#[async_trait]
pub trait HasRoutingMatrix: AudioDevice {
    async fn available_inputs(&self) -> Vec<String>;
    async fn available_outputs(&self) -> Vec<String>;
    async fn is_routed(&self, input: &str, output: &str) -> Result<bool, DeviceError>;
    async fn set_route(
        &mut self,
        input: &str,
        output: &str,
        enabled: bool,
    ) -> Result<(), DeviceError>;
}

/// For devices with dedicated hardware DAC / mixer volumes.
#[async_trait]
pub trait HasHardwareVolume: AudioDevice {
    async fn get_channel_volume(&self, channel: &str) -> Result<f32, DeviceError>;
    async fn set_channel_volume(&mut self, channel: &str, level: f32) -> Result<(), DeviceError>;
    async fn set_channel_mute(&mut self, channel: &str, muted: bool) -> Result<(), DeviceError>;
}
