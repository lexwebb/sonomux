use sonomux_backend::Backend;
use sonomux_core::rules::{RuleContext, RuleEngine};
use sonomux_core::traits::{AudioDevice, DeviceEvent, DeviceStatus};
use sonomux_core::AppConfig;
use sonomux_daemon::dbus_service::{SharedDaemonState, SonomuxDbusService};
use sonomux_daemon::executor::ActionExecutor;
use sonomux_daemon::state::StateStore;
use sonomux_device_goxlr::GoXLRClient;
use sonomux_device_headset::HeadsetMonitor;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
#[cfg(not(target_os = "macos"))]
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
        system_volume: 100.0,
        game_volume: 85.0,
        chat_volume: 90.0,
        music_volume: 70.0,
        mic_volume: 100.0,
        loopback_active: false,
    }));

    let config_shared = Arc::new(Mutex::new(config.clone()));

    // Start D-Bus session service if enabled
    if config.daemon.dbus_enabled {
        let dbus_svc = SonomuxDbusService {
            state: Arc::clone(&shared_state),
            goxlr: goxlr.clone(),
            config: Arc::clone(&config_shared),
            tokio_handle: tokio::runtime::Handle::current(),
        };

        #[cfg(target_os = "macos")]
        tokio::spawn(serve_unix_socket(dbus_svc));

        #[cfg(not(target_os = "macos"))]
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
    let mut fader_ticker = tokio::time::interval(Duration::from_millis(50));

    #[cfg(unix)]
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();

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

            // 2. Fast GoXLR hardware fader polling (100ms) for responsive GUI sync
            _ = fader_ticker.tick() => {
                if let Ok(vols) = goxlr.get_all_volumes().await {
                    let mut s = shared_state.lock().await;
                    if let Some(&v) = vols.get("Headphones") {
                        s.speaker_volume = (v as f64 / 255.0) * 100.0;
                    }
                    if let Some(&v) = vols.get("System") {
                        s.system_volume = (v as f64 / 255.0) * 100.0;
                    }
                    if let Some(&v) = vols.get("Game") {
                        s.game_volume = (v as f64 / 255.0) * 100.0;
                    }
                    if let Some(&v) = vols.get("Chat") {
                        s.chat_volume = (v as f64 / 255.0) * 100.0;
                    }
                    if let Some(&v) = vols.get("Music") {
                        s.music_volume = (v as f64 / 255.0) * 100.0;
                    }
                    if let Some(&v) = vols.get("Mic") {
                        s.mic_volume = (v as f64 / 255.0) * 100.0;
                    }
                }
            }

            // 3. Periodic topology & routing maintenance tick
            _ = ticker.tick() => {
                let current_cfg = config_shared.lock().await.clone();

                // macOS shows multichannel hardware (e.g. the GoXLR) as one device; keep a
                // device per named stereo pair so apps and routing can target each channel.
                #[cfg(target_os = "macos")]
                sonomux_backend::coreaudio_channels::ensure_split_devices(
                    current_cfg.devices.split_unpaired_inputs,
                );
                // Keep the default devices where the config says (what FineTune's lock did).
                #[cfg(target_os = "macos")]
                for (key, input) in [
                    (&current_cfg.devices.default_output, false),
                    (&current_cfg.devices.default_input, true),
                ] {
                    if let Some(k) = key {
                        if sonomux_backend::coreaudio::CoreAudioManager::set_default_device(k, input) {
                            info!("Default {} set to '{}'", if input { "input" } else { "output" }, k);
                        }
                    }
                }

                let goxlr_up = goxlr.status().await == DeviceStatus::Active
                    && Backend::source_exists(&current_cfg.devices.goxlr_mix_source).await;

                {
                    let mut s = shared_state.lock().await;
                    s.goxlr_active = goxlr_up;
                }

                if goxlr_up {
                    if !goxlr_was_up {
                        // The sampler mix only feeds the headset loopback, which macOS lacks so far.
                        #[cfg(not(target_os = "macos"))]
                        {
                            info!("GoXLR detected active! Applying sampler matrix routing...");
                            if let Err(e) = goxlr.setup_sampler_routing().await {
                                tracing::error!("Failed to configure GoXLR routing: {}", e);
                            }
                        }
                        goxlr_was_up = true;
                    }

                    // Enforce Virtual Surround Manager pinning
                    let keep_off: Vec<&str> = current_cfg.policies.keep_off_surround.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = Backend::enforce_surround_policies(
                        &current_cfg.policies.surround_out_node,
                        &current_cfg.policies.surround_in_node,
                        &current_cfg.devices.goxlr_game_sink,
                        &current_cfg.devices.goxlr_system_sink,
                        &keep_off,
                    ).await {
                        debug!("Surround policy enforcement: {}", e);
                    }

                    // Enforce intelligent stream classification & routing policies
                    if routing_should_run(goxlr_up, current_cfg.routing.enabled) {
                        if let Err(e) = Backend::enforce_stream_routing(
                            &current_cfg.routing,
                            &current_cfg.devices,
                            &current_cfg.policies,
                        ).await {
                            debug!("Stream routing enforcement error: {}", e);
                        }
                    } else {
                        Backend::stop_stream_routing().await;
                    }

                    // Evaluate rules on maintenance tick
                    evaluate_and_dispatch(
                        &headset,
                        &goxlr,
                        &current_cfg,
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
                    Backend::stop_stream_routing().await;
                }
            }

            // 3. Graceful shutdown on Ctrl+C or SIGTERM
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal (Ctrl+C) received. Cleaning up Sonomux daemon...");
                if let Some(id) = current_loopback_id {
                    let _ = Backend::unload_module(id).await;
                }
                break;
            }

            _ = async {
                #[cfg(unix)]
                {
                    if let Some(ref mut term) = sigterm {
                        term.recv().await;
                        return;
                    }
                }
                std::future::pending::<()>().await
            } => {
                info!("Shutdown signal (SIGTERM) received. Cleaning up Sonomux daemon...");
                if let Some(id) = current_loopback_id {
                    let _ = Backend::unload_module(id).await;
                }
                break;
            }
        }
    }

    info!("Sonomux daemon exited cleanly.");
    Ok(())
}

