//! How the CLI and GUI reach `sonomuxd`.
//!
//! Linux uses the D-Bus session bus. macOS has no session bus by default, so the daemon serves
//! the same zbus interface peer-to-peer over a Unix socket instead.

pub const BUS_NAME: &str = "org.sonomux.Daemon";
pub const OBJECT_PATH: &str = "/org/sonomux/Daemon";

/// Daemon socket on macOS: `$XDG_STATE_HOME/sonomux/sonomuxd.sock` (default `~/.local/state`).
pub fn socket_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            std::path::PathBuf::from(home).join(".local/state")
        });
    base.join("sonomux").join("sonomuxd.sock")
}

/// Connect to the daemon's IPC endpoint for this OS.
#[cfg(not(target_os = "macos"))]
pub async fn connect() -> zbus::Result<zbus::Connection> {
    zbus::Connection::session().await
}

/// Connect to the daemon's IPC endpoint for this OS.
#[cfg(target_os = "macos")]
pub async fn connect() -> zbus::Result<zbus::Connection> {
    let stream = std::os::unix::net::UnixStream::connect(socket_path())?;
    zbus::connection::Builder::async_io_unix_stream(stream)
        .p2p()
        .build()
        .await
}

/// Client proxy for the daemon's `org.sonomux.Daemon` interface. Keep it in step with
/// `sonomux-daemon/src/dbus_service.rs`: every daemon method belongs here.
#[zbus::proxy(
    interface = "org.sonomux.Daemon",
    default_service = "org.sonomux.Daemon",
    default_path = "/org/sonomux/Daemon"
)]
pub trait SonomuxDaemon {
    /// `(goxlr_active, headset_status, headset_active, battery_pct, speaker_volume, loopback_active)`
    async fn get_status(&self) -> zbus::Result<(bool, String, bool, u8, f64, bool)>;
    async fn get_channel_volumes(&self) -> zbus::Result<std::collections::HashMap<String, f64>>;
    async fn set_channel_volume(&self, channel: String, volume_pct: f64) -> zbus::Result<()>;
    async fn set_speaker_volume(&self, volume_pct: f64) -> zbus::Result<()>;
    async fn toggle_speaker_mute(&self) -> zbus::Result<bool>;
    async fn enforce_policies(&self) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)] // tuple mirrors the daemon's D-Bus signature
    async fn get_routed_streams(&self) -> zbus::Result<Vec<(u64, String, String, String, String)>>;
    async fn get_routing_config(
        &self,
    ) -> zbus::Result<(bool, std::collections::HashMap<String, String>)>;
    async fn set_routing_enabled(&self, enabled: bool) -> zbus::Result<()>;
    async fn set_category_target(&self, category: String, target: String) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)]
    async fn get_apps(&self) -> zbus::Result<Vec<(String, String, String, String, bool)>>;
    async fn set_app_override(&self, id: String, target: String) -> zbus::Result<()>;
    async fn get_channel_mutes(&self) -> zbus::Result<std::collections::HashMap<String, bool>>;
    async fn set_channel_muted(&self, channel: String, muted: bool) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)]
    async fn get_audio_devices(&self) -> zbus::Result<(Vec<String>, Vec<String>, String, String)>;
    async fn get_device_settings(&self) -> zbus::Result<(bool, bool)>;
    async fn set_default_device(&self, name: String, input: bool) -> zbus::Result<()>;
    async fn set_keep_defaults(&self, keep: bool) -> zbus::Result<()>;
    async fn set_split_unpaired(&self, enabled: bool) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)]
    async fn get_split_devices(
        &self,
    ) -> zbus::Result<(Vec<(String, String, String, String)>, Vec<(String, String)>)>;
    async fn get_rules(&self) -> zbus::Result<Vec<(String, bool)>>;
    async fn set_rule_enabled(&self, rule_name: String, enabled: bool) -> zbus::Result<()>;
    /// `(enabled, profile, available_profiles)`
    async fn get_surround_info(&self) -> zbus::Result<(bool, String, Vec<String>)>;
    async fn set_surround_enabled(&self, enabled: bool) -> zbus::Result<()>;
    async fn set_surround_profile(&self, profile: String) -> zbus::Result<()>;
}
