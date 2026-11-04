use slint::slint;
use std::time::Duration;
use tracing::info;
use zbus::proxy;

slint! {
    import { Button, Slider, VerticalBox, HorizontalBox, GroupBox } from "std-widgets.slint";

    export component MainWindow inherits Window {
        title: "Sonomux Audio Coordinator";
        min-width: 520px;
        min-height: 600px;
        background: #181926;

        in-out property <string> daemon_status: "Connecting...";
        in-out property <color> daemon_color: #eed49f;
        in-out property <string> goxlr_status: "Checking...";
        in-out property <string> goxlr_serial: "--";
        in-out property <float> speaker_volume: 100.0;
        in-out property <string> headset_status: "Standby (Off)";
        in-out property <string> headset_battery: "Unavailable";
        in-out property <string> routing_state: "Auto (PipeWire)";

        callback volume_changed(float);
        callback toggle_speakers();
        callback force_routing_refresh();

        VerticalBox {
            padding: 20px;
            spacing: 16px;

            // Header
            HorizontalBox {
                alignment: space-between;
                VerticalBox {
                    spacing: 2px;
                    Text {
                        text: "Sonomux 🎛️";
                        font-size: 22px;
                        font-weight: 700;
                        color: #cad3f5;
                    }
                    Text {
                        text: "Unified Audio Coordinator";
                        font-size: 12px;
                        color: #8087a2;
                    }
                }
                VerticalBox {
                    alignment: center;
                    HorizontalBox {
                        spacing: 8px;
                        Rectangle {
                            width: 10px;
                            height: 10px;
                            border-radius: 5px;
                            background: root.daemon_color;
                            y: 4px;
                        }
                        Text {
                            text: root.daemon_status;
                            font-size: 13px;
                            color: root.daemon_color;
                            font-weight: 600;
                        }
                    }
                }
            }

            // GoXLR Card
            Rectangle {
                background: #24273a;
                border-radius: 12px;
                border-width: 1px;
                border-color: #363a4f;

                VerticalBox {
                    padding: 14px;
                    spacing: 10px;

                    HorizontalBox {
                        alignment: space-between;
                        Text {
                            text: "TC-Helicon GoXLR Mini";
                            font-size: 16px;
                            font-weight: 600;
                            color: #c6a0f6;
                        }
                        Text {
                            text: root.goxlr_status;
                            font-size: 13px;
                            color: #a6da95;
                            font-weight: 500;
                        }
                    }

                    HorizontalBox {
                        Text {
                            text: "Device Serial: " + root.goxlr_serial;
                            font-size: 12px;
                            color: #a5adcb;
                        }
                    }

                    HorizontalBox {
                        Text {
                            text: "Desk Speakers (Headphone Jack)";
                            font-size: 13px;
                            color: #cad3f5;
                            vertical-alignment: center;
                        }
                    }

                    HorizontalBox {
                        Slider {
                            minimum: 0.0;
                            maximum: 100.0;
                            value: root.speaker_volume;
                            changed(val) => {
                                root.volume_changed(val);
                            }
                        }
                        Text {
                            text: Math.round(root.speaker_volume) + "%";
                            font-size: 13px;
                            color: #eed49f;
                            min-width: 45px;
                            horizontal-alignment: right;
                            vertical-alignment: center;
                        }
                        Button {
                            text: "Mute/Unmute";
                            clicked => { root.toggle_speakers(); }
                        }
                    }
                }
            }

            // Headset Card
            Rectangle {
                background: #24273a;
                border-radius: 12px;
                border-width: 1px;
                border-color: #363a4f;

                VerticalBox {
                    padding: 14px;
                    spacing: 10px;

                    HorizontalBox {
                        alignment: space-between;
                        Text {
                            text: "Arctis Nova 7 (Wireless)";
                            font-size: 16px;
                            font-weight: 600;
                            color: #8aadf4;
                        }
                        Text {
                            text: root.headset_status;
                            font-size: 13px;
                            color: #eed49f;
                            font-weight: 500;
                        }
                    }

                    HorizontalBox {
                        Text {
                            text: "Battery: " + root.headset_battery;
                            font-size: 13px;
                            color: #cad3f5;
                        }
                    }

                    Text {
                        text: "Presence detected via low-level HID battery telemetry to prevent ghost-dongle misrouting.";
                        font-size: 11px;
                        color: #6e738d;
                    }
                }
            }

            // Audio Stream Policy Card
            Rectangle {
                background: #24273a;
                border-radius: 12px;
                border-width: 1px;
                border-color: #363a4f;

                VerticalBox {
                    padding: 14px;
                    spacing: 8px;

                    Text {
                        text: "Stream Routing Policies";
                        font-size: 14px;
                        font-weight: 600;
                        color: #f5bde6;
                    }

                    Text {
                        text: "• Virtual Surround Manager -> GoXLR Game";
                        font-size: 12px;
                        color: #a5adcb;
                    }
                    Text {
                        text: "• Browser (Zen) -> Isolated on GoXLR System";
                        font-size: 12px;
                        color: #a5adcb;
                    }
                    Text {
                        text: "• Headset Loopback: " + root.routing_state;
                        font-size: 12px;
                        color: #a5adcb;
                    }

                    HorizontalBox {
                        alignment: end;
                        Button {
                            text: "Enforce Policies Now";
                            clicked => { root.force_routing_refresh(); }
                        }
                    }
                }
            }
        }
    }
}

