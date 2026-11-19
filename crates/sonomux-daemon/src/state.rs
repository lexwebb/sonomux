use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

pub struct StateStore {
    state_dir: PathBuf,
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateStore {
    pub fn new() -> Self {
        let base = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".local/state")
            });
        Self::in_dir(base.join("sonomux"))
    }

    /// Store state files in `state_dir` (created if missing).
    pub fn in_dir(state_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&state_dir);
        Self { state_dir }
    }

    /// File for a volume slot. `speaker_volume` keeps the legacy file name; any
    /// other slot is sanitised so config-supplied names can never form a path.
    fn slot_file(&self, slot: &str) -> PathBuf {
        if slot == "speaker_volume" {
            return self.state_dir.join("saved-speaker-volume");
        }
        let safe: String = slot
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe = if safe.is_empty() {
            "_".to_string()
        } else {
            safe
        };
        self.state_dir.join(format!("saved-volume-{safe}"))
    }

    pub fn save_volume(&self, slot: &str, volume: u8) {
        let file = self.slot_file(slot);
        if let Err(e) = fs::write(&file, volume.to_string()) {
            debug!("Failed to save volume for slot {}: {}", slot, e);
        } else {
            info!("Saved volume for slot {}: {}", slot, volume);
        }
    }

    pub fn load_volume(&self, slot: &str) -> Option<u8> {
        let file = self.slot_file(slot);
        if let Ok(content) = fs::read_to_string(&file) {
            content.trim().parse::<u8>().ok()
        } else {
            None
        }
    }
}

/// Debounced GoXLR presence. Losing it releases every route (apps fall back to the System
/// fader, so volumes jump), and a single failed probe can be a blip, so it only counts as gone
/// after `MISSES` failed checks in a row. Coming back counts at once.
#[derive(Debug, Default)]
pub struct Presence {
    up: bool,
    misses: u32,
}

impl Presence {
    /// Failed checks in a row before the device counts as gone.
    pub const MISSES: u32 = 3;

    /// Record one check; returns whether the device counts as present.
    pub fn update(&mut self, seen: bool) -> bool {
        if seen {
            self.misses = 0;
            self.up = true;
        } else {
            self.misses = self.misses.saturating_add(1);
            if self.misses >= Self::MISSES {
                self.up = false;
            }
        }
        self.up
    }

    /// Failed checks in a row so far.
    pub fn misses(&self) -> u32 {
        self.misses
    }
}

/// GoXLR poll interval with no one watching the faders. The tray and status bar refresh once a
/// second and only show the speaker level, so this only bounds how stale that is.
pub const FADER_POLL_IDLE: std::time::Duration = std::time::Duration::from_secs(5);
/// GoXLR poll interval while the GUI's Mixer page is open, so strips follow the hardware.
pub const FADER_POLL_LIVE: std::time::Duration = std::time::Duration::from_millis(100);
/// The Mixer page counts as open while it read volumes this recently (it reads every 100 ms).
pub const VOLUMES_WATCHED: std::time::Duration = std::time::Duration::from_secs(1);

/// How long one GoXLR Utility reply proves it is up: three idle polls. Its replies take
/// 0.1-0.8 s and longer when busy, so a single missed or late reply is not a disconnect.
pub const UTILITY_FRESH: std::time::Duration = std::time::Duration::from_secs(15);

/// Interval until the next GoXLR poll, given when a client last read volumes.
pub fn fader_poll_interval(
    volumes_read_at: Option<std::time::Instant>,
    now: std::time::Instant,
) -> std::time::Duration {
    match volumes_read_at {
        Some(t) if now.saturating_duration_since(t) < VOLUMES_WATCHED => FADER_POLL_LIVE,
        _ => FADER_POLL_IDLE,
    }
}

/// Did the Utility answer recently enough at `now` to count as up without asking again?
pub fn replied_recently(last_ok: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last_ok.is_some_and(|t| now.saturating_duration_since(t) < UTILITY_FRESH)
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn utility_reply_is_fresh_for_three_idle_polls() {
        let t = Instant::now();
        assert!(!replied_recently(None, t));
        assert!(replied_recently(Some(t), t + FADER_POLL_IDLE * 2));
        assert!(!replied_recently(Some(t), t + FADER_POLL_IDLE * 3));
    }

    #[test]
    fn polls_fast_only_while_volumes_are_read() {
        let t = Instant::now();
        assert_eq!(fader_poll_interval(None, t), FADER_POLL_IDLE);
        assert_eq!(
            fader_poll_interval(Some(t), t + Duration::from_millis(500)),
            FADER_POLL_LIVE
        );
        assert_eq!(
            fader_poll_interval(Some(t), t + VOLUMES_WATCHED),
            FADER_POLL_IDLE
        );
    }

    #[test]
    fn starts_absent_and_comes_up_at_once() {
        let mut p = Presence::default();
        assert!(!p.update(false));
        assert!(p.update(true));
    }

    #[test]
    fn blips_shorter_than_misses_keep_it_up() {
        let mut p = Presence::default();
        p.update(true);
        for _ in 1..Presence::MISSES {
            assert!(p.update(false));
        }
        assert!(p.update(true)); // recovered: the count starts over
        assert_eq!(p.misses(), 0);
        for _ in 1..Presence::MISSES {
            assert!(p.update(false));
        }
    }

    #[test]
    fn goes_down_after_misses_in_a_row() {
        let mut p = Presence::default();
        p.update(true);
        for _ in 1..Presence::MISSES {
            p.update(false);
        }
        assert!(!p.update(false));
        assert!(!p.update(false));
    }
}
