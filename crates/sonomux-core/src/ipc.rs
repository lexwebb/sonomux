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
