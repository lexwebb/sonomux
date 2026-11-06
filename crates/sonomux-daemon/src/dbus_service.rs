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
    /// Asks the main loop for a routing pass with the latest config. Handlers never route
    /// themselves, so passes never overlap or run on a stale config.
    pub routing_kick: Arc<tokio::sync::Notify>,
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

                let (raw, pct, muted) = if curr > 0 {
                    (0, 0.0, true)
                } else {
                    (255, 100.0, false)
                };
                goxlr
                    .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, raw)
                    .await
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                state.lock().await.speaker_volume = pct;
                Ok(muted)
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
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Task execution error: {}", e)))?;
        self.routing_kick.notify_one();
        Ok(())
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
        cfg.save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        self.routing_kick.notify_one();
        Ok(())
    }

    /// Set target channel for an audio category (e.g. ("browser", "System")).
    async fn set_category_target(&self, category: String, target: String) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        cfg.routing
            .category_targets
            .insert(category.to_lowercase(), target);
        cfg.save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        self.routing_kick.notify_one();
        Ok(())
    }

    /// Apps for the routing list: (name, override key, category, override target or "", playing).
    async fn get_apps(&self) -> Vec<(String, String, String, String, bool)> {
        let overrides = self.config.lock().await.routing.app_overrides.clone();
        let streams = self
            .tokio_handle
            .spawn(async { sonomux_backend::Backend::list_streams().await })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
        sonomux_core::group_apps(&streams, &overrides)
            .into_iter()
            // Idle background apps that would stay on System anyway are just noise.
            .filter(|a| {
                a.playing
                    || a.category != sonomux_core::AppCategory::System
                    || overrides.contains_key(&a.id)
            })
            .map(|a| {
                let target = overrides.get(&a.id).cloned().unwrap_or_default();
                (a.name, a.id, a.category.to_string(), target, a.playing)
            })
            .collect()
    }

    /// Pin an app (by override key) to a channel, or "" to go back to its category's channel.
    async fn set_app_override(&self, id: String, target: String) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        if target.is_empty() {
            cfg.routing.app_overrides.remove(&id);
        } else {
            cfg.routing.app_overrides.insert(id, target);
        }
        cfg.save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        self.routing_kick.notify_one();
        Ok(())
    }

    /// Mute state of each channel that sits on a GoXLR fader.
    async fn get_channel_mutes(&self) -> std::collections::HashMap<String, bool> {
        let goxlr = self.goxlr.clone();
        self.tokio_handle
            .spawn(async move { goxlr.get_channel_mutes().await.unwrap_or_default() })
            .await
            .unwrap_or_default()
    }

    /// Mute or unmute a channel through its fader, keeping its volume.
    async fn set_channel_muted(&self, channel: String, muted: bool) -> zbus::fdo::Result<()> {
        let goxlr = self.goxlr.clone();
        self.tokio_handle
            .spawn(async move { goxlr.set_channel_muted(&channel, muted).await })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Selectable devices and the current defaults: (outputs, inputs, default out, default in).
    /// Empty on Linux, where PipeWire manages devices.
    async fn get_audio_devices(&self) -> (Vec<String>, Vec<String>, String, String) {
        #[cfg(target_os = "macos")]
        {
            use sonomux_backend::coreaudio::CoreAudioManager as Ca;
            let devs = Ca::devices();
            let names = |input: bool| {
                devs.iter()
                    .filter(|d| if input { d.has_input } else { d.has_output })
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>()
            };
            let name_of = |input: bool| {
                Ca::default_device(input)
                    .map(|d| d.name)
                    .unwrap_or_default()
            };
            (names(false), names(true), name_of(false), name_of(true))
        }
        #[cfg(not(target_os = "macos"))]
        {
            (Vec::new(), Vec::new(), String::new(), String::new())
        }
    }

    /// (keep the configured defaults, create devices for unpaired inputs).
    async fn get_device_settings(&self) -> (bool, bool) {
        let cfg = self.config.lock().await;
        (
            cfg.devices.default_output.is_some() || cfg.devices.default_input.is_some(),
            cfg.devices.split_unpaired_inputs,
        )
    }

    /// Make a device the default output or input. When defaults are kept, also remember it.
    async fn set_default_device(&self, name: String, input: bool) -> zbus::fdo::Result<()> {
        #[cfg(target_os = "macos")]
        sonomux_backend::coreaudio::CoreAudioManager::set_default_device(&name, input);
        let mut cfg = self.config.lock().await;
        let slot = if input {
            &mut cfg.devices.default_input
        } else {
            &mut cfg.devices.default_output
        };
        if slot.is_some() {
            *slot = Some(name);
            cfg.save()
                .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        }
        Ok(())
    }

    /// Keep the macOS defaults on the current devices (true) or stop managing them (false).
    async fn set_keep_defaults(&self, keep: bool) -> zbus::fdo::Result<()> {
        let (_, _, out, inp) = self.get_audio_devices().await;
        let mut cfg = self.config.lock().await;
        if keep {
            cfg.devices.default_output = (!out.is_empty()).then_some(out);
            cfg.devices.default_input = (!inp.is_empty()).then_some(inp);
        } else {
            cfg.devices.default_output = None;
            cfg.devices.default_input = None;
        }
        cfg.save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        Ok(())
    }

    /// Create (true) or remove (false) devices for inputs without a matching output.
    async fn set_split_unpaired(&self, enabled: bool) -> zbus::fdo::Result<()> {
        let mut cfg = self.config.lock().await;
        cfg.devices.split_unpaired_inputs = enabled;
        cfg.save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        Ok(())
    }

    /// Split devices as (hardware, device, plays to, records from), and other hardware as
    /// (name, why not split). Empty on Linux.
    async fn get_split_devices(
        &self,
    ) -> (Vec<(String, String, String, String)>, Vec<(String, String)>) {
        #[cfg(target_os = "macos")]
        {
            let unpaired = self.config.lock().await.devices.split_unpaired_inputs;
            let (split, other) = sonomux_backend::coreaudio_channels::describe(unpaired);
            let pair = |label: &Option<String>, first: Option<u32>| match (label, first) {
                (Some(l), Some(f)) => format!("{} ({}–{})", l, f, f + 1),
                _ => "—".to_string(),
            };
            let rows = split
                .into_iter()
                .flat_map(|(hw, splits)| {
                    splits.into_iter().map(move |s| {
                        (
                            hw.clone(),
                            s.name.clone(),
                            pair(&s.output_label, s.output),
                            pair(&s.input_label, s.input),
                        )
                    })
                })
                .collect();
            (rows, other)
        }
        #[cfg(not(target_os = "macos"))]
        {
            (Vec::new(), Vec::new())
        }
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
            cfg.save()
                .map_err(|e| zbus::fdo::Error::Failed(format!("Could not save config: {}", e)))?;
        }
        Ok(())
    }
}
