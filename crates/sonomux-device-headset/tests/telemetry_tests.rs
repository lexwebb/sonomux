use sonomux_device_headset::HeadsetMonitor;

#[test]
fn test_parse_telemetry_headset_off() {
    let output = r#"
HEADSETCONTROL_NAME="HeadsetControl"
DEVICE_COUNT=1
DEVICE_0="SteelSeries Arctis Nova 7"
DEVICE_0_BATTERY_STATUS="BATTERY_UNAVAILABLE"
DEVICE_0_BATTERY_LEVEL=-1
DEVICE_0_ERROR_COUNT=1
DEVICE_0_ERROR_1_SOURCE="battery"
DEVICE_0_ERROR_1_MESSAGE="Device is offline or not responding"
"#;

    let telemetry = HeadsetMonitor::parse_telemetry(output);
    assert!(!telemetry.is_active);
    assert_eq!(telemetry.battery_percent, None);
    assert_eq!(telemetry.is_charging, None);
}

#[test]
fn test_parse_telemetry_headset_on_discharging() {
    let output = r#"
HEADSETCONTROL_NAME="HeadsetControl"
DEVICE_COUNT=1
DEVICE_0="SteelSeries Arctis Nova 7"
DEVICE_0_BATTERY_STATUS="BATTERY_AVAILABLE"
DEVICE_0_BATTERY_LEVEL=82
"#;

    let telemetry = HeadsetMonitor::parse_telemetry(output);
    assert!(telemetry.is_active);
    assert_eq!(telemetry.battery_percent, Some(82));
    assert_eq!(telemetry.is_charging, Some(false));
}

#[test]
fn test_parse_telemetry_headset_charging() {
    let output = r#"
HEADSETCONTROL_NAME="HeadsetControl"
DEVICE_COUNT=1
DEVICE_0="SteelSeries Arctis Nova 7"
DEVICE_0_BATTERY_STATUS="BATTERY_CHARGING"
DEVICE_0_BATTERY_LEVEL=100
"#;

    let telemetry = HeadsetMonitor::parse_telemetry(output);
    assert!(telemetry.is_active);
    assert_eq!(telemetry.battery_percent, Some(100));
    assert_eq!(telemetry.is_charging, Some(true));
}

#[test]
fn test_parse_telemetry_empty_or_malformed() {
    let telemetry = HeadsetMonitor::parse_telemetry("");
    assert!(!telemetry.is_active);
    assert_eq!(telemetry.battery_percent, None);

    let telemetry_garbage = HeadsetMonitor::parse_telemetry("some random daemon error message");
    assert!(!telemetry_garbage.is_active);
}
