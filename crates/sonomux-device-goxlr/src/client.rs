use crate::types::*;
use async_trait::async_trait;
use serde_json::Value;
use sonomux_core::traits::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info};

pub const DEFAULT_GOXLR_SOCKET: &str = "/tmp/goxlr.socket";

#[derive(Clone)]
pub struct GoXLRClient {
    socket_path: PathBuf,
    serial: Arc<Mutex<Option<String>>>,
    metadata: DeviceMetadata,
    event_tx: broadcast::Sender<DeviceEvent>,
}

impl Default for GoXLRClient {
    fn default() -> Self {
        Self::new(crate::DEFAULT_GOXLR_SOCKET)
    }
}

impl GoXLRClient {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        let (tx, _) = broadcast::channel(32);
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            serial: Arc::new(Mutex::new(None)),
            metadata: DeviceMetadata {
                id: "goxlr".to_string(),
                name: "TC-Helicon GoXLR".to_string(),
                vendor: "TC-Helicon".to_string(),
                capabilities: vec!["routing_matrix".to_string(), "hardware_volume".to_string()],
            },
            event_tx: tx,
        }
    }

    /// Connect and send a raw JSON request with 4-byte Big Endian length framing.
    async fn request(&self, req_json: &Value) -> Result<String, DeviceError> {
        let payload = serde_json::to_vec(req_json)
            .map_err(|e| DeviceError::Internal(format!("Failed to serialize request: {}", e)))?;

        let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            DeviceError::Communication(format!(
                "Could not connect to {}: {}",
                self.socket_path.display(),
                e
            ))
        })?;

        let len_bytes = (payload.len() as u32).to_be_bytes();
        stream
            .write_all(&len_bytes)
            .await
            .map_err(|e| DeviceError::Communication(format!("Failed to write length: {}", e)))?;
        stream
            .write_all(&payload)
            .await
            .map_err(|e| DeviceError::Communication(format!("Failed to write payload: {}", e)))?;
        stream
            .flush()
            .await
            .map_err(|e| DeviceError::Communication(format!("Flush failed: {}", e)))?;

        let mut resp_len_buf = [0u8; 4];
        stream.read_exact(&mut resp_len_buf).await.map_err(|e| {
            DeviceError::Communication(format!("Failed to read response length: {}", e))
        })?;
        let resp_len = u32::from_be_bytes(resp_len_buf) as usize;

        let mut resp_buf = vec![0u8; resp_len];
        stream.read_exact(&mut resp_buf).await.map_err(|e| {
            DeviceError::Communication(format!("Failed to read response payload: {}", e))
        })?;

        String::from_utf8(resp_buf)
            .map_err(|e| DeviceError::Internal(format!("Invalid UTF-8 in response: {}", e)))
    }

    /// Query the daemon for the full mixer status and cache the device serial.
    pub async fn get_status(&self) -> Result<DaemonStatus, DeviceError> {
        let req = serde_json::json!("GetStatus");
        let resp_str = self.request(&req).await?;

        let val: Value = serde_json::from_str(&resp_str)
            .map_err(|e| DeviceError::Internal(format!("Failed to parse status JSON: {}", e)))?;

        if let Some(err) = val.get("Error") {
            return Err(DeviceError::Internal(err.to_string()));
        }

        let status_val = val
            .get("Status")
            .ok_or_else(|| DeviceError::Internal("Missing 'Status' key in response".to_string()))?;

        let status: DaemonStatus = serde_json::from_value(status_val.clone())
            .map_err(|e| DeviceError::Internal(format!("Status deserialization error: {}", e)))?;

        // Cache the primary mixer serial
        if let Some(first_serial) = status.mixers.keys().next() {
            let mut guard = self.serial.lock().await;
            if guard.is_none() {
                *guard = Some(first_serial.clone());
                debug!("Discovered GoXLR Serial: {}", first_serial);
            }
        }

        Ok(status)
    }

    /// Get the primary mixer serial (querying status if not yet discovered).
    pub async fn get_serial(&self) -> Result<String, DeviceError> {
        {
            let guard = self.serial.lock().await;
            if let Some(s) = guard.as_ref() {
                return Ok(s.clone());
            }
        }
        let status = self.get_status().await?;
        let serial = status
            .mixers
            .keys()
            .next()
            .ok_or_else(|| {
                DeviceError::NotFound("No GoXLR devices connected to goxlr-daemon".to_string())
            })?
            .clone();
        *self.serial.lock().await = Some(serial.clone());
        Ok(serial)
    }

    /// Send a GoXLR command targeting the active device serial.
    pub async fn send_command(&self, command: Value) -> Result<(), DeviceError> {
        let serial = self.get_serial().await?;
        let req = serde_json::json!({
            "Command": [serial, command]
        });

        let resp_str = self.request(&req).await?;
        if resp_str.contains("\"Error\"") {
            return Err(DeviceError::Internal(resp_str));
        }
        Ok(())
    }

    /// Set volume for a channel (0-255 scale).
    pub async fn set_volume_raw(&self, channel: Channel, volume: u8) -> Result<(), DeviceError> {
        debug!("Setting GoXLR volume {} -> {}", channel.as_str(), volume);
        self.send_command(serde_json::json!({
            "SetVolume": [channel.as_str(), volume]
        }))
        .await
    }

    /// Set router state between input and output devices.
    pub async fn set_route(
        &self,
        input: InputDevice,
        output: OutputDevice,
        enabled: bool,
    ) -> Result<(), DeviceError> {
        debug!(
            "Setting GoXLR router: {} -> {} = {}",
            input.as_str(),
            output.as_str(),
            enabled
        );
        self.send_command(serde_json::json!({
            "SetRouter": [input.as_str(), output.as_str(), enabled]
        }))
        .await
    }

    /// Setup the standard Sampler capture routing (headset feed):
    /// Microphone -> Sampler = false
    /// Game, Chat, Music, System, Console, LineIn, Samples -> Sampler = true
    pub async fn setup_sampler_routing(&self) -> Result<(), DeviceError> {
        info!("Configuring GoXLR Sampler routing matrix for headset feed...");
        self.set_route(InputDevice::Microphone, OutputDevice::Sampler, false)
            .await?;

        let submix_inputs = [
            InputDevice::Game,
            InputDevice::Chat,
            InputDevice::Music,
            InputDevice::System,
            InputDevice::Console,
            InputDevice::LineIn,
            InputDevice::Samples,
        ];

        for input in submix_inputs {
            self.set_route(input, OutputDevice::Sampler, true).await?;
        }

        info!("GoXLR Sampler routing matrix configured.");
        Ok(())
    }

    /// Get current volume for headphones (desk speakers) in raw 0-255.
    pub async fn get_headphones_volume(&self) -> Result<u8, DeviceError> {
        let status = self.get_status().await?;
        let serial = self.get_serial().await?;
        let mixer = status
            .mixers
            .get(&serial)
            .ok_or_else(|| DeviceError::NotFound(format!("Mixer {} not in status", serial)))?;

        let vol = mixer.levels.volumes.get("Headphones").copied().unwrap_or(0);
        Ok(vol)
    }
}

