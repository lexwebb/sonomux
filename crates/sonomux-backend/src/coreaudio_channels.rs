//! Per-function CoreAudio devices for multichannel hardware.
//!
//! macOS shows a mixer like the GoXLR Mini as one device with many channels (10 out, 21 in).
//! Apps that pick a device expect stereo, so sonomuxd splits such hardware into aggregate
//! devices, one per named stereo pair, each with its preferred stereo pairs set.
//!
//! An aggregate always carries every input and output channel of the hardware, so each device
//! shows up in both the Output and Input lists. To make both directions meaningful, each output
//! pair is joined with its matching input pair by name:
//!   1. an input whose name starts with the output's name ("Chat" -> "Chat Mic Mix"), else
//!   2. an input with exactly the same name ("System" -> the "System" return).
//!
//! Inputs with no matching output are only created when `include_unpaired_inputs` is set,
//! since they would sit in the Output list playing to channels 1/2.
//!
//! Pairs come from the hardware's own channel names: consecutive channels named "X L" / "X R".
//! Devices with unnamed channels, surround layouts (no L/R pairs), a single pair, and virtual or
//! aggregate devices are left alone.

use crate::coreaudio::{
    get, get_string, get_string_el, set, CoreAudioManager, DEV_PREFERRED_STEREO, SCOPE_GLOBAL,
    SCOPE_INPUT, SCOPE_OUTPUT,
};
use crate::coreaudio_tap::{check, dict, obj};
use crate::BackendError;
use objc2_core_audio::{
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceUIDKey, kAudioSubDeviceUIDKey,
    AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
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
const MAIN_SUB_DEVICE: u32 = u32::from_be_bytes(*b"amst");
/// Upper bound when probing channel names.
const MAX_CHANNELS: u32 = 64;

/// One device to expose: an output pair, an input pair, or both.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// Display name, e.g. "GoXLRMini Chat".
    pub name: String,
    /// Stable UID derived from the hardware UID and the channel pairs.
    pub uid: String,
    /// First output channel, 1-based.
    pub output: Option<u32>,
    /// First input channel, 1-based.
    pub input: Option<u32>,
    /// Hardware names of the pairs, e.g. "Chat" / "Chat Mic Mix".
    pub output_label: Option<String>,
    pub input_label: Option<String>,
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

/// Plan the devices for one physical device from its channel names.
/// Pure, so the pairing and naming rules are testable without hardware.
pub fn plan(
    device_name: &str,
    device_uid: &str,
    outs: &[String],
    ins: &[String],
    include_unpaired_inputs: bool,
) -> Vec<Split> {
    // Drop a vendor prefix the hardware repeats, e.g. "GoXLR-Music" on device "GoXLRMini".
    let tidy = |(s, first): (String, u32)| -> (String, u32) {
        match s.split_once('-') {
            Some((pre, rest)) if !pre.is_empty() && device_name.starts_with(pre) => {
                (rest.to_string(), first)
            }
            _ => (s, first),
        }
    };
    let out_pairs: Vec<_> = find_pairs(outs).into_iter().map(tidy).collect();
    let mut in_pairs: Vec<_> = find_pairs(ins).into_iter().map(tidy).collect();
    // A single pair in total is already a stereo device; nothing to split.
    if out_pairs.len() + in_pairs.len() < 2 {
        return Vec::new();
    }

    let mut splits = Vec::new();
    for (label, out_first) in &out_pairs {
        let prefix = format!("{} ", label);
        let pos = in_pairs
            .iter()
            .position(|(n, _)| n.starts_with(&prefix))
            .or_else(|| in_pairs.iter().position(|(n, _)| n == label));
        let (input, input_label) = match pos.map(|i| in_pairs.remove(i)) {
            Some((l, f)) => (Some(f), Some(l)),
            None => (None, None),
        };
        splits.push(Split {
            name: format!("{} {}", device_name, label),
            uid: match input {
                Some(i) => format!("{}o{}i{}::{}", UID_PREFIX, out_first, i, device_uid),
                None => format!("{}o{}::{}", UID_PREFIX, out_first, device_uid),
            },
            output: Some(*out_first),
            input,
            output_label: Some(label.clone()),
            input_label,
        });
    }
    if include_unpaired_inputs {
        for (label, in_first) in in_pairs {
            let label: String = label;
            let mut name = format!("{} {}", device_name, label);
            if splits.iter().any(|s| s.name == name) {
                name.push_str(" In");
            }
            splits.push(Split {
                name,
                uid: format!("{}i{}::{}", UID_PREFIX, in_first, device_uid),
                output: None,
                input: Some(in_first),
                output_label: None,
                input_label: Some(label),
            });
        }
    }
    splits
}

fn channel_names(id: u32, scope: u32) -> Vec<String> {
    (1..=MAX_CHANNELS)
        .map_while(|el| get_string_el(id, ELEMENT_NAME, scope, el).filter(|n| !n.is_empty()))
        .collect()
}

/// Bring the split devices for every connected physical multichannel device in line with the
/// plan: create missing ones and remove our own that no longer match (e.g. after a rule or
/// config change). Returns the names created.
pub fn ensure_split_devices(include_unpaired_inputs: bool) -> Vec<String> {
    let devices = CoreAudioManager::devices();
    let mut created = Vec::new();
    for hw in &devices {
        let transport = get::<u32>(hw.id, TRANSPORT, SCOPE_GLOBAL).unwrap_or(0);
        if transport == TRANSPORT_AGGREGATE || transport == TRANSPORT_VIRTUAL {
            continue;
        }
        let planned = plan(
            &hw.name,
            &hw.uid,
            &channel_names(hw.id, SCOPE_OUTPUT),
            &channel_names(hw.id, SCOPE_INPUT),
            include_unpaired_inputs,
        );

        // Ours for this hardware but no longer planned: remove.
        for d in &devices {
            let ours = d.uid.starts_with(UID_PREFIX) && d.uid.ends_with(&format!("::{}", hw.uid));
            if ours && !planned.iter().any(|s| s.uid == d.uid) {
                let st = unsafe { AudioHardwareDestroyAggregateDevice(d.id) };
                info!(
                    "Removed stale CoreAudio device '{}' (status {})",
                    d.name, st
                );
            }
        }

        for split in planned {
            if devices.iter().any(|d| d.uid == split.uid) {
                continue;
            }
            match create(&hw.uid, &split) {
                Ok(()) => {
                    info!(
                        "Created CoreAudio device '{}' (out {:?}, in {:?})",
                        split.name, split.output, split.input
                    );
                    created.push(split.name);
                }
                Err(e) => warn!("Could not create '{}': {}", split.name, e),
            }
        }
    }
    created
}

/// Per split hardware its planned devices; then every other physical device with why it is
/// not split.
pub type Description = (Vec<(String, Vec<Split>)>, Vec<(String, String)>);

/// What the Devices screen shows (see [`Description`]).
pub fn describe(include_unpaired_inputs: bool) -> Description {
    let mut split = Vec::new();
    let mut other = Vec::new();
    for hw in CoreAudioManager::devices() {
        let transport = get::<u32>(hw.id, TRANSPORT, SCOPE_GLOBAL).unwrap_or(0);
        if transport == TRANSPORT_AGGREGATE || transport == TRANSPORT_VIRTUAL {
            continue;
        }
        let outs = channel_names(hw.id, SCOPE_OUTPUT);
        let ins = channel_names(hw.id, SCOPE_INPUT);
        let planned = plan(&hw.name, &hw.uid, &outs, &ins, include_unpaired_inputs);
        if !planned.is_empty() {
            split.push((hw.name, planned));
        } else {
            let channels = CoreAudioManager::channel_count(hw.id, false)
                .max(CoreAudioManager::channel_count(hw.id, true));
            let why = if channels <= 2 {
                "Stereo · not split"
            } else if outs.is_empty() && ins.is_empty() {
                "Channels not named · not split"
            } else {
                "No stereo pairs · not split"
            };
            other.push((hw.name, why.to_string()));
        }
    }
    (split, other)
}

/// The hardware device and first output channel behind one of our split devices, if it is one.
/// Lets routing write to the physical device directly (macOS can't nest aggregates).
pub fn resolve_output(device_id: u32) -> Option<(String, u32)> {
    let uid = get_string(device_id, u32::from_be_bytes(*b"uid "))?;
    let rest = uid.strip_prefix(UID_PREFIX)?;
    let (pairs, hw_uid) = rest.split_once("::")?;
    let first: u32 = pairs.strip_prefix('o')?.split('i').next()?.parse().ok()?;
    // Prefer the live main sub-device; fall back to the UID's record of it.
    let main = get_string(device_id, MAIN_SUB_DEVICE).unwrap_or_else(|| hw_uid.to_string());
    Some((main, first))
}

fn create(hw_uid: &str, split: &Split) -> Result<(), BackendError> {
    let hw = NSString::from_str(hw_uid);
    let name = NSString::from_str(&split.name);
    let uid = NSString::from_str(&split.uid);
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
    for (first, scope) in [(split.output, SCOPE_OUTPUT), (split.input, SCOPE_INPUT)] {
        if let Some(f) = first {
            check(
                set(id, DEV_PREFERRED_STEREO, scope, &[f, f + 1]),
                "set preferred stereo pair",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn goxlr() -> (Vec<String>, Vec<String>) {
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
            "GoXLR-Game L",
            "GoXLR-Game R",
            "GoXLR-Chat L",
            "GoXLR-Chat R",
            "GoXLR-Music L",
            "GoXLR-Music R",
            "GoXLR-Line In L",
            "GoXLR-Line In R",
            "GoXLR-Dry Mic (unprocessed)",
        ]);
        (outs, ins)
    }

    #[test]
    fn goxlr_mini_joins_outputs_with_matching_inputs() {
        let (outs, ins) = goxlr();
        let p = plan("GoXLRMini", "HW", &outs, &ins, false);
        let got: Vec<(&str, Option<u32>, Option<u32>)> = p
            .iter()
            .map(|s| (s.name.as_str(), s.output, s.input))
            .collect();
        assert_eq!(
            got,
            [
                ("GoXLRMini System", Some(1), Some(9)), // System return
                ("GoXLRMini Game", Some(3), Some(11)),  // Game return
                ("GoXLRMini Chat", Some(5), Some(3)),   // Chat Mic Mix beats the Chat return
                ("GoXLRMini Music", Some(7), Some(15)), // Music return
                ("GoXLRMini Sample Source Mix", Some(9), Some(5)),
            ]
        );
        assert_eq!(p[2].uid, "org.sonomux.split::o5i3::HW");
    }

    #[test]
    fn unpaired_inputs_only_when_asked() {
        let (outs, ins) = goxlr();
        let p = plan("GoXLRMini", "HW", &outs, &ins, true);
        let extra: Vec<&str> = p[5..].iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            extra,
            [
                "GoXLRMini Broadcast Stream Mix",
                "GoXLRMini Mic (processed)",
                "GoXLRMini Chat In",
                "GoXLRMini Line In",
            ]
        );
        assert_eq!(p[5].output, None);
        assert_eq!(p[5].uid, "org.sonomux.split::i1::HW");
    }

    #[test]
    fn leaves_stereo_and_surround_alone() {
        assert!(plan("Speakers", "U", &names(&["Left", "Right"]), &[], true).is_empty());
        assert!(plan("Amp", "U", &names(&["Out L", "Out R"]), &[], true).is_empty());
        let surround = names(&["Front Left", "Front Right", "Center", "LFE"]);
        assert!(plan("HDMI", "U", &surround, &[], true).is_empty());
        assert!(find_pairs(&names(&["A R", "A L", "B R", "B L"])).is_empty());
    }

    #[test]
    fn output_only_interface_splits_outputs() {
        let outs = names(&["Main L", "Main R", "Phones L", "Phones R"]);
        let p = plan("Interface", "U", &outs, &[], false);
        assert_eq!(p.len(), 2);
        assert_eq!((p[1].name.as_str(), p[1].input), ("Interface Phones", None));
    }
}
