mod dbus_service;
mod executor;
mod state;

use dbus_service::{SharedDaemonState, SonomuxDbusService};
use executor::ActionExecutor;
use sonomux_backend::PulseManager;
use sonomux_core::rules::{RuleContext, RuleEngine};
use sonomux_core::traits::{AudioDevice, DeviceEvent, DeviceStatus};
use sonomux_core::AppConfig;
use sonomux_device_goxlr::GoXLRClient;
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
            tokio_handle: tokio::runtime::Handle::current(),
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
    let mut rule_engine = RuleEngine::new(config.rules.clone());

    info!(
        "Rule engine loaded {} declarative rule(s).",
        rule_engine.rules().len()
    );
    info!("Sonomux daemon initialized and watching audio graph.");

    let mut ticker = tokio::time::interval(Duration::from_millis(config.daemon.check_interval_ms));

    loop {
        tokio::select! {
            // 1. React immediately to headset state events
            Ok(event) = headset_events.recv() => {
                match event {
                    DeviceEvent::StatusChanged { id: _, status } => {
                        info!("Headset status changed event: {:?}", status);
                        {
                            let mut s = shared_state.lock().await;
                            s.headset_active = status == DeviceStatus::Active;
                            s.headset_status = format!("{:?}", status);
                        }

                        // Evaluate rules against updated context
                        evaluate_and_dispatch(
                            &headset,
                            &goxlr,
                            &config,
                            &shared_state,
                            &state_store,
                            &mut rule_engine,
                            &mut current_loopback_id,
                        ).await;
                    }
                    DeviceEvent::BatteryChanged { id: _, percentage } => {
                        debug!("Headset battery: {}%", percentage);
                        {
                            let mut s = shared_state.lock().await;
                            s.battery_pct = percentage;
                        }

                        evaluate_and_dispatch(
                            &headset,
                            &goxlr,
                            &config,
                            &shared_state,
                            &state_store,
                            &mut rule_engine,
                            &mut current_loopback_id,
                        ).await;
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

                    // Evaluate rules on maintenance tick
                    evaluate_and_dispatch(
                        &headset,
                        &goxlr,
                        &config,
                        &shared_state,
                        &state_store,
                        &mut rule_engine,
                        &mut current_loopback_id,
                    ).await;
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

async fn evaluate_and_dispatch(
    headset: &HeadsetMonitor,
    goxlr: &GoXLRClient,
    config: &AppConfig,
    shared_state: &Arc<Mutex<SharedDaemonState>>,
    state_store: &StateStore,
    rule_engine: &mut RuleEngine,
    current_loopback_id: &mut Option<u32>,
) {
    let headset_status = headset.status().await;
    let goxlr_status = goxlr.status().await;
    let battery = {
        let s = shared_state.lock().await;
        s.battery_pct
    };

    let headset_sink_present = PulseManager::sink_exists(&config.devices.headset_sink).await;
    let goxlr_source_present = PulseManager::source_exists(&config.devices.goxlr_mix_source).await;

    let ctx = RuleContext::new()
        .with_device_status("headset", headset_status)
        .with_device_status("goxlr", goxlr_status)
        .with_device_battery("headset", battery)
        .with_node(&config.devices.headset_sink, headset_sink_present)
        .with_node(&config.devices.goxlr_mix_source, goxlr_source_present);

    let actions = rule_engine.evaluate(&ctx);
    if !actions.is_empty() {
        debug!(
            "Rule engine generated {} action(s) to execute",
            actions.len()
        );
        let mut executor = ActionExecutor {
            goxlr,
            state_store,
            shared_state,
            current_loopback_id,
        };
        executor.execute_actions(&actions).await;
    }
}
