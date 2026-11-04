use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;

pub struct SharedDaemonState {
    pub goxlr_active: bool,
    pub headset_status: String,
    pub headset_active: bool,
    pub battery_pct: u8,
    pub speaker_volume: f64,
    pub loopback_active: bool,
}

pub struct SonomuxDbusService {
    pub state: Arc<Mutex<SharedDaemonState>>,
    pub goxlr: sonomux_device_goxlr::GoXLRClient,
    pub config: sonomux_core::AppConfig,
    pub tokio_handle: tokio::runtime::Handle,
}

#[interface(name = "org.sonomux.Daemon")]
impl SonomuxDbusService {
    /// Get current audio states:
    /// (goxlr_active, headset_status, headset_active, battery_pct, speaker_volume, loopback_active)
    async fn get_status(&self) -> (bool, String, bool, u8, f64, bool) {
        let s = self.state.lock().await;
        (
            s.goxlr_active,
            s.headset_status.clone(),
            s.headset_active,
            s.battery_pct,
            s.speaker_volume,
            s.loopback_active,
        )
    }

    /// Set GoXLR desk speaker volume (0.0 to 100.0).
    async fn set_speaker_volume(&self, volume_pct: f64) -> zbus::fdo::Result<()> {
        let goxlr = self.goxlr.clone();
        let state = Arc::clone(&self.state);

        self.tokio_handle
            .spawn(async move {
                let raw = ((volume_pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                goxlr
                    .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, raw)
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

                let mut s = state.lock().await;
                s.speaker_volume = volume_pct;
                Ok(())
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?
    }

    /// Toggle mute for GoXLR desk speakers. Returns new mute state (true = muted).
    async fn toggle_speaker_mute(&self) -> zbus::fdo::Result<bool> {
        let goxlr = self.goxlr.clone();
        let state = Arc::clone(&self.state);

        self.tokio_handle
            .spawn(async move {
                let curr = goxlr
                    .get_headphones_volume()
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

                let mut s = state.lock().await;
                if curr > 0 {
                    goxlr
                        .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, 0)
                        .await
                        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                    s.speaker_volume = 0.0;
                    Ok(true)
                } else {
                    goxlr
                        .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, 255)
                        .await
                        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                    s.speaker_volume = 100.0;
                    Ok(false)
                }
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?
    }

    /// Enforce Virtual Surround Manager and stream isolation rules immediately.
    async fn enforce_policies(&self) -> zbus::fdo::Result<()> {
        let config = self.config.clone();

        self.tokio_handle
            .spawn(async move {
                let keep_off: Vec<&str> = config
                    .policies
                    .keep_off_surround
                    .iter()
                    .map(|s| s.as_str())
                    .collect();

                sonomux_backend::PulseManager::enforce_surround_policies(
                    &config.policies.surround_out_node,
                    &config.policies.surround_in_node,
                    &config.devices.goxlr_game_sink,
                    &config.devices.goxlr_system_sink,
                    &keep_off,
                )
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?
    }
}
