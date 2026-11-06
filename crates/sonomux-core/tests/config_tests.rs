use sonomux_core::config::*;
use std::env;
use std::fs;
use std::sync::Mutex;

/// Tests that change HOME / XDG_CONFIG_HOME take this, so they don't race each other.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_default_config_values() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.daemon.check_interval_ms, 1500);
    assert!(cfg.daemon.dbus_enabled);
    assert_eq!(cfg.devices.debounce_count, 2);
    assert_eq!(cfg.devices.goxlr_socket, "/tmp/goxlr.socket");
    #[cfg(not(target_os = "macos"))]
    {
        assert!(cfg.devices.goxlr_chat_sink.contains("Headphones"));
        assert!(cfg.devices.goxlr_music_sink.contains("Line2"));
        assert!(cfg.devices.goxlr_sample_sink.contains("Line3"));
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(cfg.devices.goxlr_chat_sink, "GoXLRMini Chat");
        assert_eq!(cfg.devices.goxlr_music_sink, "GoXLRMini Music");
        assert_eq!(cfg.devices.goxlr_mix_source, "GoXLRMini Sample Source Mix");
    }
    assert_eq!(cfg.policies.loopback_latency_ms, 20);
    assert!(cfg.policies.keep_off_surround.contains(&"Zen".to_string()));
    assert!(cfg.routing.enabled);
    assert_eq!(
        cfg.routing.category_targets.get("browser"),
        Some(&"System".to_string())
    );
    assert_eq!(
        cfg.routing.category_targets.get("game"),
        Some(&"Surround".to_string())
    );
}

#[test]
fn test_toml_serialization_roundtrip() {
    let original = AppConfig::default();
    let toml_str = toml::to_string(&original).expect("Serialization failed");
    let parsed: AppConfig = toml::from_str(&toml_str).expect("Deserialization failed");

    assert_eq!(
        parsed.daemon.check_interval_ms,
        original.daemon.check_interval_ms
    );
    assert_eq!(parsed.devices.headset_name, original.devices.headset_name);
    assert_eq!(
        parsed.policies.keep_off_surround,
        original.policies.keep_off_surround
    );
}

#[test]
fn test_custom_toml_parsing() {
    let custom_toml = r#"
        [daemon]
        check_interval_ms = 3000
        dbus_enabled = false

        [devices]
        goxlr_socket = "/custom/socket.sock"
        headset_name = "Custom Headset"
        headset_sink = "custom_sink"
        goxlr_mix_source = "custom_source"
        goxlr_game_sink = "custom_game"
        goxlr_system_sink = "custom_system"
        debounce_count = 5

        [policies]
        surround_in_node = "surround_in"
        surround_out_node = "surround_out"
        keep_off_surround = ["Firefox", "Discord"]
        loopback_latency_ms = 10
    "#;

    let cfg: AppConfig = toml::from_str(custom_toml).expect("Failed to parse custom TOML");
    assert_eq!(cfg.daemon.check_interval_ms, 3000);
    assert!(!cfg.daemon.dbus_enabled);
    assert_eq!(cfg.devices.debounce_count, 5);
    assert_eq!(cfg.devices.goxlr_socket, "/custom/socket.sock");
    assert_eq!(
        cfg.policies.keep_off_surround,
        vec!["Firefox".to_string(), "Discord".to_string()]
    );
    assert_eq!(cfg.policies.loopback_latency_ms, 10);
}

#[test]
fn test_partial_toml_uses_defaults() {
    let partial_toml = r#"
        [daemon]
        check_interval_ms = 500
    "#;

    let cfg: AppConfig = toml::from_str(partial_toml).expect("Failed to parse partial TOML");
    assert_eq!(cfg.daemon.check_interval_ms, 500);
    assert!(cfg.daemon.dbus_enabled);
    assert_eq!(cfg.devices.goxlr_socket, "/tmp/goxlr.socket");
    assert_eq!(cfg.policies.loopback_latency_ms, 20);
}

