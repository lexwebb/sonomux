#![cfg(target_os = "macos")]
//! Read-only checks against the live CoreAudio HAL. Safe on machines with no audio hardware.

use sonomux_backend::coreaudio::CoreAudioManager;

#[tokio::test]
async fn devices_are_found_by_name_and_uid() {
    let devices = CoreAudioManager::devices();
    for d in &devices {
        assert!(!d.uid.is_empty(), "device {} has no UID", d.id);
    }
    if let Some(out) = devices.iter().find(|d| d.has_output) {
        assert!(CoreAudioManager::sink_exists(&out.name).await);
        assert!(CoreAudioManager::sink_exists(&out.uid).await);
    }
    if let Some(inp) = devices.iter().find(|d| d.has_input) {
        assert!(CoreAudioManager::source_exists(&inp.uid).await);
    }
    assert!(!CoreAudioManager::sink_exists("sonomux-no-such-device").await);
    assert!(!CoreAudioManager::source_exists("sonomux-no-such-device").await);
}

#[tokio::test]
async fn active_streams_have_a_name() {
    for s in CoreAudioManager::get_active_streams().await.unwrap() {
        assert!(!s.app_name.is_empty() || s.pid.is_none(), "{:?}", s);
    }
}

#[tokio::test]
async fn unsupported_routing_is_reported_not_faked() {
    assert!(CoreAudioManager::ensure_loopback("a", "b", 20)
        .await
        .is_err());
    assert!(CoreAudioManager::move_stream(1, "b").await.is_err());
    assert_eq!(CoreAudioManager::find_loopback_module("a").await, None);
}
