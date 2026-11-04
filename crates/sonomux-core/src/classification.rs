use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Semantic purpose category for an application audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCategory {
    Game,
    Chat,
    Music,
    Browser,
    Production,
    Soundboard,
    System,
}

impl std::fmt::Display for AppCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppCategory::Game => write!(f, "Game"),
            AppCategory::Chat => write!(f, "Chat"),
            AppCategory::Music => write!(f, "Music"),
            AppCategory::Browser => write!(f, "Browser"),
            AppCategory::Production => write!(f, "Production"),
            AppCategory::Soundboard => write!(f, "Soundboard"),
            AppCategory::System => write!(f, "System"),
        }
    }
}

/// Metadata extracted from a PipeWire / PulseAudio sink-input stream.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u64,
    pub sink_index: u64,
    pub sink_name: String,
    pub app_name: String,
    pub binary: String,
    pub media_role: Option<String>,
    pub media_name: Option<String>,
    pub node_name: Option<String>,
    pub pid: Option<u32>,
    pub cmdline: Option<String>,
    pub exe_path: Option<String>,
    pub is_corked: bool,
    pub is_muted: bool,
}

impl StreamInfo {
    pub fn new(index: u64, sink_index: u64) -> Self {
        Self {
            index,
            sink_index,
            ..Default::default()
        }
    }
}

/// Curated database and heuristics engine for classifying desktop audio streams.
pub struct AppClassifier;

impl AppClassifier {
    /// Classify a stream using the full multi-tier heuristics pipeline:
    /// 1. User overrides (exact binary or application name)
    /// 2. PipeWire `media.role` metadata (music, communication, game)
    /// 3. Proton / Wine Windows executable name heuristics (.exe)
    /// 4. Process execution path and cmdline heuristics (/steamapps/, /games/, lutris, etc.)
    /// 5. Comprehensive desktop software catalog matching
    /// 6. Fallback default (System)
    pub fn classify(stream: &StreamInfo, overrides: &HashMap<String, String>) -> AppCategory {
        let app_name_clean = stream.app_name.trim();
        let binary_clean = stream.binary.trim();

        // 1. Explicit user override from config.toml
        if let Some(target) = overrides
            .get(binary_clean)
            .or_else(|| overrides.get(app_name_clean))
        {
            if let Some(cat) = Self::parse_category_name(target) {
                return cat;
            }
        }

        let app_lower = app_name_clean.to_lowercase();
        let bin_lower = binary_clean.to_lowercase();

        // 2. PipeWire / PulseAudio media.role metadata
        if let Some(ref role) = stream.media_role {
            match role.to_lowercase().as_str() {
                "music" => return AppCategory::Music,
                "communication" | "phone" => return AppCategory::Chat,
                "game" => return AppCategory::Game,
                "event" | "alert" => return AppCategory::System,
                _ => {}
            }
        }

        // 3. Game Auto-Detection Heuristics (Proton, Wine, Steam, Windows binaries)
        if Self::is_game_heuristic(stream, &app_lower, &bin_lower) {
            return AppCategory::Game;
        }

        // 4. Browser Catalog Matching
        if Self::is_browser(&app_lower, &bin_lower) {
            return AppCategory::Browser;
        }

        // 5. Music Catalog Matching
        if Self::is_music(&app_lower, &bin_lower) {
            return AppCategory::Music;
        }

        // 6. Voice / Chat Catalog Matching
        if Self::is_chat(&app_lower, &bin_lower) {
            return AppCategory::Chat;
        }

        // 7. Production / DAW Catalog Matching
        if Self::is_production(&app_lower, &bin_lower) {
            return AppCategory::Production;
        }

        // 8. Soundboard Catalog Matching
        if Self::is_soundboard(&app_lower, &bin_lower) {
            return AppCategory::Soundboard;
        }

        // 9. Default Fallback
        AppCategory::System
    }

    /// Helper to parse string representation into AppCategory
    pub fn parse_category_name(s: &str) -> Option<AppCategory> {
        match s.trim().to_lowercase().as_str() {
            "game" | "games" => Some(AppCategory::Game),
            "chat" | "voice" | "voip" => Some(AppCategory::Chat),
            "music" | "media" => Some(AppCategory::Music),
            "browser" | "browsers" | "web" => Some(AppCategory::Browser),
            "production" | "daw" | "stream" => Some(AppCategory::Production),
            "soundboard" | "sample" | "sampler" => Some(AppCategory::Soundboard),
            "system" | "default" => Some(AppCategory::System),
            _ => None,
        }
    }