#[test]
fn test_load_or_create_generates_file() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = env::temp_dir().join(format!("sonomux-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    // Point XDG_CONFIG_HOME to temp_dir
    unsafe {
        env::set_var("XDG_CONFIG_HOME", &temp_dir);
    }

    let (cfg, path) = AppConfig::load_or_create().expect("Failed load_or_create");
    assert!(path.exists());
    assert_eq!(cfg.daemon.check_interval_ms, 1500);

    // Second call loads existing file
    let (cfg2, path2) = AppConfig::load_or_create().expect("Failed second load");
    assert_eq!(path, path2);
    assert_eq!(cfg2.devices.goxlr_socket, "/tmp/goxlr.socket");

    // save() writes edits back to the same path
    let mut edited = cfg2;
    edited.daemon.check_interval_ms = 4242;
    edited.save().expect("Failed save");
    let (cfg3, _) = AppConfig::load_or_create().expect("Failed reload");
    assert_eq!(cfg3.daemon.check_interval_ms, 4242);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_default_config_path_fallback() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_home = env::temp_dir().join(format!("sonomux-home-{}", std::process::id()));
    unsafe {
        env::remove_var("XDG_CONFIG_HOME");
        env::set_var("HOME", &temp_home);
    }
    let path = AppConfig::default_config_path();
    assert_eq!(
        path,
        temp_home
            .join(".config")
            .join("sonomux")
            .join("config.toml")
    );
}

#[test]
fn test_config_error_display() {
    let io_err = ConfigError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file not found",
    ));
    assert!(format!("{}", io_err).contains("I/O error"));

    let toml_err = ConfigError::Parse(toml::from_str::<AppConfig>("invalid = toml [").unwrap_err());
    assert!(format!("{}", toml_err).contains("Failed to parse TOML"));
}

/// Fresh temp XDG_CONFIG_HOME for one test. Caller holds ENV_LOCK.
fn isolated_config_home(tag: &str) -> std::path::PathBuf {
    let dir = env::temp_dir().join(format!("sonomux-cfg-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    unsafe {
        env::set_var("XDG_CONFIG_HOME", &dir);
    }
    dir
}

#[test]
fn test_save_is_atomic_and_private() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = isolated_config_home("atomic");
    let (mut cfg, path) = AppConfig::load_or_create().unwrap();
    cfg.daemon.check_interval_ms = 3333;
    cfg.save().unwrap();
    let (reloaded, _) = AppConfig::load_or_create().unwrap();
    assert_eq!(reloaded.daemon.check_interval_ms, 3333);
    assert!(!path.parent().unwrap().join(".config.toml.tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_save_keeps_symlink() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = isolated_config_home("symlink");
    let real = dir.join("real-config.toml");
    fs::write(&real, "[daemon]\ncheck_interval_ms = 2000\n").unwrap();
    let link = AppConfig::default_config_path();
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let (mut cfg, _) = AppConfig::load_or_create().unwrap();
    assert_eq!(cfg.daemon.check_interval_ms, 2000);
    cfg.daemon.check_interval_ms = 2468;
    cfg.save().unwrap();

    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::read_to_string(&real)
        .unwrap()
        .contains("check_interval_ms = 2468"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_zero_interval_is_clamped() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = isolated_config_home("zero");
    let path = AppConfig::default_config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[daemon]\ncheck_interval_ms = 0\n").unwrap();
    let (cfg, _) = AppConfig::load_or_create().unwrap();
    assert_eq!(cfg.daemon.check_interval_ms, MIN_CHECK_INTERVAL_MS);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_parse_error_names_file() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = isolated_config_home("badparse");
    let path = AppConfig::default_config_path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "invalid = toml [").unwrap();
    let err = AppConfig::load_or_create().expect_err("broken file must fail");
    assert!(err.to_string().contains("config.toml"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid = toml [");
    let _ = fs::remove_dir_all(&dir);
}
