//! Pure routing decisions for per-app routes: which processes go where, and whether a live
//! route is still good. No CoreAudio calls, so this compiles and is tested on every platform.

use sonomux_core::classification::channel_for;
use sonomux_core::{ChannelTarget, DevicesConfig, RoutingConfig, StreamInfo};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// (source hardware UID, target hardware UID, target first channel 0-based).
pub type RouteKey = (String, String, usize);

/// Object IDs of a route's source and target hardware. A device that comes back after sleep
/// or a replug gets a new ID and the old tap goes silent, so a changed ID means rebuild.
pub type DeviceIds = (Option<u32>, Option<u32>);

/// Sink for a channel on macOS: no surround stage, so Surround plays on Game.
pub fn target_sink(channel: ChannelTarget, devices: &DevicesConfig) -> String {
    match channel {
        ChannelTarget::Game | ChannelTarget::Surround => &devices.goxlr_game_sink,
        ChannelTarget::Chat => &devices.goxlr_chat_sink,
        ChannelTarget::Music => &devices.goxlr_music_sink,
        ChannelTarget::Sample => &devices.goxlr_sample_sink,
        ChannelTarget::System => &devices.goxlr_system_sink,
    }
    .clone()
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
        let target = target_sink(channel_for(s, routing), devices);
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

/// True if any of `procs` (HAL process object IDs) is playing right now.
pub fn any_playing(streams: &[StreamInfo], procs: &[u32]) -> bool {
    streams
        .iter()
        .any(|s| !s.is_corked && procs.contains(&(s.index as u32)))
}

/// Whether a live route should stay this tick. It stays only if the plan still wants exactly
/// these processes on it, its hardware still has the object IDs it was built on (a device
/// back from sleep or a replug gets new IDs), and it is alive. A tap only counts frames while
/// its apps play, so a route whose apps play but whose frame count has not moved is dead.
pub fn keep_route(
    desired: Option<&Vec<u32>>,
    procs: &[u32],
    built_on: DeviceIds,
    ids_now: DeviceIds,
    playing: bool,
    frames_before: u64,
    frames_now: u64,
) -> bool {
    let alive = !route_dead(playing, frames_before, frames_now);
    desired.is_some_and(|d| d.as_slice() == procs) && built_on == ids_now && alive
}

/// A route whose apps play but whose tap has not counted a frame since the last tick.
pub fn route_dead(playing: bool, frames_before: u64, frames_now: u64) -> bool {
    playing && frames_now <= frames_before
}

/// Retry pacing for one route key that keeps failing (start errors, or dead right after start).
/// Waits 1 s, 2 s, 4 s ... up to 60 s between attempts.
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    failures: u32,
    until: Option<Instant>,
}

impl Backoff {
    /// Longest wait between attempts.
    pub const MAX: Duration = Duration::from_secs(60);

    /// May we try this route again at `now`?
    pub fn ready(&self, now: Instant) -> bool {
        self.until.is_none_or(|u| now >= u)
    }

    /// Record a failure at `now`. Returns true only for the first failure in a row, so the
    /// caller warns once and logs later failures at debug level.
    pub fn fail(&mut self, now: Instant) -> bool {
        self.failures = self.failures.saturating_add(1);
        let wait = Duration::from_secs(1u64 << (self.failures - 1).min(6)).min(Self::MAX);
        self.until = Some(now + wait);
        self.failures == 1
    }

