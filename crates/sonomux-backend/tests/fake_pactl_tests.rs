//! Exercises the PulseManager `pactl` calls against a fake `pactl` on PATH, so they run the
//! same on any machine (including CI, which has `pactl` but no sound server).

use sonomux_backend::PulseManager;
use std::sync::Once;

const FAKE_PACTL: &str = r#"#!/bin/sh
case "$*" in
  "list sinks short") printf '1\tvirt_out\tPipeWire\n2\talsa_output.real_sink\tPipeWire\n3\ts\tPipeWire\n' ;;
  "list sources short") printf '42\tmix_src\tPipeWire\n50\tbad_src\tPipeWire\n51\tnew_src\tPipeWire\n' ;;
  "list modules short") printf '7\tmodule-loopback\tsource=mix_src sink=x\n8\tmodule-loopback\tsource=bad_src sink=x\n' ;;
  "-f json list source-outputs") echo '[{"owner_module": 7, "source": 42}]' ;;
  "-f json list sinks") echo '[{"index": 1, "name": "virt_out"}, {"index": 2, "name": "alsa_output.real_sink"}]' ;;
  "-f json list sink-inputs") echo '[{"index": 5, "sink": 1, "corked": false, "mute": false,
      "properties": {"application.name": "Spotify", "application.process.binary": "spotify",
                     "media.role": "music", "application.process.id": "1"}}]' ;;
  "list sink-inputs") printf 'Sink Input #5\n\tSink: 2\n\t\tnode.name = "virt_out.monitor"\n' ;;
  load-module*) echo 99 ;;
  "unload-module 8") ;;
  unload-module*) echo "no such module" >&2; exit 1 ;;
  "move-sink-input 5 "*) ;;
  move-sink-input*) echo "no such stream" >&2; exit 1 ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

fn use_fake_pactl() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sonomux-fake-pactl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("pactl");
        std::fs::write(&bin, FAKE_PACTL).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        // Every test in this binary sets the same value, once.
        unsafe { std::env::set_var("PATH", path) };
    });
}

#[tokio::test]
async fn device_queries() {
    use_fake_pactl();
    assert!(PulseManager::sink_exists("virt_out").await);
    assert!(!PulseManager::sink_exists("missing").await);
    assert!(PulseManager::source_exists("mix_src").await);
    assert_eq!(PulseManager::find_loopback_module("mix_src").await, Some(7));
    assert_eq!(
        PulseManager::get_sink_name_by_index("2").await.as_deref(),
        Some("alsa_output.real_sink")
    );
}

#[tokio::test]
async fn loopback_lifecycle() {
    use_fake_pactl();
    // Healthy existing module is reused.
    assert!(PulseManager::is_loopback_healthy(7, "mix_src").await);
    assert_eq!(
        PulseManager::ensure_loopback("mix_src", "s", 20)
            .await
            .unwrap(),
        7
    );
    // Detached module (8) is unloaded and replaced.
    assert!(!PulseManager::is_loopback_healthy(8, "bad_src").await);
    assert_eq!(
        PulseManager::ensure_loopback("bad_src", "s", 20)
            .await
            .unwrap(),
        99
    );
    // No existing module: a new one is loaded.
    assert_eq!(
        PulseManager::ensure_loopback("new_src", "s", 20)
            .await
            .unwrap(),
        99
    );
    assert!(!PulseManager::is_loopback_healthy(7, "missing_src").await);

    assert!(PulseManager::unload_module(8).await.is_ok());
    assert!(PulseManager::unload_module(13).await.is_err());
}

#[tokio::test]
async fn streams_and_routing() {
    use_fake_pactl();
    let streams = PulseManager::get_active_streams().await.unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].app_name, "Spotify");
    assert_eq!(streams[0].sink_name, "virt_out");

    assert!(PulseManager::move_stream(5, "alsa_output.real_sink")
        .await
        .is_ok());
    assert!(PulseManager::move_stream(6, "alsa_output.real_sink")
        .await
        .is_err());

    // Virtual sink resolves to the physical sink it feeds; physical sinks resolve to themselves.
    assert_eq!(
        PulseManager::resolve_effective_sink("virt_out").await,
        "alsa_output.real_sink"
    );
    assert_eq!(
        PulseManager::resolve_effective_sink("alsa_output.x").await,
        "alsa_output.x"
    );

    let cfg = sonomux_core::AppConfig::default();
    assert!(
        PulseManager::enforce_stream_routing(&cfg.routing, &cfg.devices, &cfg.policies)
            .await
            .is_ok()
    );
    assert!(
        PulseManager::enforce_surround_policies("virt_out", "virt_in", "g", "s", &["Zen"])
            .await
            .is_ok()
    );
}
