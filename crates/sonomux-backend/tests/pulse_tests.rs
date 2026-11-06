use serde_json::json;
use sonomux_backend::PulseManager;

#[test]
fn test_pulse_manager_constructor() {
    let _mgr = PulseManager::new();
}

#[tokio::test]
async fn test_pulse_manager_live_queries() {
    let _ = PulseManager::sink_exists("non_existent_sink_12345").await;
    let _ = PulseManager::source_exists("non_existent_source_12345").await;
    let _ = PulseManager::find_loopback_module("non_existent_source_12345").await;
    let _ = PulseManager::is_loopback_healthy(99999, "non_existent_source_12345").await;
    let _ = PulseManager::resolve_effective_sink("effect_input.dummy").await;
    let _ = PulseManager::unload_module(999999).await;
    let _ = PulseManager::ensure_loopback("invalid_source_123", "invalid_sink_123", 20).await;
    let _ = PulseManager::enforce_surround_policies(
        "dummy_out",
        "dummy_in",
        "dummy_game",
        "dummy_system",
        &["DummyApp"],
    )
    .await;
}

#[test]
fn test_parse_exists() {
    let output = "1\talsa_output.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts16le 2ch 48000Hz\tSUSPENDED\n\
                  2\talsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink\tPipeWire\ts32le 2ch 48000Hz\tRUNNING\n";

    assert!(PulseManager::parse_exists(
        output,
        "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink"
    ));
    assert!(!PulseManager::parse_exists(output, "non_existent_sink"));
}

#[test]
fn test_parse_source_index() {
    let output = "1\talsa_input.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts16le 2ch 48000Hz\n\
                  42\talsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source\tPipeWire\ts32le 2ch 48000Hz\n";

    let idx = PulseManager::parse_source_index(
        output,
        "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source",
    );
    assert_eq!(idx, Some(42));

    let not_found = PulseManager::parse_source_index(output, "unknown_source");
    assert_eq!(not_found, None);
}

#[test]
fn test_parse_sink_name_by_index() {
    let output = "10\talsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink\n\
                  11\talsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink\n";

    assert_eq!(
        PulseManager::parse_sink_name_by_index(output, "11"),
        Some("alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink".to_string())
    );
    assert_eq!(PulseManager::parse_sink_name_by_index(output, "99"), None);
}

#[tokio::test]
async fn test_resolve_effective_sink_fast_paths() {
    let empty = PulseManager::resolve_effective_sink("").await;
    assert_eq!(empty, "");

    let alsa = PulseManager::resolve_effective_sink(
        "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink",
    )
    .await;
    assert_eq!(
        alsa,
        "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink"
    );
}

#[test]
fn test_parse_loopback_module_id() {
    let output = "14\tmodule-always-sink\t\n\
                  27\tmodule-loopback\tsource=alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source sink=alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo latency_msec=20\n\
                  35\tmodule-native-protocol-unix\t\n";

    let mod_id = PulseManager::parse_loopback_module_id(
        output,
        "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source",
    );
    assert_eq!(mod_id, Some(27));

    let not_found = PulseManager::parse_loopback_module_id(output, "non_existent_source");
    assert_eq!(not_found, None);
}

#[test]
fn test_parse_loopback_module_id_exact_source() {
    let input = "10\tmodule-loopback\tsource=mix_src.monitor sink=a\n\
                 11\tmodule-loopback\tsource=mix_src_extra sink=b\n\
                 12\tmodule-loopback\tsink=c source=mix_src latency_msec=20\n\
                 13\tmodule-null-sink\tsource=mix_src\n";

    assert_eq!(
        PulseManager::parse_loopback_module_id(input, "mix_src"),
        Some(12)
    );
    assert_eq!(
        PulseManager::parse_loopback_module_id(input, "mix_src.monitor"),
        Some(10)
    );
    assert_eq!(PulseManager::parse_loopback_module_id(input, "mix"), None);
}

#[test]
fn test_is_stream_on_source_healthy() {
    let source_outputs = json!([
        {
            "index": 1,
            "owner_module": 27,
            "source": 42
        },
        {
            "index": 2,
            "owner_module": 10,
            "source": 5
        }
    ]);

    assert!(PulseManager::is_stream_on_source(&source_outputs, 27, 42));
    assert!(!PulseManager::is_stream_on_source(&source_outputs, 27, 5));
    assert!(!PulseManager::is_stream_on_source(&source_outputs, 999, 42));
}

#[test]
fn test_is_stream_on_source_fallback_detected() {
    let source_outputs = json!([
        {
            "index": 1,
            "owner_module": 27,
            "source": 1
        }
    ]);

    assert!(!PulseManager::is_stream_on_source(&source_outputs, 27, 42));
}

