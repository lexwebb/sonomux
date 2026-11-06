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

    pub fn save_speaker_volume(&self, volume: u8) {
        let file = self.state_dir.join("saved-speaker-volume");
        if let Err(e) = fs::write(&file, volume.to_string()) {
            debug!("Failed to save speaker volume: {}", e);
        } else {
            info!("Saved speaker volume: {}", volume);
        }
    }

    pub fn load_speaker_volume(&self) -> Option<u8> {
        let file = self.state_dir.join("saved-speaker-volume");
        if let Ok(content) = fs::read_to_string(&file) {
            content.trim().parse::<u8>().ok()
        } else {
            None
        }
    }
}