    /// Advanced multi-signal heuristic for game detection on Linux
    pub fn is_game_heuristic(stream: &StreamInfo, app_lower: &str, bin_lower: &str) -> bool {
        // A. Windows executables (.exe) running under Wine/Proton
        if app_lower.ends_with(".exe") || bin_lower.ends_with(".exe") {
            // Ignore non-game known utilities if any
            if !app_lower.contains("speech-dispatcher") {
                return true;
            }
        }

        // B. Wine/Proton loader binaries
        const WINE_BINARIES: &[&str] = &[
            "wine",
            "wine64",
            "wine-preloader",
            "wine64-preloader",
            "wineserver",
            "gamescope",
        ];
        if WINE_BINARIES.contains(&bin_lower) {
            return true;
        }

        // C. Process path and cmdline heuristics (/proc/<pid>/exe and /proc/<pid>/cmdline)
        if let Some(ref path) = stream.exe_path {
            let path_lower = path.to_lowercase();
            if path_lower.contains("steamapps/common")
                || path_lower.contains("/games/")
                || path_lower.contains("/lutris/runners")
                || path_lower.contains("/heroic/runners")
                || path_lower.contains(".var/app/com.valvesoftware.steam")
            {
                return true;
            }
        }

        if let Some(ref cmd) = stream.cmdline {
            let cmd_lower = cmd.to_lowercase();
            if cmd_lower.contains("steamapps/common")
                || cmd_lower.contains("pressure-vessel")
                || cmd_lower.contains(".minecraft")
                || cmd_lower.contains("prismlauncher")
                || cmd_lower.contains("retroarch")
            {
                return true;
            }
        }

        // D. Popular Native Linux Games & Game Launchers/Emulators
        const NATIVE_GAMES_AND_RUNNERS: &[&str] = &[
            // Native Linux Games
            "cs2",
            "csgo_linux64",
            "dota2",
            "factorio",
            "hl2_linux",
            "portal2_linux",
            "valheim.x86_64",
            "deadcells",
            "celeste",
            "terraria.bin.x86_64",
            "tmodloader.bin.x86_64",
            "hades",
            "hades2",
            "hollow_knight.x86_64",
            "rimworldlinux",
            "slaythespire",
            "subnautica",
            "dontstarve",
            "eu4",
            "stellaris",
            "crusaderkings3",
            "cities.x64",
            "civ6",
            "xcom2",
            "metro",
            "bioshock",
            "borderlands2",
            "sober",
            "roblox",
            // Launchers & Emulators
            "steam",
            "steamwebhelper",
            "lutris",
            "heroic",
            "bottles",
            "prismlauncher",
            "polymc",
            "multimc",
            "minecraft",
            "retroarch",
            "rpcs3",
            "yuzu",
            "ryujinx",
            "dolphin-emu",
            "pcsx2",
            "cemu",
            "duckstation",
            "ppsspp",
            "xemu",
            "vita3k",
            "mgba",
            "snes9x",
            "mame",
            "dosbox",
            "scummvm",
        ];

        NATIVE_GAMES_AND_RUNNERS.contains(&bin_lower)
            || NATIVE_GAMES_AND_RUNNERS.contains(&app_lower)
    }

    /// Check if application is a web browser
    pub fn is_browser(app_lower: &str, bin_lower: &str) -> bool {
        const BROWSER_IDENTIFIERS: &[&str] = &[
            "zen",
            "zen-bin",
            "zen-browser",
            "firefox",
            "firefox-bin",
            "firefox-esr",
            "chromium",
            "chrome",
            "google-chrome",
            "google-chrome-stable",
            "google-chrome-beta",
            "google-chrome-unstable",
            "brave",
            "brave-browser",
            "brave-bin",
            "edge",
            "microsoft-edge",
            "microsoft-edge-stable",
            "microsoft-edge-dev",
            "opera",
            "opera-developer",
            "opera-beta",
            "vivaldi",
            "vivaldi-bin",
            "vivaldi-stable",
            "vivaldi-snapshot",
            "tor-browser",
            "torbrowser-launcher",
            "librewolf",
            "librewolf-bin",
            "waterfox",
            "waterfox-bin",
            "floorp",
            "floorp-bin",
            "epiphany",
            "qutebrowser",
            "midori",
            "falkon",
            "nyxt",
            "arc",
        ];

        BROWSER_IDENTIFIERS.contains(&bin_lower)
            || BROWSER_IDENTIFIERS.contains(&app_lower)
            || app_lower.contains("browser")
    }