#[proxy(
    interface = "org.sonomux.Daemon",
    default_service = "org.sonomux.Daemon",
    default_path = "/org/sonomux/Daemon"
)]
trait SonomuxDaemon {
    async fn get_status(&self) -> zbus::Result<(bool, String, bool, u8, f64, bool)>;
    async fn set_speaker_volume(&self, volume: f64) -> zbus::Result<()>;
    async fn toggle_speaker_mute(&self) -> zbus::Result<bool>;
    async fn enforce_policies(&self) -> zbus::Result<()>;
}

/// Searches Hyprland's clients JSON for an existing Sonomux window address.
pub fn find_existing_hyprland_address(clients_json: &str) -> Option<String> {
    let clients: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let array = clients.as_array()?;
    for client in array {
        let title = client.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let init_title = client.get("initialTitle").and_then(|v| v.as_str()).unwrap_or("");
        let class = client.get("class").and_then(|v| v.as_str()).unwrap_or("");
        let init_class = client.get("initialClass").and_then(|v| v.as_str()).unwrap_or("");

        if title == "Sonomux Audio Coordinator"
            || init_title == "Sonomux Audio Coordinator"
            || class == "sonomux"
            || init_class == "sonomux"
        {
            if let Some(addr) = client.get("address").and_then(|v| v.as_str()) {
                return Some(addr.to_string());
            }
        }
    }
    None
}

