use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;

pub struct SharedDaemonState {
    pub goxlr_active: bool,
    pub headset_status: String,
    pub headset_active: bool,
    pub battery_pct: u8,
    pub speaker_volume: f64,
    pub system_volume: f64,
    pub game_volume: f64,
    pub chat_volume: f64,
    pub music_volume: f64,
    pub mic_volume: f64,
    pub loopback_active: bool,
}

#[derive(Clone)]
pub struct SonomuxDbusService {
    pub state: Arc<Mutex<SharedDaemonState>>,
    pub goxlr: sonomux_device_goxlr::GoXLRClient,
    pub config: Arc<Mutex<sonomux_core::AppConfig>>,
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

    /// Get all GoXLR channel volumes in percentages (0.0 to 100.0).
    async fn get_channel_volumes(&self) -> std::collections::HashMap<String, f64> {
        let s = self.state.lock().await;
        let mut map = std::collections::HashMap::new();
        map.insert("System".into(), s.system_volume);
        map.insert("Game".into(), s.game_volume);
        map.insert("Chat".into(), s.chat_volume);
        map.insert("Music".into(), s.music_volume);
        map.insert("Mic".into(), s.mic_volume);
        map.insert("Headphones".into(), s.speaker_volume);
        map
    }

    /// Set volume for any GoXLR channel by name (0.0 to 100.0).
    async fn set_channel_volume(&self, channel: String, volume_pct: f64) -> zbus::fdo::Result<()> {
        let goxlr = self.goxlr.clone();
        let state = Arc::clone(&self.state);

        self.tokio_handle
            .spawn(async move {
                let raw = ((volume_pct / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                let ch = match channel.as_str() {
                    "System" => sonomux_device_goxlr::Channel::System,
                    "Game" => sonomux_device_goxlr::Channel::Game,
                    "Chat" => sonomux_device_goxlr::Channel::Chat,
                    "Music" => sonomux_device_goxlr::Channel::Music,
                    "Headphones" | "Speakers" => sonomux_device_goxlr::Channel::Headphones,
                    "Mic" => sonomux_device_goxlr::Channel::Mic,
                    "LineIn" => sonomux_device_goxlr::Channel::LineIn,
                    "LineOut" => sonomux_device_goxlr::Channel::LineOut,
                    _ => {
                        return Err(zbus::fdo::Error::InvalidArgs(format!(
                            "Unknown channel: {}",
                            channel
                        )))
                    }
                };

                goxlr
                    .set_volume_raw(ch, raw)
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

                let mut s = state.lock().await;
                match channel.as_str() {
                    "System" => s.system_volume = volume_pct,
                    "Game" => s.game_volume = volume_pct,
                    "Chat" => s.chat_volume = volume_pct,
                    "Music" => s.music_volume = volume_pct,
                    "Mic" => s.mic_volume = volume_pct,
                    "Headphones" | "Speakers" => s.speaker_volume = volume_pct,
                    _ => {}
                }
                Ok(())
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?
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

    /// Enforce Virtual Surround Manager and stream classification routing rules immediately.
    async fn enforce_policies(&self) -> zbus::fdo::Result<()> {
        let config = self.config.lock().await.clone();

        self.tokio_handle
            .spawn(async move {
                let keep_off: Vec<&str> = config
                    .policies
                    .keep_off_surround
                    .iter()
                    .map(|s| s.as_str())
                    .collect();

                let _ = sonomux_backend::Backend::enforce_surround_policies(
                    &config.policies.surround_out_node,
                    &config.policies.surround_in_node,
                    &config.devices.goxlr_game_sink,
                    &config.devices.goxlr_system_sink,
                    &keep_off,
                )
                .await;

                if config.routing.enabled {
                    sonomux_backend::Backend::enforce_stream_routing(
                        &config.routing,
                        &config.devices,
                        &config.policies,
                    )
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                }
                Ok(())
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?
    }

    /// Return live detected streams: (index, app_name, binary, category, current_sink)
    async fn get_routed_streams(&self) -> Vec<(u64, String, String, String, String)> {
        let config = self.config.lock().await.clone();
        if let Ok(streams) = sonomux_backend::Backend::get_active_streams().await {
            streams
                .into_iter()
                .map(|s| {
                    let cat =
                        sonomux_core::AppClassifier::classify(&s, &config.routing.app_overrides);
                    (s.index, s.app_name, s.binary, cat.to_string(), s.sink_name)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Return routing configuration: (enabled, category_targets)
    async fn get_routing_config(&self) -> (bool, std::collections::HashMap<String, String>) {
        let cfg = self.config.lock().await;
        (cfg.routing.enabled, cfg.routing.category_targets.clone())
    }

    /// Set master toggle for automatic stream routing.
    async fn set_routing_enabled(&self, enabled: bool) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        cfg.routing.enabled = enabled;
        let _ = cfg.save();
        if enabled {
            let r = cfg.routing.clone();
            let d = cfg.devices.clone();
            let p = cfg.policies.clone();
            self.tokio_handle.spawn(async move {
                let _ = sonomux_backend::Backend::enforce_stream_routing(&r, &d, &p).await;
            });
        }
        Ok(())
    }

    /// Set target channel for an audio category (e.g. ("browser", "System")).
    async fn set_category_target(&self, category: String, target: String) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        cfg.routing
            .category_targets
            .insert(category.to_lowercase(), target);
        let _ = cfg.save();
        if cfg.routing.enabled {
            let r = cfg.routing.clone();
            let d = cfg.devices.clone();
            let p = cfg.policies.clone();
            self.tokio_handle.spawn(async move {
                let _ = sonomux_backend::Backend::enforce_stream_routing(&r, &d, &p).await;
            });
        }
        Ok(())
    }

    /// Return all automation rules with their enabled states: (name, enabled).
    async fn get_rules(&self) -> Vec<(String, bool)> {
        let cfg = self.config.lock().await;
        cfg.rules
            .iter()
            .map(|r| (r.name.clone(), r.enabled))
            .collect()
    }

    /// Set enabled state for a specific automation rule.
    async fn set_rule_enabled(&self, rule_name: String, enabled: bool) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        if let Some(r) = cfg.rules.iter_mut().find(|r| r.name == rule_name) {
            r.enabled = enabled;
            let _ = cfg.save();
        }
        Ok(())
    }
}
