#[cfg(test)]
mod tests {
    use sonomux_core::config::*;

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.daemon.check_interval_ms, 1500);
        assert!(cfg.daemon.dbus_enabled);
        assert_eq!(cfg.devices.debounce_count, 2);
        assert_eq!(cfg.devices.goxlr_socket, "/tmp/goxlr.socket");
        assert_eq!(cfg.policies.loopback_latency_ms, 20);
        assert_eq!(cfg.policies.keep_off_surround, vec!["Zen".to_string()]);
    }

    #[test]
    fn test_toml_serialization_roundtrip() {
        let original = AppConfig::default();
        let toml_str = toml::to_string(&original).expect("Serialization failed");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("Deserialization failed");

        assert_eq!(parsed.daemon.check_interval_ms, original.daemon.check_interval_ms);
        assert_eq!(parsed.devices.headset_name, original.devices.headset_name);
        assert_eq!(parsed.policies.keep_off_surround, original.policies.keep_off_surround);
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
        assert_eq!(cfg.policies.keep_off_surround, vec!["Firefox".to_string(), "Discord".to_string()]);
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
        // Rest should default
        assert!(cfg.daemon.dbus_enabled);
        assert_eq!(cfg.devices.goxlr_socket, "/tmp/goxlr.socket");
        assert_eq!(cfg.policies.loopback_latency_ms, 20);
    }
}
