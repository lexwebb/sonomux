use sonomux_core::classification::*;
use std::collections::HashMap;

#[test]
fn test_classify_media_role_precedence() {
    let overrides = HashMap::new();
    let mut stream = StreamInfo::default();
    stream.app_name = "UnknownApp".to_string();
    stream.binary = "unknown_bin".to_string();

    stream.media_role = Some("music".to_string());
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Music);

    stream.media_role = Some("communication".to_string());
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Chat);

    stream.media_role = Some("game".to_string());
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);

    stream.media_role = Some("alert".to_string());
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::System);
}

#[test]
fn test_classify_explicit_user_overrides() {
    let mut overrides = HashMap::new();
    overrides.insert("my-music-tool".to_string(), "Music".to_string());
    overrides.insert("zen-bin".to_string(), "Music".to_string()); // Override browser to music

    let mut stream = StreamInfo::default();
    stream.binary = "my-music-tool".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Music);

    stream.binary = "zen-bin".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Music);
}

#[test]
fn test_classify_wine_proton_games_exe() {
    let overrides = HashMap::new();

    let mut stream = StreamInfo::default();
    stream.app_name = "Cyberpunk2077.exe".to_string();
    stream.binary = "wine64-preloader".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);

    stream.app_name = "eldenring.exe".to_string();
    stream.binary = "wine64".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);

    stream.app_name = "MonsterHunterWilds.exe".to_string();
    stream.binary = "wineserver".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);
}

#[test]
fn test_classify_gamescope_and_wine_loaders() {
    let overrides = HashMap::new();

    let mut stream = StreamInfo::default();
    stream.binary = "gamescope".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);

    stream.binary = "wine-preloader".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);
}

#[test]
fn test_classify_path_and_cmdline_heuristics() {
    let overrides = HashMap::new();

    let mut stream = StreamInfo::default();
    stream.app_name = "game_binary".to_string();
    stream.binary = "game_binary".to_string();
    stream.exe_path = Some("/home/user/.local/share/Steam/steamapps/common/Half-Life 2/hl2".to_string());
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game);

    let mut stream2 = StreamInfo::default();
    stream2.cmdline = Some("/usr/bin/bwrap --args pressure-vessel-wrap game".to_string());
    assert_eq!(AppClassifier::classify(&stream2, &overrides), AppCategory::Game);

    let mut stream3 = StreamInfo::default();
    stream3.cmdline = Some("prismlauncher --launch 1.20.4".to_string());
    assert_eq!(AppClassifier::classify(&stream3, &overrides), AppCategory::Game);
}

#[test]
fn test_classify_native_games_and_emulators() {
    let overrides = HashMap::new();

    for name in &["cs2", "dota2", "factorio", "celeste", "deadcells", "rpcs3", "dolphin-emu", "yuzu", "ryujinx", "pcsx2", "retroarch"] {
        let mut stream = StreamInfo::default();
        stream.binary = name.to_string();
        assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Game, "Failed for {}", name);
    }
}

#[test]
fn test_classify_browsers() {
    let overrides = HashMap::new();

    for name in &["zen", "zen-bin", "firefox", "chromium", "google-chrome", "brave", "microsoft-edge", "opera", "vivaldi", "tor-browser", "librewolf"] {
        let mut stream = StreamInfo::default();
        stream.binary = name.to_string();
        assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Browser, "Failed for {}", name);
    }
}

#[test]
fn test_classify_music_players() {
    let overrides = HashMap::new();

    for name in &["spotify", "spotify-launcher", "cider", "strawberry", "audacious", "deadbeef", "rhythmbox", "tidal-hifi", "amberol", "foobar2000"] {
        let mut stream = StreamInfo::default();
        stream.binary = name.to_string();
        assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Music, "Failed for {}", name);
    }
}

#[test]
fn test_classify_chat_applications() {
    let overrides = HashMap::new();

    for name in &["discord", "vesktop", "webcord", "telegram-desktop", "slack", "teams-for-linux", "zoom", "skypeforlinux", "element", "mumble", "signal-desktop"] {
        let mut stream = StreamInfo::default();
        stream.binary = name.to_string();
        assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::Chat, "Failed for {}", name);
    }
}

#[test]
fn test_classify_production_and_soundboard() {
    let overrides = HashMap::new();

    let mut stream_obs = StreamInfo::default();
    stream_obs.binary = "obs".to_string();
    assert_eq!(AppClassifier::classify(&stream_obs, &overrides), AppCategory::Production);

    let mut stream_reaper = StreamInfo::default();
    stream_reaper.binary = "reaper".to_string();
    assert_eq!(AppClassifier::classify(&stream_reaper, &overrides), AppCategory::Production);

    let mut stream_sound = StreamInfo::default();
    stream_sound.binary = "soundux".to_string();
    assert_eq!(AppClassifier::classify(&stream_sound, &overrides), AppCategory::Soundboard);
}

#[test]
fn test_classify_fallback_default_to_system() {
    let overrides = HashMap::new();
    let mut stream = StreamInfo::default();
    stream.app_name = "alacritty".to_string();
    stream.binary = "alacritty".to_string();
    assert_eq!(AppClassifier::classify(&stream, &overrides), AppCategory::System);
}

#[test]
fn test_category_display_and_parsing() {
    let categories = [
        (AppCategory::Game, "Game", "game"),
        (AppCategory::Chat, "Chat", "chat"),
        (AppCategory::Music, "Music", "music"),
        (AppCategory::Browser, "Browser", "browser"),
        (AppCategory::Production, "Production", "production"),
        (AppCategory::Soundboard, "Soundboard", "soundboard"),
        (AppCategory::System, "System", "system"),
    ];

    for (cat, disp, parse_key) in categories {
        assert_eq!(format!("{}", cat), disp);
        assert_eq!(AppClassifier::parse_category_name(parse_key), Some(cat));
    }

    assert_eq!(AppClassifier::parse_category_name("voice"), Some(AppCategory::Chat));
    assert_eq!(AppClassifier::parse_category_name("daw"), Some(AppCategory::Production));
    assert_eq!(AppClassifier::parse_category_name("sample"), Some(AppCategory::Soundboard));
    assert_eq!(AppClassifier::parse_category_name("default"), Some(AppCategory::System));
    assert_eq!(AppClassifier::parse_category_name("unknown_nonexistent"), None);
}

#[test]
fn test_stream_info_constructor() {
    let s = StreamInfo::new(10, 20);
    assert_eq!(s.index, 10);
    assert_eq!(s.sink_index, 20);
    assert!(!s.is_corked);
    assert!(!s.is_muted);
}
