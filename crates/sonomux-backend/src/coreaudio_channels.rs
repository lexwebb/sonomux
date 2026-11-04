//! Per-channel CoreAudio devices for a multichannel mixer.
//!
//! macOS shows the GoXLR Mini as one device ("GoXLRMini") with 10 output and 21 input channels.
//! Apps that pick a device expect stereo, so each GoXLR channel gets its own aggregate device
//! ("GoXLR Chat" etc.) wrapping the GoXLR, with its preferred stereo pair set to that channel.
//! GoXLR Utility's own Aggregate Manager does the same but fails on macOS 26 (goxlr-utility #226).

use crate::coreaudio::{set, CoreAudioManager, DEV_PREFERRED_STEREO, SCOPE_INPUT, SCOPE_OUTPUT};
use crate::coreaudio_tap::{check, dict, obj};
use crate::BackendError;
use objc2_core_audio::{
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceUIDKey, kAudioSubDeviceUIDKey,
    AudioHardwareCreateAggregateDevice,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSString};
use std::ptr::NonNull;
use tracing::{info, warn};

/// One per-channel device: name suffix, first channel (1-based), and whether it is an input.
pub struct ChannelDevice {
    pub name: &'static str,
    pub first: u32,
    pub input: bool,
}

/// GoXLR Mini layout on macOS. Outputs 1-10, inputs 1-6 (probed on hardware).
// ponytail: Mini only; the full-size GoXLR needs its own table when someone has one to probe.
pub const GOXLR_MINI: &[ChannelDevice] = &[
    ChannelDevice {
        name: "System",
        first: 1,
        input: false,
    },
    ChannelDevice {
        name: "Game",
        first: 3,
        input: false,
    },
    ChannelDevice {
        name: "Chat",
        first: 5,
        input: false,
    },
    ChannelDevice {
        name: "Music",
        first: 7,
        input: false,
    },
    ChannelDevice {
        name: "Sample",
        first: 9,
        input: false,
    },
    ChannelDevice {
        name: "Stream Mix",
        first: 1,
        input: true,
    },
    ChannelDevice {
        name: "Chat Mic",
        first: 3,
        input: true,
    },
    ChannelDevice {
        name: "Sampler",
        first: 5,
        input: true,
    },
];

pub const UID_PREFIX: &str = "org.sonomux.goxlr::";

/// Display name for a channel device, e.g. "GoXLR Chat".
pub fn device_name(ch: &ChannelDevice) -> String {
    format!("GoXLR {}", ch.name)
}

/// Create any missing per-channel devices for `physical` (e.g. "GoXLRMini").
/// Devices are matched by name, so ones made by older tools are kept as they are.
/// Returns the names created. Errors if the physical device is not connected.
pub fn ensure_channel_devices(
    physical: &str,
    layout: &[ChannelDevice],
) -> Result<Vec<String>, BackendError> {
    let devices = CoreAudioManager::devices();
    let hw = devices
        .iter()
        .find(|d| d.name == physical)
        .ok_or_else(|| BackendError::NotFound(format!("{} not connected", physical)))?;

    let mut created = Vec::new();
    for ch in layout {
        let name = device_name(ch);
        if devices.iter().any(|d| d.name == name) {
            continue;
        }
        match create(&hw.uid, &name, ch) {
            Ok(()) => {
                info!(
                    "Created CoreAudio device '{}' (ch {}/{})",
                    name,
                    ch.first,
                    ch.first + 1
                );
                created.push(name);
            }
            Err(e) => warn!("Could not create '{}': {}", name, e),
        }
    }
    Ok(created)
}

fn create(hw_uid: &str, name: &str, ch: &ChannelDevice) -> Result<(), BackendError> {
    let hw = NSString::from_str(hw_uid);
    let ns_name = NSString::from_str(name);
    let uid = NSString::from_str(&format!("{}{}", UID_PREFIX, ch.name.replace(' ', "")));
    let subs = NSArray::from_retained_slice(&[dict(&[(kAudioSubDeviceUIDKey, obj(&hw))])]);
    let desc = dict(&[
        (kAudioAggregateDeviceNameKey, obj(&ns_name)),
        (kAudioAggregateDeviceUIDKey, obj(&uid)),
        (kAudioAggregateDeviceSubDeviceListKey, obj(&subs)),
        (kAudioAggregateDeviceMainSubDeviceKey, obj(&hw)),
    ]);
    let cf = unsafe { &*(objc2::rc::Retained::as_ptr(&desc) as *const CFDictionary) };
    let mut id = 0u32;
    check(
        unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut id)) },
        "AudioHardwareCreateAggregateDevice",
    )?;
    let scope = if ch.input { SCOPE_INPUT } else { SCOPE_OUTPUT };
    check(
        set(id, DEV_PREFERRED_STEREO, scope, &[ch.first, ch.first + 1]),
        "set preferred stereo pair",
    )
}
