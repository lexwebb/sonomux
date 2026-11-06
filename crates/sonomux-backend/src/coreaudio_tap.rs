//! Per-app routing on macOS via Core Audio process taps (macOS 14.2+).
//!
//! An `AppRoute` taps a set of processes on the hardware device they play to (muting their
//! normal output while tapped), wraps the tap and the target hardware in a private aggregate
//! device, and copies the tapped audio into one channel pair of the target from an IOProc.
//!
//! The tap reads the source device's stream at full width. An app writes to one stereo pair of
//! it (e.g. 5/6 when it chose "GoXLRMini Chat"), so summing every left and every right channel
//! recovers its audio at exact level whichever pair it used. (A stereo-mixdown tap would
//! average all pairs instead, playing a 10-channel device's app at 1/5 volume.)
//!
//! The device must be a real device, not an aggregate (macOS can't nest aggregates), so for the
//! GoXLR this targets "GoXLRMini" directly with a channel offset (System 0, Game 2, Chat 4, ...).

use crate::BackendError;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioSubDeviceUIDKey, kAudioSubTapDriftCompensationKey,
    kAudioSubTapUIDKey, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectID, CATapDescription, CATapMuteBehavior,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::{c_void, CStr};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

/// Shared with the real-time IOProc. Only atomics: no locks or allocation on the audio thread.
struct RouteState {
    /// First output channel (0-based) of the target stereo pair.
    first_channel: usize,
    /// Frames copied, for diagnostics.
    frames: AtomicU64,
    /// Peak of the tapped signal since last read, as f32 bits.
    peak: AtomicU64,
}

/// A live tap-and-route. Dropping it restores the app's normal output.
pub struct AppRoute {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    state: *mut RouteState,
}

// The raw pointer is owned by this struct and only freed in Drop.
unsafe impl Send for AppRoute {}

pub(crate) fn check(st: i32, what: &str) -> Result<(), BackendError> {
    if st == 0 {
        Ok(())
    } else {
        Err(BackendError::Routing(format!(
            "{} failed (OSStatus {})",
            what, st
        )))
    }
}

pub(crate) fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap())
}

pub(crate) fn obj<T: objc2::Message>(r: &Retained<T>) -> &AnyObject {
    // Every Objective-C object is an AnyObject.
    unsafe { &*(Retained::as_ptr(r) as *const AnyObject) }
}

pub(crate) fn dict(pairs: &[(&CStr, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| key(k)).collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let vals: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&key_refs, &vals)
}

impl AppRoute {
    /// Frames processed so far. Stops growing once the HAL stops the IOProc, e.g. when its
    /// hardware went away during sleep.
    pub fn frames(&self) -> u64 {
        unsafe { &*self.state }.frames.load(Ordering::Relaxed)
    }

    /// Tap `processes` (HAL process object IDs) and play them on `device_uid` starting at
    /// output channel `first_channel` (0-based).
    /// Tap `processes` (HAL process object IDs) on hardware device `source_uid` and play them
    /// on hardware device `device_uid` starting at output channel `first_channel` (0-based).
    pub fn start(
        processes: &[AudioObjectID],
        source_uid: &str,
        device_uid: &str,
        first_channel: usize,
    ) -> Result<Self, BackendError> {
        if processes.is_empty() {
            return Err(BackendError::NotFound("no audio processes to tap".into()));
        }

        // 1. Tap: stereo mixdown of the processes, silenced at their normal output while tapped.
        let ids: Vec<Retained<NSNumber>> =
            processes.iter().map(|&p| NSNumber::new_u32(p)).collect();
        let desc = unsafe {
            CATapDescription::initWithProcesses_andDeviceUID_withStream(
                CATapDescription::alloc(),
                &NSArray::from_retained_slice(&ids),
                &NSString::from_str(source_uid),
                0,
            )
        };
        unsafe {
            desc.setPrivate(true);
            desc.setMuteBehavior(CATapMuteBehavior::MutedWhenTapped);
            desc.setName(&NSString::from_str("sonomux route"));
        }
        let tap_uuid = unsafe { desc.UUID().UUIDString() };
        let mut tap: AudioObjectID = 0;
        check(
            unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) },
            "AudioHardwareCreateProcessTap",
        )?;

        // 2. Private aggregate: the target device plus the tap, drift-compensated to its clock.
        let dev_uid = NSString::from_str(device_uid);
        let agg_uid = NSString::from_str(&format!("org.sonomux.route.{}", tap_uuid));
        let sub_dev = dict(&[(kAudioSubDeviceUIDKey, obj(&dev_uid))]);
        let yes = NSNumber::new_bool(true);
        let sub_tap = dict(&[
            (kAudioSubTapUIDKey, obj(&tap_uuid)),
            (kAudioSubTapDriftCompensationKey, obj(&yes)),
        ]);
        let name = NSString::from_str("Sonomux Route");
        let devices = NSArray::from_retained_slice(&[sub_dev]);
        let taps = NSArray::from_retained_slice(&[sub_tap]);
        let desc_dict = dict(&[
            (kAudioAggregateDeviceNameKey, obj(&name)),
            (kAudioAggregateDeviceUIDKey, obj(&agg_uid)),
            (kAudioAggregateDeviceIsPrivateKey, obj(&yes)),
            (kAudioAggregateDeviceMainSubDeviceKey, obj(&dev_uid)),
            (kAudioAggregateDeviceSubDeviceListKey, obj(&devices)),
            (kAudioAggregateDeviceTapListKey, obj(&taps)),
            (kAudioAggregateDeviceTapAutoStartKey, obj(&yes)),
        ]);
        let cf = unsafe { &*(Retained::as_ptr(&desc_dict) as *const CFDictionary) };
        let mut aggregate: AudioObjectID = 0;
        if let Err(e) = check(
            unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut aggregate)) },
            "AudioHardwareCreateAggregateDevice",
        ) {
            unsafe { AudioHardwareDestroyProcessTap(tap) };
            return Err(e);
        }

        // 3. IOProc copying tap input to the target channel pair.
        let state = Box::into_raw(Box::new(RouteState {
            first_channel,
            frames: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }));
        // From here on, Drop cleans up whatever has been created.
        let mut route = AppRoute {
            tap,
            aggregate,
            proc_id: None,
            state,
        };
        let mut proc_id: AudioDeviceIOProcID = None;
        check(
            unsafe {
                AudioDeviceCreateIOProcID(
                    aggregate,
                    Some(io_proc),
                    state as *mut c_void,
                    NonNull::from(&mut proc_id),
                )
            },
            "AudioDeviceCreateIOProcID",
        )?;
        route.proc_id = proc_id;
        tap_input_only(aggregate, proc_id)?;
        check(
            unsafe { AudioDeviceStart(aggregate, proc_id) },
            "AudioDeviceStart",
        )?;
        Ok(route)
    }

    /// Frames copied so far and the tapped peak since the last call (0.0-1.0).
    pub fn stats(&self) -> (u64, f32) {
        let st = unsafe { &*self.state };
        let peak = f32::from_bits(st.peak.swap(0, Ordering::Relaxed) as u32);
        (st.frames.load(Ordering::Relaxed), peak)
    }
}