/// Attempts to focus an existing window if Sonomux is already running.
/// Returns true if an existing window was found and focused.
pub fn try_focus_existing_instance() -> bool {
    // 1. Hyprland compositor
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Ok(output) = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
        {
            if output.status.success() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Some(addr) = find_existing_hyprland_address(&json_str) {
                        info!("Found existing Sonomux window in Hyprland at {}, focusing...", addr);
                        // Try modern Lua dispatch first (Hyprland >= 0.56)
                        let lua_cmd = format!("hl.dsp.focus({{ window = \"address:{}\" }})", addr);
                        let status = std::process::Command::new("hyprctl")
                            .args(["dispatch", &lua_cmd])
                            .status();

                        if let Ok(s) = status {
                            if s.success() {
                                return true;
                            }
                        }

                        // Fallback to classic focuswindow
                        let _ = std::process::Command::new("hyprctl")
                            .args(["dispatch", "focuswindow", &format!("address:{}", addr)])
                            .status();
                        return true;
                    }
                }
            }
        }
    }

    // 2. Generic X11 / xdotool fallback
    if let Ok(status) = std::process::Command::new("xdotool")
        .args(["search", "--name", "Sonomux Audio Coordinator", "windowactivate"])
        .status()
    {
        if status.success() {
            info!("Found existing Sonomux window via xdotool, focused.");
            return true;
        }
    }

    false
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    if try_focus_existing_instance() {
        info!("Existing Sonomux instance focused. Exiting duplicate process.");
        return Ok(());
    }

    info!("Starting Sonomux Slint GUI...");
    let main_window = MainWindow::new()?;

    let goxlr = sonomux_device_goxlr::GoXLRClient::new(sonomux_device_goxlr::DEFAULT_GOXLR_SOCKET);

    // Initial GoXLR hardware check
    if let Ok(serial) = goxlr.get_serial().await {
        main_window.set_goxlr_status("Active (Connected)".into());
        main_window.set_goxlr_serial(serial.into());
    } else {
        main_window.set_goxlr_status("Disconnected".into());
    }

    // Try connecting to daemon over session D-Bus
    let connection = zbus::Connection::session().await.ok();
    let dbus_proxy = if let Some(ref conn) = connection {
        SonomuxDaemonProxy::new(conn).await.ok()
    } else {
        None
    };

    let has_daemon = dbus_proxy.is_some();
    if has_daemon {
        main_window.set_daemon_status("Daemon: Connected".into());
        main_window.set_daemon_color(slint::Color::from_rgb_u8(166, 218, 149));
    } else {
        main_window.set_daemon_status("Daemon: Standalone".into());
        main_window.set_daemon_color(slint::Color::from_rgb_u8(165, 173, 203));
    }

    // Spawn periodic state polling (either through D-Bus or direct hardware)
    let handle = main_window.as_weak();
    let proxy_for_poll = dbus_proxy.clone();
    let goxlr_for_poll = goxlr.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        loop {
            interval.tick().await;

            if let Some(ref proxy) = proxy_for_poll {
                if let Ok((goxlr_up, hs_status, hs_active, battery, spk_vol, loop_up)) =
                    proxy.get_status().await
                {
                    let weak = handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_daemon_status("Daemon: Connected".into());
                            ui.set_daemon_color(slint::Color::from_rgb_u8(166, 218, 149));
                            ui.set_goxlr_status(if goxlr_up {
                                "Active (Connected)".into()
                            } else {
                                "Disconnected".into()
                            });
                            ui.set_headset_status(if hs_active {
                                format!("Active ({})", hs_status).into()
                            } else {
                                hs_status.into()
                            });
                            ui.set_headset_battery(if hs_active && battery > 0 {
                                format!("{}%", battery).into()
                            } else if hs_active {
                                "Connected".into()
                            } else {
                                "Unavailable".into()
                            });
                            ui.set_speaker_volume(spk_vol as f32);
                            ui.set_routing_state(if loop_up {
                                "Active (Routing GoXLR -> Headset)".into()
                            } else {
                                "Idle (Headset Standby)".into()
                            });
                        }
                    });
                    continue;
                }
            }

            // Standalone fallback polling
            let weak = handle.clone();
            let goxlr_client = goxlr_for_poll.clone();
            let hp_vol = goxlr_client.get_headphones_volume().await.ok();
            let telemetry = sonomux_device_headset::HeadsetMonitor::probe_raw()
                .await
                .ok();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_daemon_status("Daemon: Standalone".into());
                    ui.set_daemon_color(slint::Color::from_rgb_u8(165, 173, 203));
                    if let Some(vol) = hp_vol {
                        ui.set_speaker_volume((vol as f32 / 255.0) * 100.0);
                    }
                    if let Some(t) = telemetry {
                        ui.set_headset_status(if t.is_active {
                            "Active (Powered On)".into()
                        } else {
                            "Standby (Headset Off)".into()
                        });
                        ui.set_headset_battery(if let Some(b) = t.battery_percent {
                            format!("{}%", b).into()
                        } else if t.is_active {
                            "Connected".into()
                        } else {
                            "Unavailable".into()
                        });
                    }
                }
            });
        }
    });

    // Callbacks
    let proxy_for_vol = dbus_proxy.clone();
    let goxlr_for_vol = goxlr.clone();
    main_window.on_volume_changed(move |val| {
        let proxy = proxy_for_vol.clone();
        let client = goxlr_for_vol.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                if p.set_speaker_volume(val as f64).await.is_ok() {
                    return;
                }
            }
            let raw = ((val / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let _ = client
                .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, raw)
                .await;
        });
    });

    let proxy_for_mute = dbus_proxy.clone();
    let goxlr_for_mute = goxlr.clone();
    let weak_mute = main_window.as_weak();
    main_window.on_toggle_speakers(move || {
        let proxy = proxy_for_mute.clone();
        let client = goxlr_for_mute.clone();
        let weak = weak_mute.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                if let Ok(is_muted) = p.toggle_speaker_mute().await {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_speaker_volume(if is_muted { 0.0 } else { 100.0 });
                        }
                    });
                    return;
                }
            }
            if let Ok(curr) = client.get_headphones_volume().await {
                let target = if curr > 0 { 0 } else { 255 };
                let _ = client
                    .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, target)
                    .await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_speaker_volume(if target > 0 { 100.0 } else { 0.0 });
                    }
                });
            }
        });
    });

    let proxy_for_refresh = dbus_proxy.clone();
    main_window.on_force_routing_refresh(move || {
        let proxy = proxy_for_refresh.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                if p.enforce_policies().await.is_ok() {
                    info!("Successfully requested daemon to enforce policies.");
                    return;
                }
            }
            let _ = sonomux_backend::PulseManager::enforce_surround_policies(
                "effect_output.virtual-surround-manager",
                "effect_input.virtual-surround-manager",
                "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink",
                "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Speaker__sink",
                &["Zen"],
            )
            .await;
        });
    });

    // Run Slint event loop
    main_window.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_existing_hyprland_address_matches_title() {
        let json = r#"[
            {
                "address": "0x580f2432bce0",
                "class": "discord",
                "title": "Discord",
                "initialTitle": "Discord"
            },
            {
                "address": "0x580f2480a390",
                "class": "",
                "title": "Sonomux Audio Coordinator",
                "initialTitle": "Sonomux Audio Coordinator"
            }
        ]"#;

        let addr = find_existing_hyprland_address(json);
        assert_eq!(addr, Some("0x580f2480a390".to_string()));
    }

    #[test]
    fn test_find_existing_hyprland_address_matches_class() {
        let json = r#"[
            {
                "address": "0x12345678",
                "class": "sonomux",
                "title": "Audio Settings",
                "initialTitle": "Audio Settings"
            }
        ]"#;

        let addr = find_existing_hyprland_address(json);
        assert_eq!(addr, Some("0x12345678".to_string()));
    }

    #[test]
    fn test_find_existing_hyprland_address_none_when_empty_or_different() {
        let json = r#"[
            {
                "address": "0x580f2432bce0",
                "class": "discord",
                "title": "Discord"
            }
        ]"#;

        assert_eq!(find_existing_hyprland_address(json), None);
        assert_eq!(find_existing_hyprland_address("[]"), None);
        assert_eq!(find_existing_hyprland_address("invalid json"), None);
    }
}
