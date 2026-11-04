use crate::rules::{default_rules, Rule};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{info, warn};

use std::collections::HashMap;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error reading or writing config: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse TOML configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Failed to serialize TOML configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub devices: DevicesConfig,
    #[serde(default)]
    pub policies: PoliciesConfig,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default = "default_rules")]
    pub rules: Vec<Rule>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            daemon: DaemonConfig::default(),
            devices: DevicesConfig::default(),
            policies: PoliciesConfig::default(),
            routing: RoutingConfig::default(),
            rules: default_rules(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Topology check and stream policy evaluation interval (in milliseconds).
    pub check_interval_ms: u64,
    /// Whether to expose the org.sonomux.Daemon D-Bus service.
    pub dbus_enabled: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            check_interval_ms: 1500,
            dbus_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DevicesConfig {
    /// Path to the GoXLR utility IPC unix domain socket.
    pub goxlr_socket: String,
    /// Human-readable headset name for UI and logs.
    pub headset_name: String,
    /// ALSA sink name for the wireless headset.
    pub headset_sink: String,
    /// ALSA source name for the GoXLR submix (Line5 / Sampler).
    pub goxlr_mix_source: String,
    /// ALSA sink name for GoXLR Game channel.
    pub goxlr_game_sink: String,
    /// ALSA sink name for GoXLR System channel.
    pub goxlr_system_sink: String,
    /// ALSA sink name for GoXLR Chat channel.
    pub goxlr_chat_sink: String,
    /// ALSA sink name for GoXLR Music channel.
    pub goxlr_music_sink: String,
    /// ALSA sink name for GoXLR Sample channel.
    pub goxlr_sample_sink: String,
    /// Number of consecutive matching reads before confirming headset state transition.
    pub debounce_count: usize,
}

impl Default for DevicesConfig {
    fn default() -> Self {
        Self {
            goxlr_socket: "/tmp/goxlr.socket".to_string(),
            headset_name: "SteelSeries Arctis Nova 7".to_string(),
            headset_sink: "alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo".to_string(),
            goxlr_mix_source: "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source"
                .to_string(),
            goxlr_game_sink: "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink"
                .to_string(),
            goxlr_system_sink: "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink"
                .to_string(),
            goxlr_chat_sink: "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Headphones__sink"
                .to_string(),
            goxlr_music_sink: "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line2__sink"
                .to_string(),
            goxlr_sample_sink: "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line3__sink"
                .to_string(),
            debounce_count: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PoliciesConfig {
    /// Virtual Surround Manager input effect node name.
    pub surround_in_node: String,
    /// Virtual Surround Manager output effect node name.
    pub surround_out_node: String,
    /// Application names that should never be pulled into surround sound.
    pub keep_off_surround: Vec<String>,
    /// Loopback buffer latency target (in milliseconds).
    pub loopback_latency_ms: u32,
}

impl Default for PoliciesConfig {
    fn default() -> Self {
        Self {
            surround_in_node: "effect_input.virtual-surround-manager".to_string(),
            surround_out_node: "effect_output.virtual-surround-manager".to_string(),
            keep_off_surround: vec![
                "Zen".to_string(),
                "zen-bin".to_string(),
                "Chromium".to_string(),
                "Firefox".to_string(),
                "Spotify".to_string(),
                "Discord".to_string(),
            ],
            loopback_latency_ms: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingConfig {
    /// Master toggle for automatic stream classification and routing.
    pub enabled: bool,
    /// Destination channel per category ("Game", "Chat", "Music", "System", "Sample", or "Surround").
    pub category_targets: HashMap<String, String>,
    /// Exact binary or application name overrides mapped to a target channel name.
    pub app_overrides: HashMap<String, String>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        let mut category_targets = HashMap::new();
        category_targets.insert("game".to_string(), "Surround".to_string());
        category_targets.insert("chat".to_string(), "Chat".to_string());
        category_targets.insert("music".to_string(), "Music".to_string());
        category_targets.insert("browser".to_string(), "System".to_string());
        category_targets.insert("production".to_string(), "System".to_string());
        category_targets.insert("soundboard".to_string(), "Sample".to_string());
        category_targets.insert("system".to_string(), "System".to_string());

        let mut app_overrides = HashMap::new();
        app_overrides.insert("zen-bin".to_string(), "System".to_string());
        app_overrides.insert("spotify".to_string(), "Music".to_string());
        app_overrides.insert("discord".to_string(), "Chat".to_string());

        Self {
            enabled: true,
            category_targets,
            app_overrides,
        }
    }
}

impl AppConfig {
    /// Return the standard user config path: `$XDG_CONFIG_HOME/sonomux/config.toml`
    pub fn default_config_path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config")
            });
        base.join("sonomux").join("config.toml")
    }

    /// Load config from file, or automatically create default template if missing (hands-free installation).
    pub fn load_or_create() -> Result<(Self, PathBuf), ConfigError> {
        let user_path = Self::default_config_path();

        if user_path.exists() {
            let content = fs::read_to_string(&user_path)?;
            let config: AppConfig = toml::from_str(&content)?;
            return Ok((config, user_path));
        }

        // Check system-wide fallback (/etc/sonomux/config.toml)
        let sys_path = Path::new("/etc/sonomux/config.toml");
        if sys_path.exists() {
            let content = fs::read_to_string(sys_path)?;
            let config: AppConfig = toml::from_str(&content)?;
            return Ok((config, sys_path.to_path_buf()));
        }

        // First run: generate default config file
        let default_config = AppConfig::default();
        if let Some(parent) = user_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let toml_string = toml::to_string_pretty(&default_config)?;
        let header = "# ====================================================================\n\
                      # Sonomux Configuration File\n\
                      # Auto-generated on first launch. Modify values to suit your audio setup.\n\
                      # ====================================================================\n\n";
        let full_content = format!("{}{}", header, toml_string);

        if let Err(e) = fs::write(&user_path, full_content) {
            warn!(
                "Could not save default config to {}: {}",
                user_path.display(),
                e
            );
        } else {
            info!("Created default configuration at {}", user_path.display());
        }

        Ok((default_config, user_path))
    }

    /// Save current configuration back to default_config_path().
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::default_config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let toml_string = toml::to_string_pretty(self)?;
        let header = "# ====================================================================\n\
                      # Sonomux Audio Coordinator Configuration\n\
                      # ====================================================================\n\n";
        let full_content = format!("{}{}", header, toml_string);
        fs::write(&path, full_content)?;
        Ok(())
    }
}
