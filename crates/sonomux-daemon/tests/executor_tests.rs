// ENV_LOCK is held across awaits on purpose: PATH/XDG env vars are process-global.
#![allow(clippy::await_holding_lock)]

mod common;

use common::{mock_goxlr, shared_state, temp_dir, MockGoxlr};
use sonomux_core::rules::Action;
use sonomux_daemon::dbus_service::SharedDaemonState;
use sonomux_daemon::executor::ActionExecutor;
use sonomux_daemon::state::StateStore;
use sonomux_device_goxlr::GoXLRClient;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

struct Harness {
    mock: MockGoxlr,
    goxlr: GoXLRClient,
    store: StateStore,
    state_dir: PathBuf,
    shared: Arc<Mutex<SharedDaemonState>>,
    loopback_id: Option<u32>,
}

impl Harness {
    async fn new(tag: &str) -> Self {
        let mock = mock_goxlr(tag).await;
        let goxlr = GoXLRClient::new(&mock.socket);
        let state_dir = temp_dir(&format!("exec-{tag}"));
        let store = StateStore::in_dir(state_dir.clone());
        Self {
            mock,
            goxlr,
            store,
            state_dir,
            shared: shared_state(),
            loopback_id: None,
        }
    }

    async fn run(&mut self, action: Action) {
        let mut exec = ActionExecutor {
            goxlr: &self.goxlr,
            state_store: &self.store,
            shared_state: &self.shared,
            current_loopback_id: &mut self.loopback_id,
        };
        exec.execute_action(&action).await;
    }

    fn requests_with(&self, needle: &str) -> Vec<String> {
        self.mock
            .requests()
            .into_iter()
            .filter(|r| r.contains(needle))
            .collect()
    }
}

fn s(v: &str) -> String {
    v.to_string()
}

#[tokio::test]
async fn set_hardware_volume_sends_set_volume() {
    let mut h = Harness::new("setvol").await;
    h.run(Action::SetHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        level: 0.0,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1, "{:?}", h.mock.requests());
    assert!(
        reqs[0].contains(r#""SetVolume":["Headphones",0]"#),
        "{}",
        reqs[0]
    );
    assert_eq!(h.shared.lock().await.speaker_volume, 0.0);
}

#[tokio::test]
async fn save_hardware_volume_writes_state_file() {
    let mut h = Harness::new("savevol").await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("speaker_volume"),
    })
    .await;
    let saved = std::fs::read_to_string(h.state_dir.join("saved-speaker-volume")).unwrap();
    assert_eq!(saved.trim(), "200");
}

#[tokio::test]
async fn restore_hardware_volume_uses_saved_value() {
    let mut h = Harness::new("restore").await;
    std::fs::write(h.state_dir.join("saved-speaker-volume"), "120").unwrap();
    h.run(Action::RestoreHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("speaker_volume"),
        default_level: 0.9,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains(r#""SetVolume":["Headphones",120]"#),
        "{}",
        reqs[0]
    );
    let vol = h.shared.lock().await.speaker_volume;
    assert!((vol - 120.0 / 255.0 * 100.0).abs() < 0.01, "{vol}");
}

#[tokio::test]
async fn restore_hardware_volume_falls_back_to_default() {
    let mut h = Harness::new("restoredef").await;
    h.run(Action::RestoreHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("speaker_volume"),
        default_level: 0.5,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains(r#""SetVolume":["Headphones",128]"#),
        "{}",
        reqs[0]
    );
}

#[tokio::test]
async fn save_uses_named_channel_and_slot() {
    let mut h = Harness::new("saveslot").await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Game"),
        slot: s("game_before"),
    })
    .await;
    let saved = std::fs::read_to_string(h.state_dir.join("saved-volume-game_before")).unwrap();
    assert_eq!(saved.trim(), "150");
}

#[tokio::test]
async fn restore_uses_named_slot_and_channel() {
    let mut h = Harness::new("restoreslot").await;
    std::fs::write(h.state_dir.join("saved-volume-game_before"), "90").unwrap();
    h.run(Action::RestoreHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Game"),
        slot: s("game_before"),
        default_level: 1.0,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains(r#""SetVolume":["Game",90]"#),
        "{}",
        reqs[0]
    );
}

#[tokio::test]
async fn speaker_volume_slot_reads_legacy_file() {
    let mut h = Harness::new("legacyslot").await;
    std::fs::write(h.state_dir.join("saved-speaker-volume"), "77").unwrap();
    h.run(Action::RestoreHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("speaker_volume"),
        default_level: 1.0,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains(r#""SetVolume":["Headphones",77]"#),
        "{}",
        reqs[0]
    );
}

#[tokio::test]
async fn slots_do_not_overwrite_each_other() {
    let mut h = Harness::new("slotsmulti").await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("a"),
    })
    .await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Game"),
        slot: s("b"),
    })
    .await;
    let a = std::fs::read_to_string(h.state_dir.join("saved-volume-a")).unwrap();
    let b = std::fs::read_to_string(h.state_dir.join("saved-volume-b")).unwrap();
    assert_eq!((a.trim(), b.trim()), ("200", "150"));
}

