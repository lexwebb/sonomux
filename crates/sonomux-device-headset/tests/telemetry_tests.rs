use sonomux_core::traits::{AudioDevice, DeviceStatus, HasPresenceDetection};
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

#[tokio::test]
async fn test_headset_monitor_traits_initial_state() {
    let monitor = HeadsetMonitor::new("SteelSeries Arctis Nova 7", 2);

    assert_eq!(monitor.metadata().id, "headset");
    assert_eq!(monitor.metadata().name, "SteelSeries Arctis Nova 7");
    assert_eq!(monitor.status().await, DeviceStatus::Standby);
    assert!(!monitor.is_physically_active().await);
    assert_eq!(monitor.battery_level().await, None);
    assert_eq!(monitor.is_charging().await, None);

    let _rx = monitor.subscribe_events().await;
}

#[tokio::test]
async fn test_headset_monitor_background_loop() {
    let monitor = HeadsetMonitor::new("SteelSeries Arctis Nova 7", 1);
    let _rx = monitor.subscribe_events().await;
    let handle = monitor.start_monitor(std::time::Duration::from_millis(50));
    
    // Exercise loop, probe, debounce, and state update
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    handle.abort();

    let status = monitor.status().await;
    assert!(status == DeviceStatus::Standby || status == DeviceStatus::Active);
}
