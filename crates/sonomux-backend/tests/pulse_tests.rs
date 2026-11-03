use serde_json::json;
use sonomux_backend::PulseManager;

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

    // Module 27 is indeed reading from source 42
    assert!(PulseManager::is_stream_on_source(&source_outputs, 27, 42));

    // Module 27 is NOT reading from source 5
    assert!(!PulseManager::is_stream_on_source(&source_outputs, 27, 5));

    // Non-existent module
    assert!(!PulseManager::is_stream_on_source(&source_outputs, 999, 42));
}

#[test]
fn test_is_stream_on_source_fallback_detected() {
    // Simulating GoXLR disconnect where PipeWire dropped the loopback to default mic (source 1)
    let source_outputs = json!([
        {
            "index": 1,
            "owner_module": 27,
            "source": 1
        }
    ]);

    // Expected GoXLR mix is 42, but stream is on 1 -> should detect UNHEALTHY
    assert!(!PulseManager::is_stream_on_source(&source_outputs, 27, 42));
}