#[tokio::test]
async fn unsafe_slot_name_stays_in_state_dir() {
    let mut h = Harness::new("unsafeslot").await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("Headphones"),
        slot: s("../escape"),
    })
    .await;
    assert!(h.state_dir.join("saved-volume-___escape").exists());
    assert!(!h.state_dir.parent().unwrap().join("escape").exists());
}

#[tokio::test]
async fn unknown_channel_is_not_saved() {
    let mut h = Harness::new("unknownchan").await;
    h.run(Action::SaveHardwareVolume {
        device_id: s("goxlr"),
        channel: s("NoSuchChannel"),
        slot: s("x"),
    })
    .await;
    assert!(!h.state_dir.join("saved-volume-x").exists());
}

#[tokio::test]
async fn set_hardware_mute_sends_zero_volume() {
    let mut h = Harness::new("mute").await;
    h.run(Action::SetHardwareMute {
        device_id: s("goxlr"),
        channel: s("Chat"),
        muted: true,
    })
    .await;
    let reqs = h.requests_with("SetVolume");
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].contains(r#""SetVolume":["Chat",0]"#), "{}", reqs[0]);
}

#[tokio::test]
async fn set_routing_sends_set_router() {
    let mut h = Harness::new("route").await;
    h.run(Action::SetRouting {
        device_id: s("goxlr"),
        input: s("Microphone"),
        output: s("Sampler"),
        enabled: false,
    })
    .await;
    let reqs = h.requests_with("SetRouter");
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains(r#""SetRouter":["Microphone","Sampler",false]"#),
        "{}",
        reqs[0]
    );
}

#[tokio::test]
async fn setup_submix_matrix_sends_eight_routes() {
    let mut h = Harness::new("submix").await;
    h.run(Action::SetupSubmixMatrix {
        device_id: s("goxlr"),
    })
    .await;
    let reqs = h.requests_with("SetRouter");
    assert_eq!(reqs.len(), 8, "{reqs:?}");
    assert_eq!(reqs.iter().filter(|r| r.contains("false")).count(), 1);
    assert_eq!(reqs.iter().filter(|r| r.contains("true")).count(), 7);
}

#[tokio::test]
async fn other_device_ids_are_ignored() {
    let mut h = Harness::new("other").await;
    let actions = vec![
        Action::SetHardwareVolume {
            device_id: s("other"),
            channel: s("Headphones"),
            level: 0.5,
        },
        Action::SaveHardwareVolume {
            device_id: s("other"),
            channel: s("Headphones"),
            slot: s("x"),
        },
        Action::RestoreHardwareVolume {
            device_id: s("other"),
            channel: s("Headphones"),
            slot: s("x"),
            default_level: 0.5,
        },
        Action::SetHardwareMute {
            device_id: s("other"),
            channel: s("Chat"),
            muted: true,
        },
        Action::SetRouting {
            device_id: s("other"),
            input: s("Microphone"),
            output: s("Sampler"),
            enabled: true,
        },
        Action::SetupSubmixMatrix {
            device_id: s("other"),
        },
    ];
    for a in actions {
        h.run(a).await;
    }
    assert!(h.mock.requests().is_empty(), "{:?}", h.mock.requests());
}

#[tokio::test]
async fn command_action_does_not_panic() {
    let mut h = Harness::new("command").await;
    h.run(Action::Command {
        program: s("true"),
        args: vec![],
    })
    .await;
    h.run(Action::Command {
        program: s("/nonexistent/sonomux-test"),
        args: vec![],
    })
    .await;
}

// These use the Linux pactl backend; on macOS the backend is CoreAudio.
#[cfg(not(target_os = "macos"))]
mod linux_backend {
    use super::*;

    #[tokio::test]
    async fn ensure_loopback_reuses_existing_module() {
        let _env = common::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        common::use_fake_pactl();
        let mut h = Harness::new("ensurelb").await;
        h.run(Action::EnsureLoopback {
            source: s("mix_src"),
            sink: s("alsa_output.real_sink"),
            latency_ms: 20,
        })
        .await;
        assert_eq!(h.loopback_id, Some(7));
        assert!(h.shared.lock().await.loopback_active);
    }

    #[tokio::test]
    async fn destroy_loopback_clears_state() {
        let _env = common::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        common::use_fake_pactl();
        let mut h = Harness::new("destroylb").await;
        h.loopback_id = Some(8);
        h.shared.lock().await.loopback_active = true;
        h.run(Action::DestroyLoopback {
            source: s("mix_src"),
        })
        .await;
        assert_eq!(h.loopback_id, None);
        assert!(!h.shared.lock().await.loopback_active);
    }

    #[tokio::test]
    async fn route_stream_does_not_panic() {
        let _env = common::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        common::use_fake_pactl();
        let mut h = Harness::new("routestream").await;
        h.run(Action::RouteStream {
            app_or_binary: s("spotify"),
            target_sink: s("alsa_output.real_sink"),
        })
        .await;
    }
}