/// Per-app routing runs only while it is enabled and the GoXLR is up; otherwise every route
/// must be released, or tapped apps stay muted on a channel nobody hears.
fn routing_should_run(goxlr_up: bool, routing_enabled: bool) -> bool {
    goxlr_up && routing_enabled
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

    let headset_sink_present = Backend::sink_exists(&config.devices.headset_sink).await;
    let goxlr_source_present = Backend::source_exists(&config.devices.goxlr_mix_source).await;

    let mut ctx = RuleContext::new()
        .with_device_status("headset", headset_status)
        .with_device_status("goxlr", goxlr_status)
        .with_device_battery("headset", battery)
        .with_node(&config.devices.headset_sink, headset_sink_present)
        .with_node(&config.devices.goxlr_mix_source, goxlr_source_present);

    if let Ok(streams) = Backend::get_active_streams().await {
        for s in streams {
            let cat = sonomux_core::AppClassifier::classify(&s, &config.routing.app_overrides);
            ctx = ctx.with_stream_category(&cat.to_string(), true);
        }
    }

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

/// macOS has no session bus: serve the D-Bus interface peer-to-peer, one connection per client.
#[cfg(target_os = "macos")]
async fn serve_unix_socket(svc: SonomuxDbusService) {
    let path = sonomux_core::ipc::socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A stale socket from a previous run blocks bind.
    let _ = std::fs::remove_file(&path);
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            warn!("Could not bind IPC socket {}: {}", path.display(), e);
            return;
        }
    };
    info!("IPC service listening on {}", path.display());

    loop {
        let stream = match listener.accept().await.and_then(|(s, _)| s.into_std()) {
            Ok(s) => s,
            Err(e) => {
                warn!("IPC accept error: {}", e);
                continue;
            }
        };
        let svc = svc.clone();
        tokio::spawn(async move {
            let conn = zbus::connection::Builder::async_io_unix_stream(stream)
                .server(zbus::Guid::generate())
                .map(|b| b.p2p())
                .and_then(|b| b.serve_at(sonomux_core::ipc::OBJECT_PATH, svc));
            match conn {
                Ok(b) => match b.build().await {
                    // zbus drives the connection; hold it until the client hangs up.
                    // ponytail: 1s poll, swap for a close event if zbus grows one.
                    Ok(conn) => {
                        while !conn.is_closed() {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                    Err(e) => debug!("IPC client handshake failed: {}", e),
                },
                Err(e) => warn!("IPC connection setup error: {}", e),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::routing_should_run;

    #[test]
    fn routing_runs_only_when_enabled_and_goxlr_up() {
        assert!(routing_should_run(true, true));
        assert!(!routing_should_run(true, false));
        assert!(!routing_should_run(false, true));
        assert!(!routing_should_run(false, false));
    }
}
