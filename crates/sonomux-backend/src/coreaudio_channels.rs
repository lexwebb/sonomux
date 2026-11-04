//! Per-pair CoreAudio devices for multichannel hardware.
//!
//! macOS shows a mixer like the GoXLR Mini as one device with many channels (10 out, 21 in).
//! Apps that pick a device expect stereo, so every named stereo pair on such a device gets its
//! own aggregate device wrapping the hardware, with its preferred stereo pair set to that pair.
//!
//! Pairs come from the hardware's own channel names: consecutive channels named "X L" / "X R"
//! become a device "<device> X". Devices with unnamed channels, surround layouts (no L/R pairs),
//! fewer than two pairs, and virtual or aggregate devices are left alone.

use crate::coreaudio::{
    get, get_string_el, set, CoreAudioManager, DEV_PREFERRED_STEREO, SCOPE_GLOBAL, SCOPE_INPUT,
    SCOPE_OUTPUT,
};
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

const UID_PREFIX: &str = "org.sonomux.split::";
const ELEMENT_NAME: u32 = u32::from_be_bytes(*b"lchn");
const TRANSPORT: u32 = u32::from_be_bytes(*b"tran");
const TRANSPORT_AGGREGATE: u32 = u32::from_be_bytes(*b"grup");
const TRANSPORT_VIRTUAL: u32 = u32::from_be_bytes(*b"virt");
/// Upper bound when probing channel names.
const MAX_CHANNELS: u32 = 64;

/// A stereo pair to expose as its own device.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    /// Display name, e.g. "GoXLRMini Chat".
    pub name: String,
    /// Stable UID derived from the hardware UID, direction and first channel.
    pub uid: String,
    /// First channel, 1-based.
    pub first: u32,
    pub input: bool,
}

/// Remove the L/R marker from a channel name: "Chat L" -> "Chat", "Mic L (processed)" ->
/// "Mic (processed)". Returns None if the name has no standalone `side` word.
fn stem(name: &str, side: &str) -> Option<String> {
    let words: Vec<&str> = name.split(' ').collect();
    let i = words.iter().rposition(|w| *w == side)?;
    let rest: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, w)| *w)
        .collect();
    Some(rest.join(" "))
}

/// Find stereo pairs in a list of channel names (index 0 = channel 1).
/// Only (odd, even) neighbours named "X L" / "X R" count.
pub fn find_pairs(names: &[String]) -> Vec<(String, u32)> {
    names
        .chunks(2)
        .enumerate()
        .filter_map(|(i, c)| match c {
            [l, r] => {
                let s = stem(l, "L")?;
                (stem(r, "R")? == s && !s.is_empty()).then(|| (s, i as u32 * 2 + 1))
            }
            _ => None,
        })
        .collect()
}

/// Plan the per-pair devices for one physical device from its channel names.
/// Pure, so the naming rules are testable without hardware.
pub fn plan_pairs(
    device_name: &str,
    device_uid: &str,
    outs: &[String],
    ins: &[String],
) -> Vec<Pair> {
    let out_pairs = find_pairs(outs);
    let in_pairs = find_pairs(ins);
    // A single pair is already a stereo device; nothing to split.
    let worth = |p: &Vec<(String, u32)>| p.len() >= 2;

    // Drop a vendor prefix the hardware repeats, e.g. "GoXLR-Music" on device "GoXLRMini".
    let tidy = |s: &str| -> String {
        match s.split_once('-') {
            Some((pre, rest)) if !pre.is_empty() && device_name.starts_with(pre) => rest.into(),
            _ => s.into(),
        }
    };
    let out_names: Vec<String> = out_pairs.iter().map(|(s, _)| tidy(s)).collect();

    let mut plan = Vec::new();
    if worth(&out_pairs) {
        for ((_, first), label) in out_pairs.iter().zip(&out_names) {
            plan.push(Pair {
                name: format!("{} {}", device_name, label),
                uid: format!("{}o{}::{}", UID_PREFIX, first, device_uid),
                first: *first,
                input: false,
            });
        }
    }
    if worth(&in_pairs) {
        for (s, first) in &in_pairs {
            let mut label = tidy(s);
            // Every pair device carries both directions, so names must not collide.
            if out_names.contains(&label) {
                label.push_str(" In");
            }
            plan.push(Pair {
                name: format!("{} {}", device_name, label),
                uid: format!("{}i{}::{}", UID_PREFIX, first, device_uid),
                first: *first,
                input: true,
            });
        }
    }
    plan
}

fn channel_names(id: u32, scope: u32) -> Vec<String> {
    (1..=MAX_CHANNELS)
        .map_while(|el| get_string_el(id, ELEMENT_NAME, scope, el).filter(|n| !n.is_empty()))
        .collect()
}

