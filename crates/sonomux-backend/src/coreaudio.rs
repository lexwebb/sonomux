//! macOS CoreAudio backend.
//!
//! Mirrors the `PulseManager` API so callers can use `sonomux_backend::Backend` on either OS.
//! Device and process queries are real. Per-app routing and loopbacks are not implemented yet:
//! they need Core Audio process taps (macOS 14.2+) and an in-daemon IOProc respectively.

use crate::BackendError;
use sonomux_core::{DevicesConfig, PoliciesConfig, RoutingConfig, StreamInfo};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
pub(crate) const SCOPE_GLOBAL: u32 = fourcc(b"glob");
pub(crate) const SCOPE_INPUT: u32 = fourcc(b"inpt");
pub(crate) const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const HW_DEVICES: u32 = fourcc(b"dev#");
const HW_PROCESSES: u32 = fourcc(b"prs#");
const OBJ_NAME: u32 = fourcc(b"lnam");
const DEV_UID: u32 = fourcc(b"uid ");
pub(crate) const DEV_STREAMS: u32 = fourcc(b"stm#");
pub(crate) const DEV_PREFERRED_STEREO: u32 = fourcc(b"dch2");
const DEV_STREAM_CONFIG: u32 = fourcc(b"slay");
const PROC_PID: u32 = fourcc(b"ppid");
const PROC_BUNDLE_ID: u32 = fourcc(b"pbid");
const PROC_RUNNING_OUTPUT: u32 = fourcc(b"piro");
const PROC_DEVICES: u32 = fourcc(b"pdv#");
const HW_DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
const HW_DEFAULT_INPUT: u32 = fourcc(b"dIn ");
const HW_DEFAULT_SYSTEM_OUTPUT: u32 = fourcc(b"sOut");
const CF_UTF8: u32 = 0x0800_0100;

fn addr(selector: u32, scope: u32) -> Address {
    Address {
        selector,
        scope,
        element: 0,
    }
}

