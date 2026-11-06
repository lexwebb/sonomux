//! Sonomux in the system menu bar: a menu bar extra on macOS, a StatusNotifierItem tray icon
//! on Linux (Waybar, KDE, ...). It mirrors the Omarchy widget: an icon with the speaker volume
//! or headset battery, the status lines, Open Sonomux, and Mute speakers.

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;

use sonomux_core::ipc::SonomuxDaemonProxy;
use sonomux_core::status::{summarize, Summary};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

/// What the menu asks the daemon to do.
#[derive(Debug)]
pub enum Cmd {
    ToggleMute,
    SetVolume(f64),
}

/// What the menu shows. `None` summary = daemon not reachable.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub summary: Option<Summary>,
    pub speaker_volume: f64,
}

/// Talk to the daemon forever: poll its status every second and run menu commands.
/// Reconnects when the daemon restarts. `show` is called whenever the view changes.
pub async fn daemon_loop(show: impl Fn(View), mut cmds: UnboundedReceiver<Cmd>) {
    let mut last: Option<View> = None;
    let mut update = |v: View| {
        if last.as_ref() != Some(&v) {
            show(v.clone());
            last = Some(v);
        }
    };
    loop {
        let proxy = match sonomux_core::ipc::connect().await {
            Ok(conn) => SonomuxDaemonProxy::new(&conn).await.ok().map(|p| (conn, p)),
            Err(_) => None,
        };
        let Some((_conn, proxy)) = proxy else {
            update(View::default());
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                Some(cmd) = cmds.recv() => {
                    let _ = match cmd {
                        Cmd::ToggleMute => proxy.toggle_speaker_mute().await.map(|_| ()),
                        Cmd::SetVolume(v) => proxy.set_speaker_volume(v).await,
                    };
                }
            }
            match proxy.get_status().await {
                Ok((goxlr, hs, hs_on, bat, spk, lp)) => update(View {
                    summary: Some(summarize(goxlr, &hs, hs_on, bat, spk, lp)),
                    speaker_volume: spk,
                }),
                Err(_) => break, // daemon went away: reconnect
            }
        }
    }
}

/// Open the Sonomux window. The GUI focuses an existing window itself on Hyprland; elsewhere
/// we skip launching a second one.
pub fn open_gui() {
    // ponytail: on macOS a second launch is skipped, not focused; add focusing when the GUI
    // becomes an .app bundle.
    if cfg!(target_os = "macos")
        && std::process::Command::new("pgrep")
            .args(["-x", "sonomux"])
            .status()
            .is_ok_and(|s| s.success())
    {
        return;
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("sonomux")))
        .filter(|p| p.exists());
    let bin = sibling.unwrap_or_else(|| "sonomux".into());
    if let Err(e) = std::process::Command::new(&bin).spawn() {
        tracing::warn!("Could not open {}: {}", bin.display(), e);
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    #[cfg(target_os = "macos")]
    return mac::run();
    #[cfg(not(target_os = "macos"))]
    return linux::run();
}