/// Create any missing per-pair devices for every connected physical multichannel device.
/// Returns the names created. Existing devices (matched by UID) are left untouched.
pub fn ensure_pair_devices() -> Vec<String> {
    let devices = CoreAudioManager::devices();
    let mut created = Vec::new();
    for hw in &devices {
        let transport = get::<u32>(hw.id, TRANSPORT, SCOPE_GLOBAL).unwrap_or(0);
        if transport == TRANSPORT_AGGREGATE || transport == TRANSPORT_VIRTUAL {
            continue;
        }
        let plan = plan_pairs(
            &hw.name,
            &hw.uid,
            &channel_names(hw.id, SCOPE_OUTPUT),
            &channel_names(hw.id, SCOPE_INPUT),
        );
        for pair in plan {
            if devices.iter().any(|d| d.uid == pair.uid) {
                continue;
            }
            match create(&hw.uid, &pair) {
                Ok(()) => {
                    info!(
                        "Created CoreAudio device '{}' ({} ch {}/{})",
                        pair.name,
                        if pair.input { "in" } else { "out" },
                        pair.first,
                        pair.first + 1
                    );
                    created.push(pair.name);
                }
                Err(e) => warn!("Could not create '{}': {}", pair.name, e),
            }
        }
    }
    created
}

fn create(hw_uid: &str, pair: &Pair) -> Result<(), BackendError> {
    let hw = NSString::from_str(hw_uid);
    let name = NSString::from_str(&pair.name);
    let uid = NSString::from_str(&pair.uid);
    let subs = NSArray::from_retained_slice(&[dict(&[(kAudioSubDeviceUIDKey, obj(&hw))])]);
    let desc = dict(&[
        (kAudioAggregateDeviceNameKey, obj(&name)),
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
    let scope = if pair.input {
        SCOPE_INPUT
    } else {
        SCOPE_OUTPUT
    };
    check(
        set(
            id,
            DEV_PREFERRED_STEREO,
            scope,
            &[pair.first, pair.first + 1],
        ),
        "set preferred stereo pair",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn goxlr_mini_layout() {
        let outs = names(&[
            "System L",
            "System R",
            "Game L",
            "Game R",
            "Chat L",
            "Chat R",
            "Music L",
            "Music R",
            "Sample Source Mix L",
            "Sample Source Mix R",
        ]);
        let ins = names(&[
            "Broadcast Stream Mix L",
            "Broadcast Stream Mix R",
            "Chat Mic Mix L",
            "Chat Mic Mix R",
            "Sample Source Mix L",
            "Sample Source Mix R",
            "GoXLR-Mic L (processed)",
            "GoXLR-Mic R (processed)",
            "GoXLR-System L",
            "GoXLR-System R",
            "GoXLR-Dry Mic (unprocessed)",
        ]);
        let plan = plan_pairs("GoXLRMini", "HW", &outs, &ins);
        let got: Vec<(&str, u32, bool)> = plan
            .iter()
            .map(|p| (p.name.as_str(), p.first, p.input))
            .collect();
        assert_eq!(
            got,
            [
                ("GoXLRMini System", 1, false),
                ("GoXLRMini Game", 3, false),
                ("GoXLRMini Chat", 5, false),
                ("GoXLRMini Music", 7, false),
                ("GoXLRMini Sample Source Mix", 9, false),
                ("GoXLRMini Broadcast Stream Mix", 1, true),
                ("GoXLRMini Chat Mic Mix", 3, true),
                ("GoXLRMini Sample Source Mix In", 5, true),
                ("GoXLRMini Mic (processed)", 7, true),
                ("GoXLRMini System In", 9, true),
            ]
        );
        assert_eq!(plan[2].uid, "org.sonomux.split::o5::HW");
        assert_eq!(plan[6].uid, "org.sonomux.split::i3::HW");
    }

    #[test]
    fn leaves_stereo_and_surround_alone() {
        // Plain stereo: one pair, nothing to split.
        assert!(plan_pairs("Speakers", "U", &names(&["Left", "Right"]), &[]).is_empty());
        assert!(plan_pairs("Amp", "U", &names(&["Out L", "Out R"]), &[]).is_empty());
        // Surround: no "X L"/"X R" pairs.
        let surround = names(&["Front Left", "Front Right", "Center", "LFE"]);
        assert!(plan_pairs("HDMI", "U", &surround, &[]).is_empty());
        // Misaligned pairs (R before L) don't count.
        assert!(find_pairs(&names(&["A R", "A L", "B R", "B L"])).is_empty());
    }
}
