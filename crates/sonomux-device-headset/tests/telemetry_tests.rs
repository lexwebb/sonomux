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

#[tokio::test]
async fn test_headset_monitor_default() {
    let monitor = HeadsetMonitor::default();
    assert_eq!(monitor.metadata().id, "headset");
    assert_eq!(monitor.metadata().name, "SteelSeries Arctis Nova 7");
}

#[tokio::test]
async fn test_process_sample_debouncing_and_events() {
    let (tx, mut rx) = tokio::sync::broadcast::channel(16);
    let mut pending_active = None;
    let mut pending_count = 0;
    let mut last_confirmed_active = None;
    let mut last_battery = None;
    let threshold = 2;
    let device_id = "test_headset";

    let active_sample = HeadsetMonitor::parse_telemetry(
        "BATTERY_AVAILABLE=1\nBATTERY_LEVEL=85\nBATTERY_CHARGING=0\n",
    );
    assert!(active_sample.is_active);

    // 1st sample: pending count goes to 1, no status change yet
    let res1 = HeadsetMonitor::process_sample(
        &active_sample,
        &mut pending_active,
        &mut pending_count,
        &mut last_confirmed_active,
        &mut last_battery,
        threshold,
        device_id,
        &tx,
    );
    assert_eq!(res1, None);
    assert_eq!(pending_count, 1);

    // 2nd sample: debounce threshold reached -> becomes Active
    let res2 = HeadsetMonitor::process_sample(
        &active_sample,
        &mut pending_active,
        &mut pending_count,
        &mut last_confirmed_active,
        &mut last_battery,
        threshold,
        device_id,
        &tx,
    );
    assert_eq!(res2, Some(DeviceStatus::Active));
    assert_eq!(pending_count, 2);

    // Check broadcast events
    let event1 = rx.recv().await.expect("Failed to receive status event");
    assert_eq!(
        event1,
        sonomux_core::traits::DeviceEvent::StatusChanged {
            id: device_id.to_string(),
            status: DeviceStatus::Active,
        }
    );
    let event2 = rx.recv().await.expect("Failed to receive battery event");
    assert_eq!(
        event2,
        sonomux_core::traits::DeviceEvent::BatteryChanged {
            id: device_id.to_string(),
            percentage: 85,
        }
    );

    // Battery drops to 80% while remaining active
    let battery_drop_sample = HeadsetMonitor::parse_telemetry(
        "BATTERY_AVAILABLE=1\nBATTERY_LEVEL=80\nBATTERY_CHARGING=0\n",
    );
    let res3 = HeadsetMonitor::process_sample(
        &battery_drop_sample,
        &mut pending_active,
        &mut pending_count,
        &mut last_confirmed_active,
        &mut last_battery,
        threshold,
        device_id,
        &tx,
    );
    assert_eq!(res3, None); // Status did not change, still Active
    let event3 = rx
        .recv()
        .await
        .expect("Failed to receive updated battery event");
    assert_eq!(
        event3,
        sonomux_core::traits::DeviceEvent::BatteryChanged {
            id: device_id.to_string(),
            percentage: 80,
        }
    );

    // Power off transition
    let off_sample = HeadsetMonitor::parse_telemetry("BATTERY_UNAVAILABLE=1\n");
    // Sample 1: pending resets to false, count 1
    let res4 = HeadsetMonitor::process_sample(
        &off_sample,
        &mut pending_active,
        &mut pending_count,
        &mut last_confirmed_active,
        &mut last_battery,
        threshold,
        device_id,
        &tx,
    );
    assert_eq!(res4, None);

    // Sample 2: count 2 -> transitions to Standby
    let res5 = HeadsetMonitor::process_sample(
        &off_sample,
        &mut pending_active,
        &mut pending_count,
        &mut last_confirmed_active,
        &mut last_battery,
        threshold,
        device_id,
        &tx,
    );
    assert_eq!(res5, Some(DeviceStatus::Standby));
    let event4 = rx.recv().await.expect("Failed to receive status event");
    assert_eq!(
        event4,
        sonomux_core::traits::DeviceEvent::StatusChanged {
            id: device_id.to_string(),
            status: DeviceStatus::Standby,
        }
    );
}
