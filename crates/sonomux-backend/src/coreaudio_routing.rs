//! Keeps per-app routes in line with the routing rules on macOS.
//!
//! Each tick the daemon classifies every process with an audio client (playing or idle, so a
//! notification never plays on the wrong channel first), picks its target channel device from
//! the rules, and groups processes by (where they play now, where they should play). One tap
//! serves each group; a group's tap is rebuilt only when its set of processes changes.

use crate::coreaudio::{CoreAudioManager, Device};
use crate::coreaudio_channels::resolve_output;
use crate::coreaudio_tap::AppRoute;
use sonomux_core::{AppCategory, AppClassifier, DevicesConfig, RoutingConfig, StreamInfo};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tracing::{debug, info, warn};

/// (source hardware UID, target hardware UID, target first channel 0-based).
pub type RouteKey = (String, String, usize);

/// Live routes: the processes each tap covers, the device IDs it was built on, frames seen at
/// the last tick, and the tap.
type Live = HashMap<RouteKey, (Vec<u32>, DeviceIds, u64, AppRoute)>;

/// Object IDs of a route's source and target hardware. A device that comes back after sleep
/// or a replug gets a new ID and the old tap goes silent, so a changed ID means rebuild.
type DeviceIds = (Option<u32>, Option<u32>);

fn device_ids(key: &RouteKey, all: &[Device]) -> DeviceIds {
    let id = |uid: &str| all.iter().find(|d| d.uid == uid).map(|d| d.id);
    (id(&key.0), id(&key.1))
}

fn routes() -> &'static Mutex<Live> {
    static ROUTES: OnceLock<Mutex<Live>> = OnceLock::new();
    ROUTES.get_or_init(Default::default)
}

/// Target device name for a category, from the routing rules. macOS has no surround stage, so
/// "Surround" means the Game channel.
pub fn target_sink(
    category: AppCategory,
    routing: &RoutingConfig,
    devices: &DevicesConfig,
) -> String {
    let key = match category {
        AppCategory::Game => "game",
        AppCategory::Chat => "chat",
        AppCategory::Music => "music",
        AppCategory::Browser => "browser",
        AppCategory::Production => "production",
        AppCategory::Soundboard => "soundboard",
        AppCategory::System => "system",
    };
    let channel = routing
        .category_targets
        .get(key)
        .map(String::as_str)
        .unwrap_or("System");
    match channel {
        "Game" | "Surround" => &devices.goxlr_game_sink,
        "Chat" => &devices.goxlr_chat_sink,
        "Music" => &devices.goxlr_music_sink,
        "Sample" | "Soundboard" => &devices.goxlr_sample_sink,
        _ => &devices.goxlr_system_sink,
    }
    .clone()
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

/// Group processes by route. Pure apart from the inputs, so it is testable without hardware.
/// `source_of` gives the hardware a process plays to now; `resolve` maps a target device name
/// to (hardware UID, first channel). Processes already headed to the default output stay put.
pub fn plan(
    streams: &[StreamInfo],
    routing: &RoutingConfig,
    devices: &DevicesConfig,
    default_output: &str,
    own_pid: u32,
    source_of: impl Fn(&StreamInfo) -> Option<String>,
    resolve: impl Fn(&str) -> Option<(String, usize)>,
) -> HashMap<RouteKey, Vec<u32>> {
    let mut plan: HashMap<RouteKey, Vec<u32>> = HashMap::new();
    if !routing.enabled {
        return plan;
    }
    for s in streams {
        if s.pid.is_none_or(|p| p == own_pid) {
            continue;
        }
        let category = AppClassifier::classify(s, &routing.app_overrides);
        let target = target_sink(category, routing, devices);
        if target == default_output {
            continue;
        }
        let (Some(source), Some((hw, first))) = (source_of(s), resolve(&target)) else {
            continue;
        };
        plan.entry((source, hw, first))
            .or_default()
            .push(s.index as u32);
    }
    for procs in plan.values_mut() {
        procs.sort_unstable();
    }
    plan
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

    let mut live = routes().lock().unwrap();
    live.retain(|key, (procs, ids, frames, route)| {
        // A tap only counts frames while its apps play. One whose apps play but whose frames
        // stopped is dead (its hardware slept or was replugged): rebuild it.
        let playing = streams
            .iter()
            .any(|s| !s.is_corked && procs.contains(&(s.index as u32)));
        let now = route.frames();
        let alive = !playing || now > *frames;
        *frames = now;
        let keep = desired.get(key) == Some(procs) && *ids == device_ids(key, &all) && alive;
        if !keep {
            debug!("Dropping route {:?} (alive: {})", key, alive);
        }
        keep
    });
    for (key, procs) in desired {
        if live.contains_key(&key) {
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
            Err(e) => warn!("Could not route {:?}: {}", key, e),
        }
    }
    live.values().map(|(p, ..)| p.len()).sum()
}

/// Drop every route, restoring all apps' normal output.
pub fn clear() {
    routes().lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(obj: u64, pid: u32, binary: &str, bundle: &str) -> StreamInfo {
        StreamInfo {
            index: obj,
            pid: Some(pid),
            binary: binary.into(),
            app_name: bundle.into(),
            ..Default::default()
        }
    }

    fn devs() -> DevicesConfig {
        DevicesConfig {
            goxlr_system_sink: "Sys".into(),
            goxlr_chat_sink: "Chat".into(),
            goxlr_music_sink: "Music".into(),
            goxlr_game_sink: "Game".into(),
            goxlr_sample_sink: "Sample".into(),
            ..Default::default()
        }
    }

    fn run(streams: &[StreamInfo], routing: &RoutingConfig) -> HashMap<RouteKey, Vec<u32>> {
        plan(
            streams,
            routing,
            &devs(),
            "Sys",
            1,
            |_| Some("HW".into()),
            |name| match name {
                "Chat" => Some(("HW".into(), 4)),
                "Music" => Some(("HW".into(), 6)),
                _ => None,
            },
        )
    }

    #[test]
    fn groups_apps_by_target_and_skips_default() {
        let streams = [
            stream(10, 100, "Slack", "com.tinyspeck.slackmacgap.helper"),
            stream(11, 101, "Discord", "com.hnc.Discord.helper.Renderer"),
            stream(12, 102, "Spotify", "com.spotify.client"),
            stream(13, 103, "Zen", "app.zen-browser.zen"), // browser -> System = default
            stream(14, 1, "sonomuxd", "sonomuxd"),         // ourselves
        ];
        let p = run(&streams, &RoutingConfig::default());
        assert_eq!(p.len(), 2);
        assert_eq!(p[&("HW".into(), "HW".into(), 4)], [10, 11]);
        assert_eq!(p[&("HW".into(), "HW".into(), 6)], [12]);
    }

    #[test]
    fn disabled_routing_plans_nothing() {
        let routing = RoutingConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(run(&[stream(10, 100, "Slack", "x")], &routing).is_empty());
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