#[test]
fn test_compute_surround_moves_vsm_misplaced() {
    let sinks = json!([
        { "index": 10, "name": "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink" },
        { "index": 11, "name": "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink" },
        { "index": 12, "name": "effect_input.virtual-surround-manager" }
    ]);

    let inputs = json!([
        {
            "index": 101,
            "sink": 10,
            "properties": {
                "node.name": "effect_output.virtual-surround-manager",
                "application.name": "virtual-surround-manager"
            }
        },
        {
            "index": 102,
            "sink": 12,
            "properties": {
                "node.name": "Zen",
                "application.name": "Zen"
            }
        },
        {
            "index": 103,
            "sink": 12,
            "properties": {
                "node.name": "Cyberpunk2077",
                "application.name": "Cyberpunk2077"
            }
        }
    ]);

    let moves = PulseManager::compute_surround_moves(
        &inputs,
        &sinks,
        "effect_output.virtual-surround-manager",
        "effect_input.virtual-surround-manager",
        "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink",
        "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink",
        &["Zen"],
    );

    assert_eq!(moves.len(), 2);
    assert_eq!(
        moves[0],
        (
            101,
            "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink".to_string()
        )
    );
    assert_eq!(
        moves[1],
        (
            102,
            "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink".to_string()
        )
    );
}

#[test]
fn test_parse_downstream_target_index() {
    let stdout = "Sink Input #54\n\
                  \tDriver: PipeWire\n\
                  \tOwner Module: n/a\n\
                  \tClient: 120\n\
                  \tSink: 48\n\
                  \tnode.name = \"omarchy_speaker_tuning\"\n\
                  \tapplication.name = \"Filter Chain\"\n";

    let target = PulseManager::parse_downstream_target_index(stdout, "omarchy_speaker_tuning");
    assert_eq!(target, Some("48".to_string()));

    let not_found = PulseManager::parse_downstream_target_index(stdout, "non_existent_dsp");
    assert_eq!(not_found, None);
}

#[cfg(target_os = "linux")] // shells out to pactl
#[tokio::test]
async fn test_pulse_manager_loopback_and_routing_apis() {
    // Non-existent module health check
    let healthy = PulseManager::is_loopback_healthy(999_999, "non_existent_source").await;
    assert!(!healthy);

    // Non-existent loopback finder
    let found = PulseManager::find_loopback_module("sonomux_test_dummy_source_12345").await;
    assert_eq!(found, None);

    // Unload non-existent module (should fail gracefully or succeed with error)
    let unload_res = PulseManager::unload_module(999_999).await;
    let _ = unload_res;

    // Enforce routing policy execution (with harmless names)
    let enforce_res = PulseManager::enforce_surround_policies(
        "effect_output.virtual-surround-manager",
        "effect_input.virtual-surround-manager",
        "non_existent_line1",
        "non_existent_speaker",
        &["NonExistentApp"],
    )
    .await;
    assert!(enforce_res.is_ok());

    // Resolve effective sink non-existent fallback
    let resolved = PulseManager::resolve_effective_sink("dummy_virtual_dsp").await;
    assert_eq!(resolved, "dummy_virtual_dsp");

    // Attempt loopback creation for non-existent source
    let loopback_res =
        PulseManager::ensure_loopback("dummy_source_123", "dummy_sink_123", 20).await;
    let _ = loopback_res;

    // Get sink name by index (non-existent)
    let sink_name = PulseManager::get_sink_name_by_index("999_999").await;
    assert_eq!(sink_name, None);
}

#[test]
fn test_parse_sink_inputs_json_structured() {
    let json_data = json!([
        {
            "index": 101,
            "sink": 2,
            "corked": false,
            "mute": false,
            "properties": {
                "application.name": "Spotify",
                "application.process.binary": "spotify",
                "media.role": "music",
                "media.name": "Track Playback",
                "node.name": "spotify-player",
                "application.process.id": "12345"
            }
        },
        {
            "index": 102,
            "sink": 1,
            "corked": true,
            "mute": false,
            "properties": {
                "application.name": "Zen",
                "application.process.binary": "zen-bin",
                "media.role": "none",
                "node.name": "Zen"
            }
        }
    ]);

    let mut sink_id_to_name = std::collections::HashMap::new();
    sink_id_to_name.insert(1, "system_sink".to_string());
    sink_id_to_name.insert(2, "music_sink".to_string());

    let streams = PulseManager::parse_sink_inputs_json(&json_data, &sink_id_to_name);
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0].index, 101);
    assert_eq!(streams[0].sink_name, "music_sink");
    assert_eq!(streams[0].app_name, "Spotify");
    assert_eq!(streams[0].media_role, Some("music".to_string()));
    assert!(!streams[0].is_corked);

    assert_eq!(streams[1].index, 102);
    assert_eq!(streams[1].sink_name, "system_sink");
    assert_eq!(streams[1].binary, "zen-bin");
    assert!(streams[1].is_corked);
}

