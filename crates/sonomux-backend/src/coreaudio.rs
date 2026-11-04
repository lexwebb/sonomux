//! macOS CoreAudio backend.
//!
//! Mirrors the `PulseManager` API so callers can use `sonomux_backend::Backend` on either OS.
//! Device and process queries are real. Per-app routing and loopbacks are not implemented yet:
//! they need Core Audio process taps (macOS 14.2+) and an in-daemon IOProc respectively.

use crate::BackendError;
use sonomux_core::{DevicesConfig, PoliciesConfig, RoutingConfig, StreamInfo};
use std::ffi::c_void;
use tracing::debug;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(
        id: u32,
        addr: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        id: u32,
        addr: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        id: u32,
        addr: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(s: *const c_void, buf: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFRelease(p: *const c_void);
}

extern "C" {
    // libproc, part of libSystem.
    fn proc_pidpath(pid: i32, buf: *mut c_void, size: u32) -> i32;
}

/// Set a plain-data property. Returns the OSStatus.
pub(crate) fn set<T: Copy>(id: u32, selector: u32, scope: u32, value: &[T]) -> i32 {
    unsafe {
        AudioObjectSetPropertyData(
            id,
            &addr(selector, scope),
            0,
            std::ptr::null(),
            std::mem::size_of_val(value) as u32,
            value.as_ptr().cast(),
        )
    }
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
pub(crate) const SCOPE_INPUT: u32 = fourcc(b"inpt");
pub(crate) const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const HW_DEVICES: u32 = fourcc(b"dev#");
const HW_PROCESSES: u32 = fourcc(b"prs#");
const OBJ_NAME: u32 = fourcc(b"lnam");
const DEV_UID: u32 = fourcc(b"uid ");
const DEV_STREAMS: u32 = fourcc(b"stm#");
pub(crate) const DEV_PREFERRED_STEREO: u32 = fourcc(b"dch2");
const PROC_PID: u32 = fourcc(b"ppid");
const PROC_BUNDLE_ID: u32 = fourcc(b"pbid");
const PROC_RUNNING_OUTPUT: u32 = fourcc(b"piro");
const PROC_DEVICES: u32 = fourcc(b"pdv#");
const CF_UTF8: u32 = 0x0800_0100;

fn addr(selector: u32, scope: u32) -> Address {
    Address {
        selector,
        scope,
        element: 0,
    }
}

fn data_size(id: u32, selector: u32, scope: u32) -> Option<u32> {
    let mut size = 0u32;
    let st = unsafe {
        AudioObjectGetPropertyDataSize(id, &addr(selector, scope), 0, std::ptr::null(), &mut size)
    };
    (st == 0).then_some(size)
}

fn get_vec<T: Copy + Default>(id: u32, selector: u32, scope: u32) -> Option<Vec<T>> {
    let mut size = data_size(id, selector, scope)?;
    let mut out = vec![T::default(); size as usize / std::mem::size_of::<T>()];
    let st = unsafe {
        AudioObjectGetPropertyData(
            id,
            &addr(selector, scope),
            0,
            std::ptr::null(),
            &mut size,
            out.as_mut_ptr().cast(),
        )
    };
    out.truncate(size as usize / std::mem::size_of::<T>());
    (st == 0).then_some(out)
}

fn get<T: Copy + Default>(id: u32, selector: u32, scope: u32) -> Option<T> {
    get_vec::<T>(id, selector, scope)?.first().copied()
}

fn get_string(id: u32, selector: u32) -> Option<String> {
    let cf = get::<usize>(id, selector, SCOPE_GLOBAL)? as *const c_void;
    if cf.is_null() {
        return None;
    }
    let mut buf = [0u8; 1024];
    let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, CF_UTF8) };
    unsafe { CFRelease(cf) };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

fn exe_path(pid: i32) -> Option<String> {
    let mut buf = [0u8; 4096];
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

/// A CoreAudio device as seen by the HAL.
#[derive(Debug, Clone)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub uid: String,
    pub has_input: bool,
    pub has_output: bool,
}

impl Device {
    /// Config values may hold either the device name or its UID.
    pub fn matches(&self, key: &str) -> bool {
        self.name == key || self.uid == key
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoreAudioManager;

impl CoreAudioManager {
    pub fn new() -> Self {
        Self
    }

    pub fn devices() -> Vec<Device> {
        get_vec::<u32>(SYSTEM_OBJECT, HW_DEVICES, SCOPE_GLOBAL)
            .unwrap_or_default()
            .into_iter()
            .map(|id| Device {
                id,
                name: get_string(id, OBJ_NAME).unwrap_or_default(),
                uid: get_string(id, DEV_UID).unwrap_or_default(),
                has_input: data_size(id, DEV_STREAMS, SCOPE_INPUT).unwrap_or(0) > 0,
                has_output: data_size(id, DEV_STREAMS, SCOPE_OUTPUT).unwrap_or(0) > 0,
            })
            .collect()
    }

    /// Every process the HAL knows has audio clients: (process object ID, PID, bundle ID).
    pub fn audio_processes() -> Vec<(u32, i32, String)> {
        get_vec::<u32>(SYSTEM_OBJECT, HW_PROCESSES, SCOPE_GLOBAL)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                (
                    p,
                    get::<i32>(p, PROC_PID, SCOPE_GLOBAL).unwrap_or(-1),
                    get_string(p, PROC_BUNDLE_ID).unwrap_or_default(),
                )
            })
            .collect()
    }

    /// The output device a process is playing to, if any.
    pub fn process_output_device(process: u32) -> Option<Device> {
        let id = get_vec::<u32>(process, PROC_DEVICES, SCOPE_OUTPUT)?
            .first()
            .copied()?;
        Self::devices().into_iter().find(|d| d.id == id)
    }

    /// First channel (0-based) of a device's preferred output stereo pair.
    pub fn preferred_stereo_first(device: u32) -> usize {
        get_vec::<u32>(device, DEV_PREFERRED_STEREO, SCOPE_OUTPUT)
            .and_then(|pair| pair.first().copied())
            .map_or(0, |ch| ch.saturating_sub(1) as usize)
    }

    pub async fn sink_exists(name: &str) -> bool {
        Self::devices()
            .iter()
            .any(|d| d.has_output && d.matches(name))
    }

    pub async fn source_exists(name: &str) -> bool {
        Self::devices()
            .iter()
            .any(|d| d.has_input && d.matches(name))
    }

    /// No virtual DSP sinks to walk through on macOS: the sink is its own target.
    pub async fn resolve_effective_sink(sink: &str) -> String {
        sink.to_string()
    }

    /// Processes currently playing audio, via the HAL process object list (macOS 14.2+).
    /// Returns an empty list on older systems.
    pub async fn get_active_streams() -> Result<Vec<StreamInfo>, BackendError> {
        let devices = Self::devices();
        let procs = get_vec::<u32>(SYSTEM_OBJECT, HW_PROCESSES, SCOPE_GLOBAL).unwrap_or_default();

        Ok(procs
            .into_iter()
            .filter(|&p| get::<u32>(p, PROC_RUNNING_OUTPUT, SCOPE_GLOBAL).unwrap_or(0) != 0)
            .map(|p| {
                let pid = get::<i32>(p, PROC_PID, SCOPE_GLOBAL).filter(|&pid| pid > 0);
                let exe = pid.and_then(exe_path);
                let binary = exe
                    .as_deref()
                    .and_then(|e| e.rsplit('/').next())
                    .unwrap_or_default()
                    .to_string();
                let bundle = get_string(p, PROC_BUNDLE_ID).filter(|b| !b.is_empty());
                let sink = get_vec::<u32>(p, PROC_DEVICES, SCOPE_OUTPUT)
                    .and_then(|ids| ids.first().copied())
                    .and_then(|id| devices.iter().find(|d| d.id == id));
                StreamInfo {
                    index: p as u64,
                    sink_index: sink.map(|d| d.id as u64).unwrap_or(0),
                    sink_name: sink.map(|d| d.name.clone()).unwrap_or_default(),
                    app_name: bundle.clone().unwrap_or_else(|| binary.clone()),
                    binary,
                    media_role: None,
                    media_name: None,
                    node_name: bundle,
                    pid: pid.map(|p| p as u32),
                    cmdline: None,
                    exe_path: exe,
                    is_corked: false,
                    is_muted: false,
                }
            })
            .collect())
    }

    // ponytail: routing and loopbacks are stubs until process taps land (port step 4).

    pub async fn ensure_loopback(
        _source: &str,
        _sink: &str,
        _latency_ms: u32,
    ) -> Result<u32, BackendError> {
        Err(BackendError::Routing(
            "loopback is not supported on macOS yet".into(),
        ))
    }

    pub async fn find_loopback_module(_source_name: &str) -> Option<u32> {
        None
    }

    pub async fn unload_module(_module_id: u32) -> Result<(), BackendError> {
        Ok(())
    }

    pub async fn move_stream(_stream_idx: u64, _target_sink: &str) -> Result<(), BackendError> {
        Err(BackendError::Routing(
            "per-app routing is not supported on macOS yet".into(),
        ))
    }

    /// No Virtual Surround Manager on macOS.
    pub async fn enforce_surround_policies(
        _surround_out_node: &str,
        _surround_in_node: &str,
        _game_sink: &str,
        _system_sink: &str,
        _keep_off_apps: &[&str],
    ) -> Result<(), BackendError> {
        Ok(())
    }

    /// Returns 0 moves: per-app routing is not implemented on macOS yet.
    pub async fn enforce_stream_routing(
        _routing_config: &RoutingConfig,
        _devices: &DevicesConfig,
        _policies: &PoliciesConfig,
    ) -> Result<usize, BackendError> {
        debug!("Stream routing skipped: not supported on macOS yet");
        Ok(0)
    }
}
