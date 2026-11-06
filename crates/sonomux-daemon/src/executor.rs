use crate::dbus_service::SharedDaemonState;
use crate::state::StateStore;
use sonomux_backend::Backend;
use sonomux_core::rules::Action;
use sonomux_core::traits::HasHardwareVolume;
use sonomux_device_goxlr::GoXLRClient;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub struct ActionExecutor<'a> {
    pub goxlr: &'a GoXLRClient,
    pub state_store: &'a StateStore,
    pub shared_state: &'a Arc<Mutex<SharedDaemonState>>,
    pub current_loopback_id: &'a mut Option<u32>,
}

impl<'a> ActionExecutor<'a> {
    pub async fn execute_actions(&mut self, actions: &[Action]) {
        for action in actions {
            self.execute_action(action).await;
        }
    }

    pub async fn execute_action(&mut self, action: &Action) {
        match action {
            Action::EnsureLoopback {
                source,
                sink,
                latency_ms,
            } => {
                info!(
                    "Executing EnsureLoopback: source={} -> sink={} ({}ms)",
                    source, sink, latency_ms
                );
                if Backend::sink_exists(sink).await {
                    match Backend::ensure_loopback(source, sink, *latency_ms).await {
                        Ok(id) => {
                            *self.current_loopback_id = Some(id);
                            let mut st = self.shared_state.lock().await;
                            st.loopback_active = true;
                        }
                        Err(e) => error!("EnsureLoopback error: {}", e),
                    }
                } else {
                    debug!("EnsureLoopback skipped: sink {} is not active", sink);
                }
            }

            Action::DestroyLoopback { source } => {
                info!("Executing DestroyLoopback for source={}", source);
                if let Some(id) = self.current_loopback_id.take() {
                    let _ = Backend::unload_module(id).await;
                } else if let Some(id) = Backend::find_loopback_module(source).await {
                    let _ = Backend::unload_module(id).await;
                }
                let mut st = self.shared_state.lock().await;
                st.loopback_active = false;
            }

            Action::SetHardwareVolume {
                device_id,
                channel,
                level,
            } => {
                info!(
                    "Executing SetHardwareVolume: device={} channel={} level={:.2}",
                    device_id, channel, level
                );
                if device_id == "goxlr" {
                    let mut client = self.goxlr.clone();
                    if let Err(e) = client.set_channel_volume(channel, *level).await {
                        error!("Failed to set {} volume on GoXLR: {}", channel, e);
                    } else if channel == "Headphones" {
                        let mut st = self.shared_state.lock().await;
                        st.speaker_volume = (*level as f64) * 100.0;
                    }
                }
            }

            Action::SaveHardwareVolume {
                device_id,
                channel,
                slot,
            } => {
                info!(
                    "Executing SaveHardwareVolume: device={} channel={} slot={}",
                    device_id, channel, slot
                );
                if device_id == "goxlr" {
                    if let Ok(v) = HasHardwareVolume::get_channel_volume(self.goxlr, channel).await
                    {
                        let raw = (v * 255.0).round() as u8;
                        if raw > 0 {
                            self.state_store.save_volume(slot, raw);
                        } else {
                            debug!(
                                "Not saving {} volume: it is 0 (muted or unknown channel)",
                                channel
                            );
                        }
                    }
                }
            }

            Action::RestoreHardwareVolume {
                device_id,
                channel,
                slot,
                default_level,
            } => {
                info!(
                    "Executing RestoreHardwareVolume: device={} channel={} slot={}",
                    device_id, channel, slot
                );
                if device_id == "goxlr" {
                    let restore_raw = self
                        .state_store
                        .load_volume(slot)
                        .unwrap_or((default_level.clamp(0.0, 1.0) * 255.0).round() as u8);
                    info!(
                        "Restoring GoXLR {} volume to raw {} (slot {})",
                        channel, restore_raw, slot
                    );
                    let mut client = self.goxlr.clone();
                    let norm = (restore_raw as f32) / 255.0;
                    if let Err(e) = client.set_channel_volume(channel, norm).await {
                        error!("Failed to restore GoXLR {} volume: {}", channel, e);
                    } else if channel == "Headphones" {
                        let mut st = self.shared_state.lock().await;
                        st.speaker_volume = (restore_raw as f64 / 255.0) * 100.0;
                    }
                }
            }

            Action::SetHardwareMute {
                device_id,
                channel,
                muted,
            } => {
                info!(
                    "Executing SetHardwareMute: device={} channel={} muted={}",
                    device_id, channel, muted
                );
                if device_id == "goxlr" {
                    let mut client = self.goxlr.clone();
                    if let Err(e) = client.set_channel_mute(channel, *muted).await {
                        error!("Failed to set mute on GoXLR {}: {}", channel, e);
                    }
                }
            }

            Action::SetRouting {
                device_id,
                input,
                output,
                enabled,
            } => {
                info!(
                    "Executing SetRouting: device={} {} -> {} = {}",
                    device_id, input, output, enabled
                );
                if device_id == "goxlr" {
                    let mut client = self.goxlr.clone();
                    let _ = sonomux_core::traits::HasRoutingMatrix::set_route(
                        &mut client,
                        input,
                        output,
                        *enabled,
                    )
                    .await;
                }
            }

            Action::SetupSubmixMatrix { device_id } => {
                info!("Executing SetupSubmixMatrix: device={}", device_id);
                if device_id == "goxlr" {
                    if let Err(e) = self.goxlr.setup_sampler_routing().await {
                        error!("Failed to setup GoXLR sampler routing: {}", e);
                    }
                }
            }

            Action::EnforceSurround {
                surround_out,
                surround_in,
                game_sink,
                system_sink,
                keep_off_apps,
            } => {
                let keep_off: Vec<&str> = keep_off_apps.iter().map(|s| s.as_str()).collect();
                if let Err(e) = Backend::enforce_surround_policies(
                    surround_out,
                    surround_in,
                    game_sink,
                    system_sink,
                    &keep_off,
                )
                .await
                {
                    debug!("EnforceSurround error: {}", e);
                }
            }

            Action::Command { program, args } => {
                info!("Executing command: {} {:?}", program, args);
                if let Err(e) = tokio::process::Command::new(program).args(args).spawn() {
                    warn!("Failed to spawn command '{}': {}", program, e);
                }
            }

            Action::RouteStream {
                app_or_binary,
                target_sink,
            } => {
                info!(
                    "Executing RouteStream: target app/binary='{}' -> '{}'",
                    app_or_binary, target_sink
                );
                if let Ok(streams) = Backend::get_active_streams().await {
                    for s in streams {
                        if s.app_name == *app_or_binary || s.binary == *app_or_binary {
                            let _ = Backend::move_stream(s.index, target_sink).await;
                        }
                    }
                }
            }
        }
    }
}