#[test]
fn test_compute_routing_moves_categories() {
    let routing_config = sonomux_core::RoutingConfig {
        enabled: true,
        ..Default::default()
    };
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    // Stream 1: Spotify currently on System sink (should move to Music sink)
    let mut s1 = sonomux_core::StreamInfo::new(1, 10);
    s1.app_name = "Spotify".to_string();
    s1.binary = "spotify".to_string();
    s1.sink_name = devices.goxlr_system_sink.clone();

    // Stream 2: Zen Browser currently on System sink (should stay on System sink)
    let mut s2 = sonomux_core::StreamInfo::new(2, 10);
    s2.app_name = "Zen".to_string();
    s2.binary = "zen-bin".to_string();
    s2.sink_name = devices.goxlr_system_sink.clone();

    // Stream 3: Game (Cyberpunk2077.exe) currently on System sink (should move to Surround node when available)
    let mut s3 = sonomux_core::StreamInfo::new(3, 10);
    s3.app_name = "Cyberpunk2077.exe".to_string();
    s3.binary = "wine64-preloader".to_string();
    s3.sink_name = devices.goxlr_system_sink.clone();

    // Stream 4: Discord on System sink (should move to Chat sink)
    let mut s4 = sonomux_core::StreamInfo::new(4, 10);
    s4.app_name = "Discord".to_string();
    s4.binary = "discord".to_string();
    s4.sink_name = devices.goxlr_system_sink.clone();

    let streams = vec![s1, s2, s3, s4];
    let moves = PulseManager::compute_routing_moves(
        &streams,
        &routing_config,
        &devices,
        &policies,
        true, // surround available
    );

    assert_eq!(moves.len(), 3);
    // Spotify -> goxlr_music_sink
    assert_eq!(moves[0].0, 1);
    assert_eq!(moves[0].1, devices.goxlr_music_sink);

    // Game -> policies.surround_in_node
    assert_eq!(moves[1].0, 3);
    assert_eq!(moves[1].1, policies.surround_in_node);

    // Discord -> goxlr_chat_sink
    assert_eq!(moves[2].0, 4);
    assert_eq!(moves[2].1, devices.goxlr_chat_sink);
}

#[test]
fn test_compute_routing_moves_fallback_when_surround_unavailable() {
    let routing_config = sonomux_core::RoutingConfig::default();
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    let mut game_stream = sonomux_core::StreamInfo::new(42, 10);
    game_stream.app_name = "eldenring.exe".to_string();
    game_stream.binary = "wine64".to_string();
    game_stream.sink_name = devices.goxlr_system_sink.clone();

    let moves = PulseManager::compute_routing_moves(
        &[game_stream],
        &routing_config,
        &devices,
        &policies,
        false, // surround NOT available -> fallback to goxlr_game_sink!
    );

    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].0, 42);
    assert_eq!(moves[0].1, devices.goxlr_game_sink);
}

#[test]
fn test_compute_routing_moves_surround_override() {
    let mut routing_config = sonomux_core::RoutingConfig::default();
    routing_config
        .app_overrides
        .insert("myapp".to_string(), "Surround".to_string());
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    let mut stream = sonomux_core::StreamInfo::new(7, 10);
    stream.app_name = "myapp".to_string();
    stream.binary = "myapp".to_string();
    stream.sink_name = devices.goxlr_system_sink.clone();

    let moves = PulseManager::compute_routing_moves(
        &[stream.clone()],
        &routing_config,
        &devices,
        &policies,
        true,
    );
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].1, policies.surround_in_node);

    let moves =
        PulseManager::compute_routing_moves(&[stream], &routing_config, &devices, &policies, false);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].1, devices.goxlr_game_sink);
}

#[test]
fn test_compute_routing_moves_keep_off_surround_enforced() {
    let routing_config = sonomux_core::RoutingConfig::default();
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    // Stream: Zen Browser mistakenly routed to surround_in_node
    let mut zen_stream = sonomux_core::StreamInfo::new(99, 5);
    zen_stream.app_name = "Zen".to_string();
    zen_stream.binary = "zen-bin".to_string();
    zen_stream.sink_name = policies.surround_in_node.clone();

    let moves = PulseManager::compute_routing_moves(
        &[zen_stream],
        &routing_config,
        &devices,
        &policies,
        true,
    );

    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].0, 99);
    assert_eq!(moves[0].1, devices.goxlr_system_sink); // Diverted to System!
}

#[test]
fn test_compute_routing_disabled_skips_all() {
    let routing_config = sonomux_core::RoutingConfig {
        enabled: false, // Disabled
        ..Default::default()
    };
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    let mut stream = sonomux_core::StreamInfo::new(1, 10);
    stream.app_name = "Spotify".to_string();
    stream.sink_name = devices.goxlr_system_sink.clone();

    let moves =
        PulseManager::compute_routing_moves(&[stream], &routing_config, &devices, &policies, true);
    assert!(moves.is_empty());
}

#[tokio::test]
async fn test_pulse_manager_stream_routing_async_apis() {
    let _ = PulseManager::get_active_streams().await;
    let _ = PulseManager::move_stream(999_999, "non_existent_sink").await;

    let routing_config = sonomux_core::RoutingConfig::default();
    let devices = sonomux_core::DevicesConfig::default();
    let policies = sonomux_core::PoliciesConfig::default();

    let _ = PulseManager::enforce_stream_routing(&routing_config, &devices, &policies).await;
}
