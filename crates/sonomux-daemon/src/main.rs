mod dbus_service;
mod state;

use dbus_service::{SharedDaemonState, SonomuxDbusService};
use sonomux_backend::PulseManager;
use sonomux_core::traits::{AudioDevice, DeviceEvent, DeviceStatus};
use sonomux_core::AppConfig;
use sonomux_device_goxlr::{Channel, GoXLRClient};
use sonomux_device_headset::HeadsetMonitor;
use state::StateStore;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use zbus::connection::Builder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(
        "🎛️ Starting Sonomux Daemon (sonomuxd) v{}...",
        env!("CARGO_PKG_VERSION")
    );

    // Load or generate default config (~/.config/sonomux/config.toml)
    let (config, config_path) = AppConfig::load_or_create()?;
    info!("Configuration loaded from: {}", config_path.display());

    let state_store = StateStore::new();
    let goxlr = GoXLRClient::new(&config.devices.goxlr_socket);
    let headset = HeadsetMonitor::new(&config.devices.headset_name, config.devices.debounce_count);

    // Initial volume readout
    let initial_vol = goxlr.get_headphones_volume().await.unwrap_or(255);
    let shared_state = Arc::new(Mutex::new(SharedDaemonState {
        goxlr_active: false,
        headset_status: "Standby".into(),
        headset_active: false,
        battery_pct: 0,
        speaker_volume: (initial_vol as f64 / 255.0) * 100.0,
        loopback_active: false,
    }));

    // Start D-Bus session service if enabled
    if config.daemon.dbus_enabled {
        let dbus_svc = SonomuxDbusService {
            state: Arc::clone(&shared_state),
            goxlr: goxlr.clone(),
            config: config.clone(),
        };

        tokio::spawn(async move {
            match Builder::session() {
                Ok(builder) => {
                    match builder
                        .name("org.sonomux.Daemon")
                        .and_then(|b| b.serve_at("/org/sonomux/Daemon", dbus_svc))
                    {
                        Ok(named) => {
                            match named.build().await {
                                Ok(conn) => {
                                    info!("D-Bus service 'org.sonomux.Daemon' registered on session bus.");
                                    let () = std::future::pending().await;
                                    drop(conn);
                                }
                                Err(e) => warn!("Failed to start D-Bus service connection: {}", e),
                            }
                        }
                        Err(e) => warn!("D-Bus service name registration error: {}", e),
                    }
                }
                Err(e) => warn!("Could not connect to session D-Bus: {}", e),
            }
        });
    }

    // Start headset background presence watcher
    let _monitor_task = headset.start_monitor(Duration::from_millis(1000));
    let mut headset_events = headset.subscribe_events().await;

    let mut goxlr_was_up = false;
    let mut current_loopback_id: Option<u32> = None;

    info!("Sonomux daemon initialized and watching audio graph.");

    let mut ticker = tokio::time::interval(Duration::from_millis(config.daemon.check_interval_ms));

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
                            &config,
                            &state_store,
                            &shared_state,
                            &mut current_loopback_id,
                        ).await;
                    }
                    DeviceEvent::BatteryChanged { id: _, percentage } => {
                        debug!("Headset battery: {}%", percentage);
                        let mut s = shared_state.lock().await;
                        s.battery_pct = percentage;
                    }
                    _ => {}
                }
            }

            // 2. Periodic topology & routing maintenance tick
            _ = ticker.tick() => {
                let goxlr_up = goxlr.status().await == DeviceStatus::Active
                    && PulseManager::source_exists(&config.devices.goxlr_mix_source).await;

                {
                    let mut s = shared_state.lock().await;
                    s.goxlr_active = goxlr_up;
                }

                if goxlr_up {
                    if !goxlr_was_up {
                        info!("GoXLR detected active! Applying sampler matrix routing...");
                        if let Err(e) = goxlr.setup_sampler_routing().await {
                            error!("Failed to configure GoXLR routing: {}", e);
                        }
                        goxlr_was_up = true;
                    }

                    // Enforce Virtual Surround Manager pinning
                    let keep_off: Vec<&str> = config.policies.keep_off_surround.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = PulseManager::enforce_surround_policies(
                        &config.policies.surround_out_node,
                        &config.policies.surround_in_node,
                        &config.devices.goxlr_game_sink,
                        &config.devices.goxlr_system_sink,
                        &keep_off,
                    ).await {
                        debug!("Surround policy enforcement: {}", e);
                    }

                    // If headset is active, verify loopback health
                    let headset_active = headset.status().await == DeviceStatus::Active;
                    if headset_active && PulseManager::sink_exists(&config.devices.headset_sink).await {
                        match PulseManager::ensure_loopback(
                            &config.devices.goxlr_mix_source,
                            &config.devices.headset_sink,
                            config.policies.loopback_latency_ms,
                        ).await {
                            Ok(id) => {
                                current_loopback_id = Some(id);
                                let mut s = shared_state.lock().await;
                                s.loopback_active = true;
                            }
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

            // 3. Graceful shutdown on Ctrl+C or SIGTERM
            _ = async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    if let Ok(mut term) = signal(SignalKind::terminate()) {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {},
                            _ = term.recv() => {},
                        }
                        return;
                    }
                }
                let _ = tokio::signal::ctrl_c().await;
            } => {
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
    config: &AppConfig,
    state_store: &StateStore,
    shared_state: &Arc<Mutex<SharedDaemonState>>,
    current_loopback_id: &mut Option<u32>,
) {
    let mut s = shared_state.lock().await;
    match status {
        DeviceStatus::Active => {
            s.headset_status = "Active".into();
            s.headset_active = true;
            drop(s);

            info!("🎧 Headset turned ON: Enabling feed and muting desk speakers...");
            if PulseManager::sink_exists(&config.devices.headset_sink).await {
                match PulseManager::ensure_loopback(
                    &config.devices.goxlr_mix_source,
                    &config.devices.headset_sink,
                    config.policies.loopback_latency_ms,
                )
                .await
                {
                    Ok(id) => {
                        *current_loopback_id = Some(id);
                        let mut st = shared_state.lock().await;
                        st.loopback_active = true;
                    }
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
            } else {
                let mut st = shared_state.lock().await;
                st.speaker_volume = 0.0;
            }
        }
        DeviceStatus::Standby | DeviceStatus::Disconnected => {
            s.headset_status = if status == DeviceStatus::Standby {
                "Standby".into()
            } else {
                "Disconnected".into()
            };
            s.headset_active = false;
            s.loopback_active = false;
            drop(s);

            info!("🔊 Headset turned OFF: Disabling feed and restoring desk speakers...");
            if let Some(id) = current_loopback_id.take() {
                let _ = PulseManager::unload_module(id).await;
            } else if let Some(id) =
                PulseManager::find_loopback_module(&config.devices.goxlr_mix_source).await
            {
                let _ = PulseManager::unload_module(id).await;
            }

            // Restore previous speaker volume
            let restore_vol = state_store.load_speaker_volume().unwrap_or(255);
            info!("Restoring GoXLR speaker volume to {}", restore_vol);
            if let Err(e) = goxlr.set_volume_raw(Channel::Headphones, restore_vol).await {
                error!("Failed to restore GoXLR headphone volume: {}", e);
            } else {
                let mut st = shared_state.lock().await;
                st.speaker_volume = (restore_vol as f64 / 255.0) * 100.0;
            }
        }
    }
}