    /// How many failures in a row.
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

/// Fill one interleaved output buffer for the audio callback. Real-time safe: no allocation,
/// no locks, no indexing that can panic.
///
/// `out` has `out_ch` channels starting at device channel `channel_base`. Our stereo pair
/// starts at device channel `first_channel`. Every channel in `out` is set to silence, then each
/// side of our pair that falls inside this buffer gets the tap: a mono tap goes to both sides,
/// otherwise each side is the sum of that side across every pair of the tap (`tap_ch` channels,
/// interleaved). Frames past the shorter of `out` and `tap` stay silent. Returns the peak
/// absolute sample written.
pub fn mix_pair(
    out: &mut [f32],
    out_ch: usize,
    channel_base: usize,
    first_channel: usize,
    tap: &[f32],
    tap_ch: usize,
) -> f32 {
    out.fill(0.0);
    if out_ch == 0 || tap_ch == 0 {
        return 0.0;
    }
    let mut peak = 0f32;
    for side in 0..2 {
        let target = first_channel + side;
        if target < channel_base || target >= channel_base + out_ch {
            continue;
        }
        let col = target - channel_base;
        for (orow, trow) in out.chunks_exact_mut(out_ch).zip(tap.chunks_exact(tap_ch)) {
            let v: f32 = if tap_ch == 1 {
                trow[0]
            } else {
                trow.iter().skip(side).step_by(2).sum()
            };
            orow[col] = v;
            peak = peak.max(v.abs());
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_backoff_is_ready() {
        assert!(Backoff::default().ready(Instant::now()));
    }

    #[test]
    fn first_failure_warns_then_quiet() {
        let t0 = Instant::now();
        let mut b = Backoff::default();
        assert!(b.fail(t0));
        assert!(!b.fail(t0));
        assert_eq!(b.failures(), 2);
    }

    #[test]
    fn waits_double_each_time() {
        let t0 = Instant::now();
        let mut b = Backoff::default();
        b.fail(t0);
        assert!(!b.ready(t0 + Duration::from_millis(999)));
        assert!(b.ready(t0 + Duration::from_secs(1)));
        b.fail(t0);
        assert!(!b.ready(t0 + Duration::from_millis(1999)));
        assert!(b.ready(t0 + Duration::from_secs(2)));
    }

    #[test]
    fn wait_caps_at_sixty_seconds() {
        let t0 = Instant::now();
        let mut b = Backoff::default();
        for _ in 0..20 {
            b.fail(t0);
        }
        assert!(!b.ready(t0 + Duration::from_secs(59)));
        assert!(b.ready(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn route_dead_cases() {
        assert!(route_dead(true, 5, 5));
        assert!(!route_dead(true, 5, 6));
        assert!(!route_dead(false, 5, 5));
    }

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
    fn target_sink_maps_every_channel() {
        let d = devs();
        let got: Vec<String> = ChannelTarget::ALL
            .iter()
            .map(|c| target_sink(*c, &d))
            .collect();
        assert_eq!(got, ["Sys", "Game", "Chat", "Music", "Sample", "Game"]);
    }

    #[test]
    fn override_naming_a_channel_routes_there_directly() {
        let mut routing = RoutingConfig::default();
        routing.app_overrides.insert("Zen".into(), "Music".into());
        routing
            .app_overrides
            .insert("Slack".into(), "Surround".into());
        let streams = [
            stream(10, 100, "Zen", "app.zen-browser.zen"),
            stream(11, 101, "Slack", "com.tinyspeck.slackmacgap"),
        ];
        let p = run(&streams, &routing);
        // Surround on macOS is the Game sink, which the test resolver does not know: skipped.
        assert_eq!(p.len(), 1);
        assert_eq!(p[&("HW".into(), "HW".into(), 6)], [10]);
    }

    #[test]
    fn disabled_routing_plans_nothing() {
        let routing = RoutingConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(run(&[stream(10, 100, "Slack", "x")], &routing).is_empty());
    }

    const IDS: DeviceIds = (Some(10), Some(20));

    #[test]
    fn keeps_unchanged_idle_route() {
        let procs = vec![1, 2];
        assert!(keep_route(Some(&procs), &procs, IDS, IDS, false, 5, 5));
    }

    #[test]
    fn drops_when_process_set_changes() {
        let procs = vec![1, 2];
        let desired = vec![1, 2, 3];
        assert!(!keep_route(Some(&desired), &procs, IDS, IDS, false, 5, 5));
    }

    #[test]
    fn drops_when_not_desired() {
        let procs = vec![1, 2];
        assert!(!keep_route(None, &procs, IDS, IDS, false, 5, 5));
    }

    #[test]
    fn drops_when_device_id_changes() {
        let procs = vec![1, 2];
        let new_id = (Some(11), Some(20));
        assert!(!keep_route(Some(&procs), &procs, IDS, new_id, false, 5, 5));
        let gone = (None, Some(20));
        assert!(!keep_route(Some(&procs), &procs, IDS, gone, false, 5, 5));
    }

    #[test]
    fn drops_playing_route_whose_frames_stalled() {
        let procs = vec![1, 2];
        assert!(!keep_route(Some(&procs), &procs, IDS, IDS, true, 100, 100));
    }

    #[test]
    fn keeps_playing_route_with_advancing_frames() {
        let procs = vec![1, 2];
        assert!(keep_route(Some(&procs), &procs, IDS, IDS, true, 100, 612));
    }

    #[test]
    fn keeps_idle_route_with_stalled_frames() {
        let procs = vec![1, 2];
        assert!(keep_route(Some(&procs), &procs, IDS, IDS, false, 100, 100));
    }

    #[test]
    fn any_playing_checks_corked_and_membership() {
        let streams = [
            StreamInfo {
                index: 7,
                is_corked: false,
                ..Default::default()
            },
            StreamInfo {
                index: 8,
                is_corked: true,
                ..Default::default()
            },
        ];
        assert!(any_playing(&streams, &[7]));
        assert!(!any_playing(&streams, &[8]));
        assert!(!any_playing(&streams, &[9]));
        assert!(!any_playing(&streams, &[]));
    }

    #[test]
    fn mono_tap_goes_to_both_sides() {
        let mut out = [9.0; 8];
        let peak = mix_pair(&mut out, 4, 0, 2, &[0.5, -0.25], 1);
        assert_eq!(out, [0.0, 0.0, 0.5, 0.5, 0.0, 0.0, -0.25, -0.25]);
        assert_eq!(peak, 0.5);
    }

    #[test]
    fn stereo_tap_copies_sides() {
        let mut out = [0.0; 4];
        mix_pair(&mut out, 2, 0, 0, &[0.1, 0.2, 0.3, 0.4], 2);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn multi_pair_tap_sums_each_side() {
        let mut out = [0.0; 2];
        mix_pair(&mut out, 2, 0, 0, &[0.1, 0.2, 0.3, 0.4], 4);
        assert!((out[0] - 0.4).abs() < 1e-6);
        assert!((out[1] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn pair_split_across_two_buffers() {
        let tap = [0.7, 0.9];
        let mut a = [0.0; 2];
        let mut b = [0.0; 2];
        mix_pair(&mut a, 2, 0, 1, &tap, 2);
        mix_pair(&mut b, 2, 2, 1, &tap, 2);
        assert_eq!(a, [0.0, 0.7]);
        assert_eq!(b, [0.9, 0.0]);
    }

    #[test]
    fn pair_outside_buffer_writes_silence() {
        let mut out = [1.0; 4];
        let peak = mix_pair(&mut out, 2, 0, 4, &[0.5, 0.5, 0.5, 0.5], 2);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn output_longer_than_tap() {
        let mut out = [1.0; 6];
        mix_pair(&mut out, 2, 0, 0, &[0.5, 0.5], 2);
        assert_eq!(out, [0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn tap_longer_than_output() {
        let mut out = [1.0; 2];
        mix_pair(&mut out, 2, 0, 0, &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 2);
        assert_eq!(out, [0.1, 0.2]);
    }

    #[test]
    fn empty_tap_writes_silence() {
        let mut out = [1.0; 4];
        let peak = mix_pair(&mut out, 2, 0, 0, &[], 2);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn zero_channels_is_safe() {
        let mut out = [1.0; 4];
        assert_eq!(mix_pair(&mut out, 0, 0, 0, &[0.5, 0.5], 2), 0.0);
        assert_eq!(mix_pair(&mut out, 2, 0, 0, &[0.5, 0.5], 0), 0.0);
    }
}
