use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// A GoXLR output channel an app or category can be sent to. Stored in config as its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelTarget {
    System,
    Game,
    Chat,
    Music,
    Sample,
    Surround,
}

impl ChannelTarget {
    pub const ALL: [ChannelTarget; 6] = [
        ChannelTarget::System,
        ChannelTarget::Game,
        ChannelTarget::Chat,
        ChannelTarget::Music,
        ChannelTarget::Sample,
        ChannelTarget::Surround,
    ];

    /// Case-insensitive; "Soundboard" is accepted for Sample.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "system" => Some(Self::System),
            "game" => Some(Self::Game),
            "chat" => Some(Self::Chat),
            "music" => Some(Self::Music),
            "sample" | "soundboard" => Some(Self::Sample),
            "surround" => Some(Self::Surround),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Game => "Game",
            Self::Chat => "Chat",
            Self::Music => "Music",
            Self::Sample => "Sample",
            Self::Surround => "Surround",
        }
    }

    /// Like `parse`, but an unknown name logs one warning (per distinct name, per process)
    /// and gives System. An empty name is System without a warning.
    pub fn parse_or_system(s: &str) -> Self {
        if let Some(c) = Self::parse(s) {
            return c;
        }
        if !s.trim().is_empty() {
            static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
            let mut warned = WARNED
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if warned.insert(s.to_string()) {
                tracing::warn!("Unknown channel '{}' in routing config; using System", s);
            }
        }
        Self::System
    }
}

impl std::fmt::Display for ChannelTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_round_trip() {
        for c in ChannelTarget::ALL {
            assert_eq!(ChannelTarget::parse(c.as_str()), Some(c));
            assert_eq!(c.to_string(), c.as_str());
        }
    }

    #[test]
    fn parse_variants() {
        assert_eq!(ChannelTarget::parse("music"), Some(ChannelTarget::Music));
        assert_eq!(
            ChannelTarget::parse("Soundboard"),
            Some(ChannelTarget::Sample)
        );
        assert_eq!(ChannelTarget::parse("Muisc"), None);
        assert_eq!(ChannelTarget::parse(""), None);
    }

    #[test]
    fn parse_or_system_falls_back() {
        assert_eq!(
            ChannelTarget::parse_or_system("Muisc"),
            ChannelTarget::System
        );
        // second call hits the already-warned path
        assert_eq!(
            ChannelTarget::parse_or_system("Muisc"),
            ChannelTarget::System
        );
        assert_eq!(ChannelTarget::parse_or_system(""), ChannelTarget::System);
        assert_eq!(ChannelTarget::parse_or_system("chat"), ChannelTarget::Chat);
    }
}
