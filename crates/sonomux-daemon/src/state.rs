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
