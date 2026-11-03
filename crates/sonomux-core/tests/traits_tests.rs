use sonomux_core::traits::*;

#[test]
fn test_device_event_serialization() {
    let evt = DeviceEvent::StatusChanged {
        id: "headset".to_string(),
        status: DeviceStatus::Active,
    };
    let json_str = serde_json::to_string(&evt).expect("Failed to serialize event");
    assert!(json_str.contains("StatusChanged"));
    assert!(json_str.contains("Active"));

    let deserialized: DeviceEvent = serde_json::from_str(&json_str).expect("Failed to deserialize event");
    match deserialized {
        DeviceEvent::StatusChanged { id, status } => {
            assert_eq!(id, "headset");
            assert_eq!(status, DeviceStatus::Active);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_device_status_variants() {
    assert_ne!(DeviceStatus::Disconnected, DeviceStatus::Standby);
    assert_ne!(DeviceStatus::Standby, DeviceStatus::Active);
}