pub(crate) fn data_size(id: u32, selector: u32, scope: u32) -> Option<u32> {
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

pub(crate) fn get<T: Copy + Default>(id: u32, selector: u32, scope: u32) -> Option<T> {
    get_vec::<T>(id, selector, scope)?.first().copied()
}

pub(crate) fn get_string(id: u32, selector: u32) -> Option<String> {
    get_string_el(id, selector, SCOPE_GLOBAL, 0)
}

/// String property on a specific scope and element (e.g. a channel name).
pub(crate) fn get_string_el(id: u32, selector: u32, scope: u32, element: u32) -> Option<String> {
    let mut size = std::mem::size_of::<usize>() as u32;
    let mut raw = 0usize;
    let a = Address {
        selector,
        scope,
        element,
    };
    let st = unsafe {
        AudioObjectGetPropertyData(
            id,
            &a,
            0,
            std::ptr::null(),
            &mut size,
            (&mut raw as *mut usize).cast(),
        )
    };
    if st != 0 {
        return None;
    }
    let cf = raw as *const c_void;
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

/// The app an executable belongs to: the outermost `.app` in its path, without the extension
/// (".../Slack.app/.../Slack Helper.app/.../Slack Helper" -> "Slack"), else the file name.
pub fn app_of_exe(exe: &str) -> String {
    exe.split('/')
        .find_map(|c| c.strip_suffix(".app"))
        .or_else(|| exe.rsplit('/').next())
        .unwrap_or_default()
        .to_string()
}

/// An executable inside an app bundle that is not a macOS system service
/// (`/System/Applications`, e.g. Music, counts as an app).
pub fn is_user_app(exe: &str) -> bool {
    exe.contains(".app/")
        && (!exe.starts_with("/System/") || exe.starts_with("/System/Applications/"))
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

/// How long a device or process list read stays fresh. Well under one daemon tick.
// ponytail: time-based memo with explicit invalidation when we change devices ourselves;
// switch to a CoreAudio property listener if outside changes must show up faster than this.
const LIST_TTL: Duration = Duration::from_millis(250);

/// A short-lived copy of one HAL query.
struct Memo<T> {
    at: Option<Instant>,
    value: T,
}

impl<T: Default> Default for Memo<T> {
    fn default() -> Self {
        Self {
            at: None,
            value: T::default(),
        }
    }
}

impl<T: Clone> Memo<T> {
    /// The cached value if it is younger than `ttl` at `now`, else a fresh `read()`.
    fn get(&mut self, now: Instant, ttl: Duration, read: impl FnOnce() -> T) -> T {
        if self
            .at
            .is_none_or(|t| now.saturating_duration_since(t) >= ttl)
        {
            self.value = read();
            self.at = Some(now);
        }
        self.value.clone()
    }

    fn invalidate(&mut self) {
        self.at = None;
    }
}

fn device_memo() -> &'static Mutex<Memo<Vec<Device>>> {
    static M: OnceLock<Mutex<Memo<Vec<Device>>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

fn stream_memo() -> &'static Mutex<Memo<Vec<StreamInfo>>> {
    static M: OnceLock<Mutex<Memo<Vec<StreamInfo>>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Real device-list reads from the HAL since start, for measuring how often we enumerate.
static DEVICE_READS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
pub struct CoreAudioManager;

impl CoreAudioManager {
    pub fn new() -> Self {
        Self
    }

    pub fn devices() -> Vec<Device> {
        let mut memo = device_memo().lock().unwrap_or_else(|e| e.into_inner());
        memo.get(Instant::now(), LIST_TTL, || {
            DEVICE_READS.fetch_add(1, Ordering::Relaxed);
            Self::read_devices()
        })
    }

    /// How many times the device list was really read from the HAL (memo misses).
    pub fn device_reads() -> u64 {
        DEVICE_READS.load(Ordering::Relaxed)
    }

    /// Forget the cached device and process lists. Call after creating or destroying a device.
    pub fn invalidate_lists() {
        device_memo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate();
        stream_memo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate();
    }

    fn read_devices() -> Vec<Device> {
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

    /// Channels a device has in one direction (sum over its streams).
    pub fn channel_count(device: u32, input: bool) -> u32 {
        let scope = if input { SCOPE_INPUT } else { SCOPE_OUTPUT };
        // AudioBufferList: u32 count, padding, then {u32 channels, u32 bytes, *mut data} each.
        let Some(raw) = get_vec::<u64>(device, DEV_STREAM_CONFIG, scope) else {
            return 0;
        };
        let words: Vec<u32> = raw
            .iter()
            .flat_map(|w| [*w as u32, (*w >> 32) as u32])
            .collect();
        let n = words.first().copied().unwrap_or(0) as usize;
        (0..n).filter_map(|i| words.get(2 + i * 4)).sum()
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
        Ok(Self::all_streams()
            .into_iter()
            .filter(|s| !s.is_corked)
            .collect())
    }

    /// Every process with an audio client, playing or not (`is_corked` = not playing now).
    /// `index` is the HAL process object ID; `app_name` the bundle ID; `binary` the name of
    /// the app the process belongs to (its outermost `.app`), so helpers group with their app.
    // Lock order: stream memo, then (via `devices()`) device memo. Never the reverse.
    pub fn all_streams() -> Vec<StreamInfo> {
        let mut memo = stream_memo().lock().unwrap_or_else(|e| e.into_inner());
        memo.get(Instant::now(), LIST_TTL, Self::read_streams)
    }

    fn read_streams() -> Vec<StreamInfo> {
        let devices = Self::devices();
        get_vec::<u32>(SYSTEM_OBJECT, HW_PROCESSES, SCOPE_GLOBAL)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let pid = get::<i32>(p, PROC_PID, SCOPE_GLOBAL).filter(|&pid| pid > 0);
                let exe = pid.and_then(exe_path);
                let binary = exe.as_deref().map(app_of_exe).unwrap_or_default();
                let bundle = get_string(p, PROC_BUNDLE_ID).filter(|b| !b.is_empty());
                let playing = get::<u32>(p, PROC_RUNNING_OUTPUT, SCOPE_GLOBAL).unwrap_or(0) != 0;
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
                    is_corked: !playing,
                    is_muted: false,
                }
            })
            .collect()
    }

    /// Apps for the GUI's routing list: every audio process (playing or idle) that belongs to a
    /// user-facing app, leaving out system daemons and services.
    pub async fn list_streams() -> Result<Vec<StreamInfo>, BackendError> {
        Ok(Self::all_streams()
            .into_iter()
            .filter(|s| s.exe_path.as_deref().is_some_and(is_user_app))
            .collect())
    }

    /// Current default output (or input) device.
    pub fn default_device(input: bool) -> Option<Device> {
        let sel = if input {
            HW_DEFAULT_INPUT
        } else {
            HW_DEFAULT_OUTPUT
        };
        let id = get::<u32>(SYSTEM_OBJECT, sel, SCOPE_GLOBAL)?;
        Self::devices().into_iter().find(|d| d.id == id)
    }

    /// Make the device matching `key` (name or UID) the default output (and system-sound
    /// output) or input. Returns true if it changed anything.
    pub fn set_default_device(key: &str, input: bool) -> bool {
        let Some(dev) = Self::devices()
            .into_iter()
            .find(|d| d.matches(key) && if input { d.has_input } else { d.has_output })
        else {
            return false;
        };
        if Self::default_device(input).is_some_and(|d| d.id == dev.id) {
            return false;
        }
        let sels: &[u32] = if input {
            &[HW_DEFAULT_INPUT]
        } else {
            &[HW_DEFAULT_OUTPUT, HW_DEFAULT_SYSTEM_OUTPUT]
        };
        for &sel in sels {
            set(SYSTEM_OBJECT, sel, SCOPE_GLOBAL, &[dev.id]);
        }
        true
    }

    // ponytail: loopback is a stub on macOS; apps move via taps, not stream moves.

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

    /// Tap-and-route apps to their rule's channel (see `coreaudio_routing`).
    /// Returns how many processes are routed.
    pub async fn enforce_stream_routing(
        routing_config: &RoutingConfig,
        devices: &DevicesConfig,
        _policies: &PoliciesConfig,
    ) -> Result<usize, BackendError> {
        Ok(crate::coreaudio_routing::sync(routing_config, devices))
    }

    /// Release every routed app back to its normal output. Returns how many routes were dropped.
    pub async fn stop_stream_routing() -> usize {
        let n = crate::coreaudio_routing::clear();
        if n > 0 {
            tracing::info!("Routing stopped: released {} route(s)", n);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_reads_once_within_ttl_and_again_after() {
        let mut m: Memo<u32> = Memo::default();
        let mut reads = 0;
        let t0 = Instant::now();
        let ttl = Duration::from_millis(250);
        assert_eq!(
            m.get(t0, ttl, || {
                reads += 1;
                7
            }),
            7
        );
        assert_eq!(
            m.get(t0 + Duration::from_millis(100), ttl, || {
                reads += 1;
                8
            }),
            7
        );
        assert_eq!(reads, 1);
        assert_eq!(
            m.get(t0 + ttl, ttl, || {
                reads += 1;
                9
            }),
            9
        );
        assert_eq!(reads, 2);
        m.invalidate();
        assert_eq!(
            m.get(t0 + ttl, ttl, || {
                reads += 1;
                10
            }),
            10
        );
        assert_eq!(reads, 3);
    }

    #[test]
    fn a_ticks_worth_of_lookups_reads_the_device_list_at_most_once() {
        CoreAudioManager::invalidate_lists();
        let _ = CoreAudioManager::devices(); // warm
        let before = CoreAudioManager::device_reads();
        for _ in 0..12 {
            let _ = CoreAudioManager::devices();
        }
        let _ = CoreAudioManager::all_streams();
        let _ = CoreAudioManager::all_streams();
        // Other tests in this binary may invalidate concurrently, so allow one extra read.
        assert!(CoreAudioManager::device_reads() - before <= 2);
    }
}
