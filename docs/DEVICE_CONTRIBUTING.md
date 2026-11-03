# Adding Devices to Sonomux

Sonomux is designed to be a generic, open-source platform. We welcome community drivers for audio interfaces, wireless headsets, and hardware mixers.

---

## Overview

Adding support for a device in Sonomux involves:
1. Identifying the device's communication protocol (USB HID, Unix Socket IPC, WebSocket, MIDI, or ALSA/UAC).
2. Implementing the core `AudioDevice` trait.
3. Implementing whichever capability traits the device physically supports.
4. Registering the driver in the device registry.

---

## 1. Implement `AudioDevice`

Every driver must implement the baseline `AudioDevice` trait:

```rust
use async_trait::async_trait;
use sonomux_core::traits::{AudioDevice, DeviceMetadata, DeviceStatus, DeviceEvent};
use tokio::sync::broadcast;

pub struct MyAudioDevice {
    metadata: DeviceMetadata,
    event_tx: broadcast::Sender<DeviceEvent>,
}

#[async_trait]
impl AudioDevice for MyAudioDevice {
    fn metadata(&self) -> &DeviceMetadata {
        &self.metadata
    }

    async fn status(&self) -> DeviceStatus {
        // Query connection state
        DeviceStatus::Active
    }

    async fn subscribe_events(&self) -> broadcast::Receiver<DeviceEvent> {
        self.event_tx.subscribe()
    }
}
```

---

## 2. Implement Specific Capabilities

Depending on what the hardware can do, implement one or more capability traits:

### Case A: Wireless Peripheral with Dongle (Presence Detection)
If your device uses a persistent USB dongle (e.g. Logitech G PRO X, Razer BlackShark, Corsair Virtuoso), implement `HasPresenceDetection`:

```rust
use sonomux_core::traits::HasPresenceDetection;

#[async_trait]
impl HasPresenceDetection for MyAudioDevice {
    async fn is_physically_active(&self) -> bool {
        // Query battery or ping the headset over HID.
        // If the query times out or returns BATTERY_UNAVAILABLE, return false.
        true
    }

    async fn battery_level(&self) -> Option<u8> {
        Some(85)
    }

    async fn is_charging(&self) -> Option<bool> {
        Some(false)
    }
}
```

### Case B: Hardware Mixer (Routing Matrix)
If your device has an internal cross-point mixer (e.g. TC-Helicon GoXLR, Rodecaster Pro, Elgato Wave XLR):

```rust
use sonomux_core::traits::HasRoutingMatrix;

#[async_trait]
impl HasRoutingMatrix for MyAudioDevice {
    async fn available_inputs(&self) -> Vec<String> {
        vec!["Mic".into(), "Chat".into(), "Music".into(), "Game".into()]
    }

    async fn available_outputs(&self) -> Vec<String> {
        vec!["Headphones".into(), "Broadcast".into(), "Sampler".into()]
    }

    async fn is_routed(&self, input: &str, output: &str) -> Result<bool, DeviceError> {
        // Check hardware state
        Ok(true)
    }

    async fn set_route(&mut self, input: &str, output: &str, enabled: bool) -> Result<(), DeviceError> {
        // Send routing command to hardware
        Ok(())
    }
}
```

### Case C: Hardware Mixer (Volume & Mute)
If the hardware controls DAC registers, headphone amplifiers, or mixer gains:

```rust
use sonomux_core::traits::HasHardwareVolume;

#[async_trait]
impl HasHardwareVolume for MyAudioDevice {
    async fn get_channel_volume(&self, channel: &str) -> Result<f32, DeviceError> {
        // Return 0.0 - 1.0 normalized level
        Ok(0.8)
    }

    async fn set_channel_volume(&mut self, channel: &str, level: f32) -> Result<(), DeviceError> {
        // Translate normalized level to device range (e.g. 0-255)
        Ok(())
    }

    async fn set_channel_mute(&mut self, channel: &str, muted: bool) -> Result<(), DeviceError> {
        Ok(())
    }
}
```

---

## 3. Declarative / Code-less Profiles (Coming Soon)

For devices that use standard USB HID or ALSA mixer controls, Sonomux is developing a declarative JSON/TOML format:

```toml
[device]
id = "my-device-id"
name = "My Audio Interface"
vendor_id = 0x1234
product_id = 0x5678

[presence]
method = "hid_battery"
report_id = 0x06
standby_value = 0xFF

[channels]
inputs = ["Mic", "LineIn"]
outputs = ["Main", "Headphones"]
```

If you have a device you'd like supported, feel free to open an issue or pull request with the device's USB VID/PID and ALSA profile!
