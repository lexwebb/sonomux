// ENV_LOCK must be held across awaits: XDG_CONFIG_HOME is process-global.
#![allow(clippy::await_holding_lock)]

mod common;

use common::{mock_goxlr, shared_state, temp_dir, ENV_LOCK};
use sonomux_core::AppConfig;
use sonomux_daemon::dbus_service::SonomuxDbusService;
use sonomux_device_goxlr::GoXLRClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[zbus::proxy(
    interface = "org.sonomux.Daemon",
    default_service = "org.sonomux.Daemon",
    default_path = "/org/sonomux/Daemon"
)]
trait Daemon {
    async fn get_status(&self) -> zbus::Result<(bool, String, bool, u8, f64, bool)>;
    async fn get_channel_volumes(&self) -> zbus::Result<HashMap<String, f64>>;
    async fn get_rules(&self) -> zbus::Result<Vec<(String, bool)>>;
    async fn set_rule_enabled(&self, rule_name: &str, enabled: bool) -> zbus::Result<()>;
    async fn get_routing_config(&self) -> zbus::Result<(bool, HashMap<String, String>)>;
    async fn set_app_override(&self, id: &str, target: &str) -> zbus::Result<()>;
    async fn set_split_unpaired(&self, enabled: bool) -> zbus::Result<()>;
}

/// Serve `svc` over an in-process socket pair; returns (server, client) connections.
async fn serve(svc: SonomuxDbusService) -> (zbus::Connection, zbus::Connection) {
    let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    let server = zbus::connection::Builder::async_io_unix_stream(a)
        .server(zbus::Guid::generate())
        .unwrap()
        .p2p()
        .serve_at(sonomux_core::ipc::OBJECT_PATH, svc)
        .unwrap()
        .build();
    let client = zbus::connection::Builder::async_io_unix_stream(b)
        .p2p()
        .build();
    let (s, c) = tokio::join!(server, client);
    (s.unwrap(), c.unwrap())
}

struct Fixture {
    config: Arc<Mutex<AppConfig>>,
    state: Arc<Mutex<sonomux_daemon::dbus_service::SharedDaemonState>>,
    config_home: std::path::PathBuf,
    _server: zbus::Connection,
    client: zbus::Connection,
}

impl Fixture {
    /// Caller must hold ENV_LOCK. Config is isolated and routing is off, so
    /// no handler can touch real audio state.
    async fn new(tag: &str) -> Self {
        let config_home = temp_dir(&format!("ipc-{tag}"));
        // SAFETY: caller holds ENV_LOCK.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config_home) };
        let mock = mock_goxlr(&format!("ipc-{tag}")).await;
        let mut cfg = AppConfig::default();
        cfg.routing.enabled = false;
        let config = Arc::new(Mutex::new(cfg));
        let state = shared_state();
        let svc = SonomuxDbusService {
            state: Arc::clone(&state),
            goxlr: GoXLRClient::new(&mock.socket),
            config: Arc::clone(&config),
            tokio_handle: tokio::runtime::Handle::current(),
        };
        let (_server, client) = serve(svc).await;
        Self {
            config,
            state,
            config_home,
            _server,
            client,
        }
    }

    async fn proxy(&self) -> DaemonProxy<'_> {
        DaemonProxy::builder(&self.client)
            .build()
            .await
            .expect("proxy")
    }

    fn saved_config(&self) -> String {
        std::fs::read_to_string(self.config_home.join("sonomux/config.toml")).unwrap()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_status_returns_all_fields() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("status").await;
    {
        let mut s = f.state.lock().await;
        s.goxlr_active = true;
        s.headset_status = "Active".into();
        s.headset_active = true;
        s.battery_pct = 77;
        s.speaker_volume = 42.5;
        s.loopback_active = true;
    }
    let got = f.proxy().await.get_status().await.unwrap();
    assert_eq!(got, (true, "Active".to_string(), true, 77, 42.5, true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_channel_volumes_has_all_channels() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("vols").await;
    let vols = f.proxy().await.get_channel_volumes().await.unwrap();
    let mut keys: Vec<_> = vols.keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["Chat", "Game", "Headphones", "Mic", "Music", "System"]
    );
    assert_eq!(vols["Game"], 85.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_rule_enabled_updates_rules_and_saves() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("rule").await;
    let p = f.proxy().await;
    p.set_rule_enabled("Auto Headset Switcher", false)
        .await
        .unwrap();
    let rules = p.get_rules().await.unwrap();
    let rule = rules
        .iter()
        .find(|(n, _)| n == "Auto Headset Switcher")
        .expect("default rule present");
    assert!(!rule.1);
    assert!(f.saved_config().contains("enabled = false"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_app_override_adds_and_removes() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("override").await;
    let p = f.proxy().await;
    p.set_app_override("com.example.app", "Music")
        .await
        .unwrap();
    assert_eq!(
        f.config
            .lock()
            .await
            .routing
            .app_overrides
            .get("com.example.app")
            .map(String::as_str),
        Some("Music")
    );
    assert!(f.saved_config().contains("com.example.app"));
    let (enabled, _) = p.get_routing_config().await.unwrap();
    assert!(!enabled);

    p.set_app_override("com.example.app", "").await.unwrap();
    assert!(!f
        .config
        .lock()
        .await
        .routing
        .app_overrides
        .contains_key("com.example.app"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_split_unpaired_updates_config() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("split").await;
    f.proxy().await.set_split_unpaired(true).await.unwrap();
    assert!(f.config.lock().await.devices.split_unpaired_inputs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_split_unpaired_reports_save_failure() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new("savefail").await;
    // Make XDG_CONFIG_HOME a regular file so creating <it>/sonomux/config.toml fails.
    std::fs::remove_dir_all(&f.config_home).unwrap();
    std::fs::write(&f.config_home, "not a directory").unwrap();
    let err = f
        .proxy()
        .await
        .set_split_unpaired(true)
        .await
        .expect_err("save failure must reach the caller");
    assert!(
        err.to_string().contains("Could not save config"),
        "unexpected error: {err}"
    );
    let _ = std::fs::remove_file(&f.config_home);
}
