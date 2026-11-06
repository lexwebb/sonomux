//! Keeps per-app routes in line with the routing rules on macOS.
//!
//! Each tick the daemon classifies every process with an audio client (playing or idle, so a
//! notification never plays on the wrong channel first), picks its target channel device from
//! the rules, and groups processes by (where they play now, where they should play). One tap
//! serves each group; a group's tap is rebuilt only when its set of processes changes.

use crate::coreaudio::{CoreAudioManager, Device};
use crate::coreaudio_channels::resolve_output;
use crate::coreaudio_tap::AppRoute;
use crate::routing_logic::DeviceIds;
use crate::routing_logic::{any_playing, keep_route, route_dead, Backoff};
pub use crate::routing_logic::{plan, target_sink, RouteKey};
use sonomux_core::{DevicesConfig, RoutingConfig};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tracing::{debug, info, warn};

/// Live routes: the processes each tap covers, the device IDs it was built on, frames seen at
/// the last tick, and the tap.
type Live = HashMap<RouteKey, (Vec<u32>, DeviceIds, u64, AppRoute)>;

fn device_ids(key: &RouteKey, all: &[Device]) -> DeviceIds {
    let id = |uid: &str| all.iter().find(|d| d.uid == uid).map(|d| d.id);
    (id(&key.0), id(&key.1))
}

fn routes() -> &'static Mutex<Live> {
    static ROUTES: OnceLock<Mutex<Live>> = OnceLock::new();
    ROUTES.get_or_init(Default::default)
}

// ponytail: one global map keyed like routes; per-key state lives with the route if this grows
fn backoffs() -> &'static Mutex<HashMap<RouteKey, Backoff>> {
    static BACKOFFS: OnceLock<Mutex<HashMap<RouteKey, Backoff>>> = OnceLock::new();
    BACKOFFS.get_or_init(Default::default)
}

/// Hardware UID and first output channel (0-based) that a device plays to: a split device
/// resolves to its hardware and pair, anything else to itself and its preferred pair.
fn hardware_of(dev: &Device) -> (String, usize) {
    match resolve_output(dev.id) {
        Some((hw, first)) => (hw, first.saturating_sub(1) as usize),
        None => (
            dev.uid.clone(),
            CoreAudioManager::preferred_stereo_first(dev.id),
        ),
    }
}

/// Bring live taps in line with the rules. Returns how many processes are routed.
pub fn sync(routing: &RoutingConfig, devices: &DevicesConfig) -> usize {
    let all = CoreAudioManager::devices();
    let default = CoreAudioManager::default_device(false);
    let default_name = default.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    let default_hw = default.as_ref().map(|d| hardware_of(d).0);
    let streams = CoreAudioManager::all_streams();

    let desired = plan(
        &streams,
        routing,
        devices,
        &default_name,
        std::process::id(),
        |s| {
            // Where it plays now; idle processes will start on the default output.
            all.iter()
                .find(|d| d.id as u64 == s.sink_index && s.sink_index != 0)
                .map(|d| hardware_of(d).0)
                .or_else(|| default_hw.clone())
        },
        |name| {
            all.iter()
                .find(|d| d.has_output && d.matches(name))
                .map(hardware_of)
        },
    );

    let now = std::time::Instant::now();
    let mut live = routes().lock().unwrap();
    let mut backoff = backoffs().lock().unwrap();
    backoff.retain(|k, _| desired.contains_key(k));
    live.retain(|key, (procs, ids, frames, route)| {
        let now_frames = route.frames();
        let playing = any_playing(&streams, procs);
        let dead = route_dead(playing, *frames, now_frames);
        let keep = keep_route(
            desired.get(key),
            procs,
            *ids,
            device_ids(key, &all),
            playing,
            *frames,
            now_frames,
        );
        if !keep && dead {
            backoff.entry(key.clone()).or_default().fail(now);
        } else if keep && playing && now_frames > *frames {
            backoff.remove(key);
        }
        *frames = now_frames;
        if !keep {
            debug!("Dropping route {:?}", key);
        }
        keep
    });
    for (key, procs) in desired {
        if live.contains_key(&key) {
            continue;
        }
        if backoff.get(&key).is_some_and(|b| !b.ready(now)) {
            continue;
        }
        let (source, target, first) = &key;
        match AppRoute::start(&procs, source, target, *first) {
            Ok(route) => {
                let names: Vec<&str> = streams
                    .iter()
                    .filter(|s| procs.contains(&(s.index as u32)))
                    .map(|s| s.binary.as_str())
                    .collect();
                info!(
                    "Routing {:?} -> {} ch {}/{}",
                    names,
                    target,
                    first + 1,
                    first + 2
                );
                let ids = device_ids(&key, &all);
                live.insert(key, (procs, ids, 0, route));
            }
            Err(e) => {
                if backoff.entry(key.clone()).or_default().fail(now) {
                    warn!("Could not route {:?}: {} (will retry with backoff)", key, e);
                } else {
                    debug!("Could not route {:?}: {}", key, e);
                }
            }
        }
    }
    live.values().map(|(p, ..)| p.len()).sum()
}

/// Drop every route, restoring all apps' normal output. Returns how many routes there were.
pub fn clear() -> usize {
    let mut live = routes().lock().unwrap();
    let n = live.len();
    live.clear();
    n
}

#[cfg(test)]
mod tests {
    use super::clear;

    #[test]
    fn clear_with_no_routes_is_a_no_op() {
        assert_eq!(clear(), 0);
    }

    #[test]
    fn app_of_exe_uses_outermost_app() {
        use crate::coreaudio::app_of_exe;
        assert_eq!(
            app_of_exe("/Applications/Slack.app/Contents/Frameworks/Slack Helper.app/Contents/MacOS/Slack Helper"),
            "Slack"
        );
        assert_eq!(app_of_exe("/usr/bin/afplay"), "afplay");
    }
}
