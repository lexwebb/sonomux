//! Shared test harness: isolated dirs, a mock GoXLR socket, a fake `pactl`.
//! Each test file compiles this module separately, hence the `allow(dead_code)`s.

use serde_json::json;
use sonomux_daemon::dbus_service::SharedDaemonState;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

/// Hold this for the whole body of any test that touches environment variables.
#[allow(dead_code)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A fresh, empty directory under the system temp dir.
#[allow(dead_code)]
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonomux-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A mock GoXLR utility socket that records every request body.
#[allow(dead_code)]
pub struct MockGoxlr {
    pub socket: PathBuf,
    pub requests: Arc<std::sync::Mutex<Vec<String>>>,
}

#[allow(dead_code)]
impl MockGoxlr {
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Start a mock GoXLR socket. `GetStatus` reports serial `MOCK_SERIAL_123` with
/// Headphones 200 and Game 150; everything else is answered with `"Ok"`.
#[allow(dead_code)]
pub async fn mock_goxlr(tag: &str) -> MockGoxlr {
    let socket =
        std::env::temp_dir().join(format!("mock-goxlr-{}-{}.socket", tag, std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).expect("bind mock goxlr socket");
    let requests = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&requests);

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let recorded = Arc::clone(&recorded);
            tokio::spawn(async move {
                let mut len_buf = [0u8; 4];
                if stream.read_exact(&mut len_buf).await.is_err() {
                    return;
                }
                let mut req = vec![0u8; u32::from_be_bytes(len_buf) as usize];
                if stream.read_exact(&mut req).await.is_err() {
                    return;
                }
                let body = String::from_utf8_lossy(&req).to_string();
                recorded.lock().unwrap().push(body.clone());

                let resp = if body.contains("GetStatus") {
                    json!({
                        "Status": {
                            "mixers": {
                                "MOCK_SERIAL_123": {
                                    "levels": { "volumes": { "Headphones": 200, "Game": 150 } },
                                    "router": {
                                        "Microphone": { "Sampler": false, "Headphones": true }
                                    },
                                    "fader_status": {
                                        "B": { "channel": "Chat" },
                                        "A": { "channel": "Mic" }
                                    }
                                }
                            }
                        }
                    })
                } else {
                    json!("Ok")
                };
                let bytes = serde_json::to_vec(&resp).unwrap();
                let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes()).await;
                let _ = stream.write_all(&bytes).await;
                let _ = stream.flush().await;
            });
        }
    });

    MockGoxlr { socket, requests }
}

#[cfg(not(target_os = "macos"))]
const FAKE_PACTL: &str = r#"#!/bin/sh
case "$*" in
  "list sinks short") printf '1\tvirt_out\tPipeWire\n2\talsa_output.real_sink\tPipeWire\n' ;;
  "list sources short") printf '42\tmix_src\tPipeWire\n50\tbad_src\tPipeWire\n' ;;
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

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn use_fake_pactl() {
    use std::sync::Once;
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

/// Same initial values `sonomuxd` starts with (speaker volume from a 255 reading).
#[allow(dead_code)]
pub fn shared_state() -> Arc<tokio::sync::Mutex<SharedDaemonState>> {
    Arc::new(tokio::sync::Mutex::new(SharedDaemonState {
        goxlr_active: false,
        headset_status: "Standby".into(),
        headset_active: false,
        battery_pct: 0,
        speaker_volume: 100.0,
        system_volume: 100.0,
        game_volume: 85.0,
        chat_volume: 90.0,
        music_volume: 70.0,
        mic_volume: 100.0,
        loopback_active: false,
    }))
}
