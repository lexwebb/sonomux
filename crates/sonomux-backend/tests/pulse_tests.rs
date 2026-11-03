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
}
