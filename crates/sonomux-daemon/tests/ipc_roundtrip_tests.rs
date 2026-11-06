//! The shared client proxy (`sonomux_core::ipc::SonomuxDaemonProxy`) against the real service
//! over an in-memory socket pair. Read-only methods only: nothing here saves config or
//! touches audio.
#![allow(clippy::await_holding_lock)]

mod common;

use common::{shared_state, temp_dir, ENV_LOCK};
use sonomux_core::ipc::SonomuxDaemonProxy;
use sonomux_core::AppConfig;
use sonomux_daemon::dbus_service::SonomuxDbusService;
use sonomux_device_goxlr::GoXLRClient;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_proxy_agrees_with_service_on_read_methods() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("ipc-roundtrip");
    // SAFETY: ENV_LOCK is held.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::set_var("XDG_STATE_HOME", &dir);
    }
    let state = shared_state();
    {
        let mut s = state.lock().await;
        s.goxlr_active = true;
        s.headset_status = "Active".into();
        s.battery_pct = 55;
        s.speaker_volume = 33.0;
    }
    let svc = SonomuxDbusService {
        state: Arc::clone(&state),
        goxlr: GoXLRClient::new(dir.join("no-goxlr.socket")),
        config: Arc::new(Mutex::new(AppConfig::default())),
        tokio_handle: tokio::runtime::Handle::current(),
        routing_kick: Arc::new(tokio::sync::Notify::new()),
    };

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
    let (server, client) = tokio::join!(server, client);
    let (_server, client) = (server.unwrap(), client.unwrap());

    let p = SonomuxDaemonProxy::new(&client).await.unwrap();

    let (goxlr, headset, headset_active, battery, volume, loopback) = {
        let s = state.lock().await;
        (
            s.goxlr_active,
            s.headset_status.clone(),
            s.headset_active,
            s.battery_pct,
            s.speaker_volume,
            s.loopback_active,
        )
    };
    assert_eq!(
        p.get_status().await.unwrap(),
        (goxlr, headset, headset_active, battery, volume, loopback)
    );

    let names: Vec<String> = p
        .get_rules()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.0)
        .collect();
    let want: Vec<String> = AppConfig::default()
        .rules
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(names, want);

    assert!(p.get_routing_config().await.unwrap().0);
    p.get_channel_volumes().await.unwrap();
    p.get_device_settings().await.unwrap();
}
