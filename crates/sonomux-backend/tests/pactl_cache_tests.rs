//! Read-only `pactl` listings are reused within a short TTL and dropped after a graph change.
//! A fake `pactl` on PATH logs every invocation so the spawns can be counted.

use sonomux_backend::PulseManager;
use std::sync::OnceLock;
use std::time::Duration;

const FAKE_PACTL: &str = r#"#!/bin/sh
echo "$*" >> "$PACTL_LOG"
case "$*" in
  "list sinks short") printf '1\tvirt_out\tPipeWire\n2\talsa_output.real_sink\tPipeWire\n' ;;
  "list sources short") printf '42\tmix_src\tPipeWire\n50\tbad_src\tPipeWire\n' ;;
  "-f json list sinks") echo '[{"index": 1, "name": "virt_out"}, {"index": 2, "name": "alsa_output.real_sink"}]' ;;
  "-f json list sink-inputs") echo '[{"index": 5, "sink": 1, "corked": false, "mute": false,
      "properties": {"application.name": "Spotify", "application.process.binary": "spotify",
                     "media.role": "music", "application.process.id": "1"}}]' ;;
  "move-sink-input 5 "*) ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

fn log_path() -> &'static std::path::PathBuf {
    static LOG: OnceLock<std::path::PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sonomux-pactl-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("pactl");
        std::fs::write(&bin, FAKE_PACTL).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = dir.join("pactl.log");
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        // Set once, before any pactl call in this binary.
        unsafe {
            std::env::set_var("PATH", path);
            std::env::set_var("PACTL_LOG", &log);
        }
        log
    })
}

fn reset_log() {
    std::fs::write(log_path(), "").unwrap();
}

fn count(args: &str) -> usize {
    std::fs::read_to_string(log_path())
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == args)
        .count()
}

#[tokio::test]
async fn reads_are_reused_until_a_write_or_the_ttl() {
    log_path();
    reset_log();

    assert!(PulseManager::sink_exists("virt_out").await);
    assert!(PulseManager::sink_exists("virt_out").await);
    assert!(PulseManager::source_exists("mix_src").await);
    assert!(PulseManager::source_exists("mix_src").await);
    PulseManager::get_active_streams().await.unwrap();
    PulseManager::get_active_streams().await.unwrap();
    assert_eq!(count("list sinks short"), 1);
    assert_eq!(count("list sources short"), 1);
    assert_eq!(count("-f json list sink-inputs"), 1);
    assert_eq!(count("-f json list sinks"), 1);

    PulseManager::move_stream(5, "alsa_output.real_sink")
        .await
        .unwrap();
    PulseManager::get_active_streams().await.unwrap();
    assert_eq!(count("-f json list sink-inputs"), 2);

    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(PulseManager::sink_exists("virt_out").await);
    assert_eq!(count("list sinks short"), 2);
}
