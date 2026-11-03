mod state;

use sonomux_backend::PulseManager;
use sonomux_core::traits::{AudioDevice, DeviceEvent, DeviceStatus};
use sonomux_device_goxlr::{Channel, GoXLRClient, DEFAULT_GOXLR_SOCKET};
use sonomux_device_headset::HeadsetMonitor;
use state::StateStore;
use std::time::Duration;
use tracing::{debug, error, info, warn};

const HEADSET_SINK: &str = "alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo";
const GOXLR_GAME: &str = "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink";
const GOXLR_SYSTEM: &str = "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink";
const SURROUND_IN: &str = "effect_input.virtual-surround-manager";
const SURROUND_OUT: &str = "effect_output.virtual-surround-manager";
const GOXLR_MIX: &str = "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source";
const KEEP_OFF_SURROUND: &[&str] = &["Zen"];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    info!("🎛️ Starting Sonomux Daemon (sonomuxd) v{}...", env!("CARGO_PKG_VERSION"));

    let state_store = StateStore::new();
    let goxlr = GoXLRClient::new(DEFAULT_GOXLR_SOCKET);
    let headset = HeadsetMonitor::new("SteelSeries Arctis Nova 7", 2);

    // Start headset background presence watcher (polls every 1000ms with debounce)
    let _monitor_task = headset.start_monitor(Duration::from_millis(1000));
    let mut headset_events = headset.subscribe_events().await;

    let mut goxlr_was_up = false;
    let mut current_loopback_id: Option<u32> = None;

    info!("Sonomux daemon initialized and watching audio graph.");

    let mut ticker = tokio::time::interval(Duration::from_millis(1500));

    loop {
        tokio::select! {
            // 1. React immediately to headset state events
            Ok(event) = headset_events.recv() => {
                match event {
                    DeviceEvent::StatusChanged { id: _, status } => {
                        info!("Headset status changed event: {:?}", status);
                        handle_headset_transition(
                            status,
                            &goxlr,
                            &state_store,
                            &mut current_loopback_id,
                        ).await;
                    }
                    DeviceEvent::BatteryChanged { id: _, percentage } => {
                        debug!("Headset battery: {}%", percentage);
                    }
                    _ => {}
                }
            }

            // 2. Periodic topology & routing maintenance tick
            _ = ticker.tick() => {
                let goxlr_up = goxlr.status().await == DeviceStatus::Active
                    && PulseManager::source_exists(GOXLR_MIX).await;

                if goxlr_up {
                    if !goxlr_was_up {
                        info!("GoXLR detected active! Applying sampler matrix routing...");
                        if let Err(e) = goxlr.setup_sampler_routing().await {
                            error!("Failed to configure GoXLR routing: {}", e);
                        }
                        goxlr_was_up = true;
                    }

                    // Enforce Virtual Surround Manager pinning
                    if let Err(e) = PulseManager::enforce_surround_policies(
                        SURROUND_OUT,
                        SURROUND_IN,
                        GOXLR_GAME,
                        GOXLR_SYSTEM,
                        KEEP_OFF_SURROUND,
                    ).await {
                        debug!("Surround policy enforcement: {}", e);
                    }

                    // If headset is active, verify loopback health
                    let headset_active = headset.status().await == DeviceStatus::Active;
                    if headset_active && PulseManager::sink_exists(HEADSET_SINK).await {
                        match PulseManager::ensure_loopback(GOXLR_MIX, HEADSET_SINK, 20).await {
                            Ok(id) => current_loopback_id = Some(id),
                            Err(e) => warn!("Loopback check error: {}", e),
                        }
                    }
                } else {
                    if goxlr_was_up {
                        warn!("GoXLR disconnected or suspended.");
                    }
                    goxlr_was_up = false;
                }
            }

            // 3. Graceful shutdown on Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received. Cleaning up Sonomux daemon...");
                if let Some(id) = current_loopback_id {
                    let _ = PulseManager::unload_module(id).await;
                }
                break;
            }
        }
    }

    info!("Sonomux daemon exited cleanly.");
    Ok(())
}

async fn handle_headset_transition(
    status: DeviceStatus,
    goxlr: &GoXLRClient,
    state_store: &StateStore,
    current_loopback_id: &mut Option<u32>,
) {
    match status {
        DeviceStatus::Active => {
            info!("🎧 Headset turned ON: Enabling feed and muting desk speakers...");
            if PulseManager::sink_exists(HEADSET_SINK).await {
                match PulseManager::ensure_loopback(GOXLR_MIX, HEADSET_SINK, 20).await {
                    Ok(id) => *current_loopback_id = Some(id),
                    Err(e) => error!("Failed to activate headset loopback: {}", e),
                }
            }

            // Save speaker volume before zeroing
            if let Ok(vol) = goxlr.get_headphones_volume().await {
                if vol > 0 {
                    state_store.save_speaker_volume(vol);
                }
            }
            if let Err(e) = goxlr.set_volume_raw(Channel::Headphones, 0).await {
                error!("Failed to mute GoXLR headphone jack: {}", e);
            }
        }
        DeviceStatus::Standby | DeviceStatus::Disconnected => {
            info!("🔊 Headset turned OFF: Disabling feed and restoring desk speakers...");
            if let Some(id) = current_loopback_id.take() {
                let _ = PulseManager::unload_module(id).await;
            } else if let Some(id) = PulseManager::find_loopback_module(GOXLR_MIX).await {
                let _ = PulseManager::unload_module(id).await;
            }

            // Restore previous speaker volume
            let restore_vol = state_store.load_speaker_volume().unwrap_or(255);
            info!("Restoring GoXLR speaker volume to {}", restore_vol);
            if let Err(e) = goxlr.set_volume_raw(Channel::Headphones, restore_vol).await {
                error!("Failed to restore GoXLR headphone volume: {}", e);
            }
        }
    }
}