impl Drop for AppRoute {
    fn drop(&mut self) {
        unsafe {
            if self.proc_id.is_some() {
                AudioDeviceStop(self.aggregate, self.proc_id);
                AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
            }
            AudioHardwareDestroyAggregateDevice(self.aggregate);
            AudioHardwareDestroyProcessTap(self.tap);
            // The IOProc is stopped and destroyed, so nothing else holds the state.
            drop(Box::from_raw(self.state));
        }
    }
}

fn buffers(list: &AudioBufferList) -> &[AudioBuffer] {
    unsafe { std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize) }
}

fn buffers_mut(list: &mut AudioBufferList) -> &mut [AudioBuffer] {
    unsafe {
        std::slice::from_raw_parts_mut(list.mBuffers.as_mut_ptr(), list.mNumberBuffers as usize)
    }
}

/// Switch off every input stream of the aggregate except the tap (the last one), so the route
/// never opens the device's microphone. Without this, macOS shows sonomuxd using the mic and
/// the device runs input IO for us next to apps in a call.
fn tap_input_only(
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
) -> Result<(), BackendError> {
    use crate::coreaudio::{data_size, DEV_STREAMS, SCOPE_INPUT};
    use objc2_core_audio::{kAudioDevicePropertyIOProcStreamUsage, AudioObjectPropertyAddress};
    let n = data_size(aggregate, DEV_STREAMS, SCOPE_INPUT).unwrap_or(0) as usize / 4;
    if n < 2 {
        return Ok(()); // only the tap: nothing to switch off
    }
    // AudioHardwareIOProcStreamUsage: { void *mIOProc; UInt32 mNumberStreams; UInt32 mStreamIsOn[n]; }
    let size = 12 + 4 * n;
    let mut buf = vec![0u64; size.div_ceil(8)];
    let bytes = buf.as_mut_ptr() as *mut u8;
    let proc_ptr = proc_id.map_or(std::ptr::null(), |f| f as *const c_void);
    unsafe {
        (bytes as *mut *const c_void).write(proc_ptr);
        (bytes.add(8) as *mut u32).write(n as u32);
        (bytes.add(12 + 4 * (n - 1)) as *mut u32).write(1);
    }
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyIOProcStreamUsage,
        mScope: SCOPE_INPUT,
        mElement: 0,
    };
    check(
        unsafe {
            objc2_core_audio::AudioObjectSetPropertyData(
                aggregate,
                NonNull::from(&addr),
                0,
                std::ptr::null(),
                size as u32,
                NonNull::new_unchecked(bytes.cast()),
            )
        },
        "IOProcStreamUsage",
    )
}

/// Real-time thread: no allocation, no locks, no logging.
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    let st = &*(client as *const RouteState);
    let ins = buffers(input.as_ref());
    let outs = buffers_mut(&mut *output.as_ptr());

    // The tap's stream comes after the device's own input streams: it is the last buffer.
    let Some(tap) = ins.last() else { return 0 };
    let tap_ch = tap.mNumberChannels.max(1) as usize;
    let tap_frames = tap.mDataByteSize as usize / 4 / tap_ch;
    let tap_data: &[f32] = if tap.mData.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(tap.mData as *const f32, tap_frames * tap_ch)
    };

    let mut peak = 0f32;
    let mut channel_base = 0usize;
    for buf in outs.iter_mut() {
        let ch = buf.mNumberChannels.max(1) as usize;
        let frames = buf.mDataByteSize as usize / 4 / ch;
        if buf.mData.is_null() {
            channel_base += ch;
            continue;
        }
        let out = std::slice::from_raw_parts_mut(buf.mData as *mut f32, frames * ch);
        peak = peak.max(crate::routing_logic::mix_pair(
            out,
            ch,
            channel_base,
            st.first_channel,
            tap_data,
            tap_ch,
        ));
        channel_base += ch;
    }

    st.frames.fetch_add(tap_frames as u64, Ordering::Relaxed);
    let prev = f32::from_bits(st.peak.load(Ordering::Relaxed) as u32);
    if peak > prev {
        st.peak.store(peak.to_bits() as u64, Ordering::Relaxed);
    }
    0
}