#[async_trait]
impl AudioDevice for GoXLRClient {
    fn metadata(&self) -> &DeviceMetadata {
        &self.metadata
    }

    async fn status(&self) -> DeviceStatus {
        match self.get_status().await {
            Ok(_) => DeviceStatus::Active,
            Err(e) => {
                debug!("GoXLR status probe failed: {}", e);
                DeviceStatus::Disconnected
            }
        }
    }

    async fn subscribe_events(&self) -> broadcast::Receiver<DeviceEvent> {
        self.event_tx.subscribe()
    }
}

#[async_trait]
impl HasRoutingMatrix for GoXLRClient {
    async fn available_inputs(&self) -> Vec<String> {
        vec![
            "Microphone".into(),
            "Chat".into(),
            "Music".into(),
            "Game".into(),
            "Console".into(),
            "LineIn".into(),
            "System".into(),
            "Samples".into(),
        ]
    }

    async fn available_outputs(&self) -> Vec<String> {
        vec![
            "Headphones".into(),
            "BroadcastMix".into(),
            "ChatMic".into(),
            "Sampler".into(),
            "LineOut".into(),
            "StreamMix2".into(),
        ]
    }

    async fn is_routed(&self, input: &str, output: &str) -> Result<bool, DeviceError> {
        let status = self.get_status().await?;
        let serial = self.get_serial().await?;
        let mixer = status
            .mixers
            .get(&serial)
            .ok_or_else(|| DeviceError::NotFound("Mixer not found".into()))?;

        if let Some(out_map) = mixer.router.get(input) {
            if let Some(&routed) = out_map.get(output) {
                return Ok(routed);
            }
        }
        Ok(false)
    }

    async fn set_route(
        &mut self,
        input: &str,
        output: &str,
        enabled: bool,
    ) -> Result<(), DeviceError> {
        self.send_command(serde_json::json!({
            "SetRouter": [input, output, enabled]
        }))
        .await
    }
}

#[async_trait]
impl HasHardwareVolume for GoXLRClient {
    async fn get_channel_volume(&self, channel: &str) -> Result<f32, DeviceError> {
        let status = self.get_status().await?;
        let serial = self.get_serial().await?;
        let mixer = status
            .mixers
            .get(&serial)
            .ok_or_else(|| DeviceError::NotFound("Mixer not found".into()))?;

        let raw = mixer.levels.volumes.get(channel).copied().unwrap_or(0);
        Ok(raw as f32 / 255.0)
    }

    async fn set_channel_volume(&mut self, channel: &str, level: f32) -> Result<(), DeviceError> {
        let raw = (level.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.send_command(serde_json::json!({
            "SetVolume": [channel, raw]
        }))
        .await
    }

    async fn set_channel_mute(&mut self, channel: &str, muted: bool) -> Result<(), DeviceError> {
        if muted {
            self.send_command(serde_json::json!({
                "SetVolume": [channel, 0]
            }))
            .await
        } else {
            // Unmute to default 100% if no other level is specified
            self.send_command(serde_json::json!({
                "SetVolume": [channel, 255]
            }))
            .await
        }
    }
}