    /// Check if application is a dedicated music / audio player
    pub fn is_music(app_lower: &str, bin_lower: &str) -> bool {
        const MUSIC_IDENTIFIERS: &[&str] = &[
            "spotify",
            "spotify-launcher",
            "ncspot",
            "spotify-tui",
            "spotifyd",
            "rhythmbox",
            "clementine",
            "strawberry",
            "audacious",
            "deadbeef",
            "cider",
            "cider2",
            "tidal-hifi",
            "tidal",
            "deezer",
            "deezer-desktop",
            "youtube-music",
            "youtube-music-desktop-app",
            "amberol",
            "tauon",
            "tauonmb",
            "foobar2000",
            "quodlibet",
            "lollypop",
            "elisa",
            "cmus",
            "moc",
            "apple-music",
            "plexamp",
            "feishin",
            "navidrome",
            "supersonic",
            "sublime-music",
            "sayonara",
            "cantata",
            "pragha",
        ];

        MUSIC_IDENTIFIERS.contains(&bin_lower) || MUSIC_IDENTIFIERS.contains(&app_lower)
    }

    /// Check if application is a voice / VoIP / communication app
    pub fn is_chat(app_lower: &str, bin_lower: &str) -> bool {
        const CHAT_IDENTIFIERS: &[&str] = &[
            "discord",
            "vesktop",
            "webcord",
            "armcord",
            "discord-canary",
            "discord-ptb",
            "lightcord",
            "legcord",
            "telegram",
            "telegram-desktop",
            "telegramdesktop",
            "kotatogram-desktop",
            "ayugram-desktop",
            "slack",
            "slack-desktop",
            "teams",
            "teams-for-linux",
            "skype",
            "skypeforlinux",
            "zoom",
            "zoom-client",
            "zoom.real",
            "element",
            "element-desktop",
            "schildichat",
            "fluffychat",
            "cinny",
            "fractal",
            "neochat",
            "mumble",
            "murmur",
            "ts3client",
            "ts3client_linux_amd64",
            "teamspeak",
            "signal",
            "signal-desktop",
            "signal-desktop-beta",
            "webex",
            "cisco-webex",
            "session",
            "session-desktop",
            "revolt",
            "revolt-desktop",
            "wire",
            "wire-desktop",
            "tox",
            "qtox",
        ];

        CHAT_IDENTIFIERS.contains(&bin_lower) || CHAT_IDENTIFIERS.contains(&app_lower)
    }

    /// Check if application is a DAW, audio editor, or streaming studio
    pub fn is_production(app_lower: &str, bin_lower: &str) -> bool {
        const PROD_IDENTIFIERS: &[&str] = &[
            "obs",
            "obs64",
            "obs-studio",
            "streamlabs",
            "audacity",
            "tenacity",
            "reaper",
            "ardour",
            "bitwig-studio",
            "lmms",
            "flstudio",
            "kdenlive",
            "davinci-resolve",
            "shotcut",
            "openshot",
            "blender",
            "carla",
            "helvum",
            "qpwgraph",
            "easyeffects",
        ];

        PROD_IDENTIFIERS.contains(&bin_lower) || PROD_IDENTIFIERS.contains(&app_lower)
    }

    /// Check if application is a soundboard or SFX player
    pub fn is_soundboard(app_lower: &str, bin_lower: &str) -> bool {
        const SOUNDBOARD_IDENTIFIERS: &[&str] = &[
            "soundux",
            "soundux-gui",
            "soundboard",
            "jingle-palette",
            "sample",
        ];

        SOUNDBOARD_IDENTIFIERS.contains(&bin_lower) || SOUNDBOARD_IDENTIFIERS.contains(&app_lower)
    }
}
