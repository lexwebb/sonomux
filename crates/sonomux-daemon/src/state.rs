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

/// How long a GoXLR Utility reply proves it is up. The fader poll asks for status 10 times a
/// second and its replies take 0.1-0.8 s (more when busy), so a separate probe on every tick
/// only doubles the load and times out behind it.
pub const UTILITY_FRESH: std::time::Duration = std::time::Duration::from_secs(5);

/// Did the Utility answer recently enough at `now` to count as up without asking again?
pub fn replied_recently(last_ok: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last_ok.is_some_and(|t| now.saturating_duration_since(t) < UTILITY_FRESH)
}

#[cfg(test)]
mod presence_tests {
    use super::{replied_recently, Presence, UTILITY_FRESH};
    use std::time::{Duration, Instant};

    #[test]
    fn utility_reply_is_fresh_for_a_while() {
        let t = Instant::now();
        assert!(!replied_recently(None, t));
        assert!(replied_recently(Some(t), t + Duration::from_secs(4)));
        assert!(!replied_recently(Some(t), t + UTILITY_FRESH));
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
