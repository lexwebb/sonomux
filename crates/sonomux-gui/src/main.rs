use slint::slint;
use std::time::Duration;
use tracing::info;
use zbus::proxy;

slint! {
    import { Button, Slider, VerticalBox, HorizontalBox } from "std-widgets.slint";

    component SidebarItem inherits Rectangle {
        in-out property <string> text;
        in-out property <string> icon;
        in-out property <bool> active: false;
        callback clicked();

        height: 40px;
        border-radius: 8px;
        background: root.active ? #363a4f : (touch.has-hover ? #24273a : transparent);

        HorizontalLayout {
            padding-left: 14px;
            padding-right: 14px;
            spacing: 12px;

            Text {
                text: root.icon;
                font-size: 15px;
                color: root.active ? #8aadf4 : #a5adcb;
                vertical-alignment: center;
            }

            Text {
                text: root.text;
                font-size: 13px;
                font-weight: root.active ? 700 : 500;
                color: root.active ? #cad3f5 : #a5adcb;
                vertical-alignment: center;
            }
        }

        touch := TouchArea {
            clicked => { root.clicked(); }
        }
    }

    component ChannelFader inherits Rectangle {
        in-out property <float> value: 100.0;
        in-out property <color> fill_color: #8aadf4;
        in-out property <bool> is_muted: false;
        callback changed(float);

        width: 32px;
        height: 200px;
        background: #11121d;
        border-radius: 6px;
        border-width: 1px;
        border-color: #363a4f;
        clip: true;

        Rectangle {
            y: parent.height - self.height;
            width: parent.width;
            height: Math.max(0px, parent.height * (root.value / 100.0));
            background: root.is_muted ? #ed8796 : root.fill_color;
            border-radius: 4px;
        }

        Rectangle {
            y: (parent.height - self.height) * (1.0 - (root.value / 100.0));
            width: parent.width + 4px;
            x: -2px;
            height: 12px;
            border-radius: 3px;
            background: #f4dbd6;
            border-width: 1px;
            border-color: #181926;
        }

        TouchArea {
            moved => {
                if (self.pressed) {
                    let rel = 1.0 - (self.mouse-y / parent.height);
                    let clamped = Math.max(0.0, Math.min(1.0, rel));
                    root.value = clamped * 100.0;
                    root.changed(root.value);
                }
            }
            clicked => {
                let rel = 1.0 - (self.mouse-y / parent.height);
                let clamped = Math.max(0.0, Math.min(1.0, rel));
                root.value = clamped * 100.0;
                root.changed(root.value);
            }
        }
    }

    component ChannelStrip inherits Rectangle {
        in-out property <string> name;
        in-out property <string> tag;
        in-out property <color> tag_color: #8aadf4;
        in-out property <float> volume: 100.0;
        in-out property <bool> muted: false;
        in-out property <bool> highlight: false;
        callback volume_changed(float);
        callback toggle_mute();

        background: #1e2030;
        border-radius: 12px;
        border-width: 1px;
        border-color: root.highlight ? #c6a0f6 : #363a4f;

        VerticalLayout {
            padding: 10px;
            spacing: 8px;
            alignment: space-between;

            // Tag badge
            HorizontalLayout {
                alignment: center;
                Rectangle {
                    background: #24273a;
                    border-radius: 4px;
                    border-width: 1px;
                    border-color: root.tag_color;
                    height: 18px;
                    HorizontalLayout {
                        padding-left: 6px;
                        padding-right: 6px;
                        Text {
                            text: root.tag;
                            font-size: 9px;
                            font-weight: 700;
                            color: root.tag_color;
                            vertical-alignment: center;
                        }
                    }
                }
            }

            // Name
            Text {
                text: root.name;
                font-size: 13px;
                font-weight: 700;
                color: #cad3f5;
                horizontal-alignment: center;
            }

            // Vertical Fader
            HorizontalLayout {
                alignment: center;
                ChannelFader {
                    value: root.volume;
                    fill_color: root.tag_color;
                    is_muted: root.muted;
                    changed(val) => {
                        root.volume = val;
                        root.volume_changed(val);
                    }
                }
            }

            // Numeric readout
            Text {
                text: root.muted ? "MUTED" : Math.round(root.volume) + "%";
                font-size: 12px;
                font-weight: 700;
                color: root.muted ? #ed8796 : #eed49f;
                horizontal-alignment: center;
            }

            // Mute Button
            Button {
                text: root.muted ? "MUTED" : "MUTE";
                clicked => { root.toggle_mute(); }
            }
        }
    }

    export component MainWindow inherits Window {
        title: "Sonomux Audio Coordinator";
        min-width: 900px;
        min-height: 600px;
        background: #181926;

        in-out property <int> active_tab: 0;

        in-out property <string> daemon_status: "Connecting...";
        in-out property <color> daemon_color: #eed49f;
        in-out property <string> goxlr_status: "Checking...";
        in-out property <string> goxlr_serial: "--";
        in-out property <float> speaker_volume: 100.0;
        in-out property <float> headset_volume: 80.0;
        in-out property <float> system_volume: 100.0;
        in-out property <float> game_volume: 85.0;
        in-out property <float> chat_volume: 90.0;
        in-out property <float> music_volume: 70.0;

        in-out property <bool> speakers_muted: false;
        in-out property <bool> headset_active: false;
        in-out property <string> headset_status: "Standby (Off)";
        in-out property <string> headset_battery: "Unavailable";
        in-out property <string> routing_state: "Auto (PipeWire)";

        callback volume_changed(float);
        callback toggle_speakers();
        callback force_routing_refresh();

        HorizontalLayout {
            // Left Navigation Sidebar
            Rectangle {
                width: 190px;
                background: #1e2030;
                border-width: 1px;
                border-color: #363a4f;

                VerticalLayout {
                    padding-top: 16px;
                    padding-bottom: 16px;
                    padding-left: 10px;
                    padding-right: 10px;
                    spacing: 6px;

                    // Header
                    HorizontalLayout {
                        spacing: 8px;
                        padding-left: 8px;
                        padding-bottom: 12px;
                        Text {
                            text: "🎛️";
                            font-size: 20px;
                            vertical-alignment: center;
                        }
                        VerticalLayout {
                            alignment: center;
                            spacing: 1px;
                            Text {
                                text: "Sonomux";
                                font-size: 16px;
                                font-weight: 800;
                                color: #cad3f5;
                            }
                            Text {
                                text: "Audio Coordinator";
                                font-size: 10px;
                                color: #8087a2;
                            }
                        }
                    }

                    Rectangle {
                        height: 1px;
                        background: #363a4f;
                    }

                    // Navigation Tabs
                    SidebarItem {
                        icon: "󰓃";
                        text: "Mixer Deck";
                        active: root.active_tab == 0;
                        clicked => { root.active_tab = 0; }
                    }

                    SidebarItem {
                        icon: "󰋋";
                        text: "Devices";
                        active: root.active_tab == 1;
                        clicked => { root.active_tab = 1; }
                    }

                    SidebarItem {
                        icon: "󰒓";
                        text: "Rule Studio";
                        active: root.active_tab == 2;
                        clicked => { root.active_tab = 2; }
                    }

                    SidebarItem {
                        icon: "󰑭";
                        text: "Policies";
                        active: root.active_tab == 3;
                        clicked => { root.active_tab = 3; }
                    }

                    // Bottom metadata
                    Rectangle { vertical-stretch: 1; }

                    Rectangle {
                        height: 1px;
                        background: #363a4f;
                    }

                    VerticalLayout {
                        padding-top: 8px;
                        padding-left: 8px;
                        spacing: 2px;
                        Text {
                            text: "v0.1.0 • Slint Native";
                            font-size: 10px;
                            color: #6e738d;
                        }
                        Text {
                            text: "GPL-3.0 Open Source";
                            font-size: 9px;
                            color: #5b6078;
                        }
                    }
                }
            }

            // Main Content Area
            Rectangle {
                horizontal-stretch: 1;
                background: #181926;

                VerticalLayout {
                    padding-top: 18px;
                    padding-bottom: 18px;
                    padding-left: 22px;
                    padding-right: 22px;
                    spacing: 16px;

                    // Top Bar with Headset Battery & Daemon Status
                    HorizontalLayout {
                        alignment: space-between;

                        VerticalLayout {
                            spacing: 2px;
                            Text {
                                text: root.active_tab == 0 ? "Mixer Deck" :
                                      root.active_tab == 1 ? "Hardware & Peripherals" :
                                      root.active_tab == 2 ? "Declarative Rule Studio" : "Audio Routing Policies";
                                font-size: 20px;
                                font-weight: 800;
                                color: #cad3f5;
                            }
                            Text {
                                text: root.active_tab == 0 ? "Hardware mixer channels and physical output endpoints" :
                                      root.active_tab == 1 ? "Connected audio interfaces, DACs, and wireless receivers" :
                                      root.active_tab == 2 ? "Deterministic automation rules and signal arbiters" : "Stream pinning and DSP graph enforcement";
                                font-size: 12px;
                                color: #8087a2;
                            }
                        }

                        HorizontalLayout {
                            spacing: 10px;
                            alignment: center;

                            // Battery Pill
                            Rectangle {
                                background: #24273a;
                                border-radius: 14px;
                                border-width: 1px;
                                border-color: #363a4f;
                                height: 28px;
                                HorizontalLayout {
                                    padding-left: 10px;
                                    padding-right: 10px;
                                    spacing: 6px;
                                    Text {
                                        text: "󰋋";
                                        font-size: 12px;
                                        color: root.headset_active ? #a6da95 : #eed49f;
                                        vertical-alignment: center;
                                    }
                                    Text {
                                        text: root.headset_active ? "Arctis 7: " + root.headset_battery : "Arctis 7: Standby";
                                        font-size: 11px;
                                        font-weight: 600;
                                        color: #cad3f5;
                                        vertical-alignment: center;
                                    }
                                }
                            }

                            // Daemon Status Pill
                            Rectangle {
                                background: #24273a;
                                border-radius: 14px;
                                border-width: 1px;
                                border-color: #363a4f;
                                height: 28px;
                                HorizontalLayout {
                                    padding-left: 10px;
                                    padding-right: 10px;
                                    spacing: 6px;
                                    Rectangle {
                                        width: 8px;
                                        height: 8px;
                                        border-radius: 4px;
                                        background: root.daemon_color;
                                        y: 10px;
                                    }
                                    Text {
                                        text: root.daemon_status;
                                        font-size: 11px;
                                        font-weight: 600;
                                        color: root.daemon_color;
                                        vertical-alignment: center;
                                    }
                                }
                            }
                        }
                    }

                    // TAB 0: THE STUDIO DECK (Homepage)
                    if root.active_tab == 0 : VerticalLayout {
                        spacing: 14px;
                        vertical-stretch: 1;

                        // 6-Channel Strips Grid
                        HorizontalLayout {
                            spacing: 12px;
                            vertical-stretch: 1;

                            ChannelStrip {
                                name: "System";
                                tag: "IPC";
                                tag_color: #8aadf4;
                                volume: root.system_volume;
                                muted: false;
                                volume_changed(v) => { root.system_volume = v; }
                                toggle_mute() => {}
                            }

                            ChannelStrip {
                                name: "Game";
                                tag: "7.1 DSP";
                                tag_color: #c6a0f6;
                                highlight: true;
                                volume: root.game_volume;
                                muted: false;
                                volume_changed(v) => { root.game_volume = v; }
                                toggle_mute() => {}
                            }

                            ChannelStrip {
                                name: "Chat";
                                tag: "VOIP";
                                tag_color: #8bd5ca;
                                volume: root.chat_volume;
                                muted: false;
                                volume_changed(v) => { root.chat_volume = v; }
                                toggle_mute() => {}
                            }

                            ChannelStrip {
                                name: "Music";
                                tag: "MEDIA";
                                tag_color: #f5a97f;
                                volume: root.music_volume;
                                muted: false;
                                volume_changed(v) => { root.music_volume = v; }
                                toggle_mute() => {}
                            }

                            ChannelStrip {
                                name: "Speakers";
                                tag: "OUT";
                                tag_color: #eed49f;
                                volume: root.speaker_volume;
                                muted: root.speaker_volume <= 0.0;
                                volume_changed(v) => {
                                    root.speaker_volume = v;
                                    root.volume_changed(v);
                                }
                                toggle_mute() => {
                                    root.toggle_speakers();
                                }
                            }

                            ChannelStrip {
                                name: "Arctis 7";
                                tag: "WIRELESS";
                                tag_color: root.headset_active ? #a6da95 : #8087a2;
                                volume: root.headset_volume;
                                muted: !root.headset_active;
                                volume_changed(v) => { root.headset_volume = v; }
                                toggle_mute() => {}
                            }
                        }

                        // Bottom Automation Ribbon
                        Rectangle {
                            height: 38px;
                            background: #1e2030;
                            border-radius: 8px;
                            border-width: 1px;
                            border-color: #363a4f;

                            HorizontalLayout {
                                padding-left: 14px;
                                padding-right: 14px;
                                alignment: space-between;

                                HorizontalLayout {
                                    spacing: 8px;
                                    Text {
                                        text: "⚡ Automation State:";
                                        font-size: 11px;
                                        font-weight: 700;
                                        color: #a6da95;
                                        vertical-alignment: center;
                                    }
                                    Text {
                                        text: root.headset_active ?
                                            "Headset Active ➔ Speakers Muted (0%) ➔ Loopback Streaming to Arctis Nova 7" :
                                            "Headset Standby ➔ Speakers Restored to 100% ➔ Loopback Unloaded";
                                        font-size: 11px;
                                        color: #cad3f5;
                                        vertical-alignment: center;
                                    }
                                }

                                Text {
                                    text: "Downstream DSP Traversal: Active (Unity Gain)";
                                    font-size: 10px;
                                    color: #8087a2;
                                    vertical-alignment: center;
                                }
                            }
                        }
                    }

                    // TAB 1: DEVICES & HARDWARE
                    if root.active_tab == 1 : VerticalLayout {
                        spacing: 14px;
                        vertical-stretch: 1;

                        // GoXLR Card
                        Rectangle {
                            background: #1e2030;
                            border-radius: 12px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 16px;
                                spacing: 10px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "TC-Helicon GoXLR Mini";
                                        font-size: 16px;
                                        font-weight: 700;
                                        color: #c6a0f6;
                                    }
                                    Text {
                                        text: root.goxlr_status;
                                        font-size: 12px;
                                        font-weight: 600;
                                        color: #a6da95;
                                    }
                                }

                                Text {
                                    text: "Hardware Serial: " + root.goxlr_serial + " • Unix Socket: /tmp/goxlr.socket";
                                    font-size: 12px;
                                    color: #a5adcb;
                                }

                                Text {
                                    text: "Physical Routing: Headphones DAC Jack routed to Desk Speakers at 0-255 hardware steps.";
                                    font-size: 11px;
                                    color: #6e738d;
                                }
                            }
                        }

                        // Arctis Nova 7 Card
                        Rectangle {
                            background: #1e2030;
                            border-radius: 12px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 16px;
                                spacing: 10px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "SteelSeries Arctis Nova 7 (Wireless)";
                                        font-size: 16px;
                                        font-weight: 700;
                                        color: #8aadf4;
                                    }
                                    Text {
                                        text: root.headset_status;
                                        font-size: 12px;
                                        font-weight: 600;
                                        color: root.headset_active ? #a6da95 : #eed49f;
                                    }
                                }

                                Text {
                                    text: "Battery Gauge: " + root.headset_battery + " • Vendor ID: 0x1038 • Product ID: 0x12ad";
                                    font-size: 12px;
                                    color: #a5adcb;
                                }

                                Text {
                                    text: "True presence polling via USB HID battery telemetry to prevent ghost-dongle misrouting.";
                                    font-size: 11px;
                                    color: #6e738d;
                                }
                            }
                        }

                        Rectangle { vertical-stretch: 1; }
                    }

                    // TAB 2: DECLARATIVE RULE STUDIO
                    if root.active_tab == 2 : VerticalLayout {
                        spacing: 12px;
                        vertical-stretch: 1;

                        Rectangle {
                            background: #1e2030;
                            border-radius: 10px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 6px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "Rule 1: Auto Headset Switcher";
                                        font-size: 14px;
                                        font-weight: 700;
                                        color: #cad3f5;
                                    }
                                    Text {
                                        text: "[✓] ACTIVE";
                                        font-size: 11px;
                                        font-weight: 700;
                                        color: #a6da95;
                                    }
                                }
                                Text {
                                    text: "Trigger: headset.status == Active (Debounce: 1500ms)";
                                    font-size: 11px;
                                    color: #8aadf4;
                                }
                                Text {
                                    text: "Enter Actions: Mute Desk Speakers (0%) + Load Loopback GoXLR Chat.monitor ➔ Arctis 7";
                                    font-size: 11px;
                                    color: #a5adcb;
                                }
                                Text {
                                    text: "Exit Actions: Unload Loopback + Restore Desk Speakers (slot 'speakers' / 100%)";
                                    font-size: 11px;
                                    color: #a5adcb;
                                }
                            }
                        }

                        Rectangle {
                            background: #1e2030;
                            border-radius: 10px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 6px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "Rule 2: Browser Stream Pinning & Isolation";
                                        font-size: 14px;
                                        font-weight: 700;
                                        color: #cad3f5;
                                    }
                                    Text {
                                        text: "[✓] ACTIVE";
                                        font-size: 11px;
                                        font-weight: 700;
                                        color: #a6da95;
                                    }
                                }
                                Text {
                                    text: "Trigger: node.app_name == 'Zen Browser'";
                                    font-size: 11px;
                                    color: #8aadf4;
                                }
                                Text {
                                    text: "Action: Pin playback stream to GoXLR System channel (bypass 7.1 spatializer)";
                                    font-size: 11px;
                                    color: #a5adcb;
                                }
                            }
                        }

                        Rectangle {
                            background: #1e2030;
                            border-radius: 10px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 6px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "Rule 3: Downstream DSP Volume Traversal";
                                        font-size: 14px;
                                        font-weight: 700;
                                        color: #cad3f5;
                                    }
                                    Text {
                                        text: "[✓] ACTIVE";
                                        font-size: 11px;
                                        font-weight: 700;
                                        color: #a6da95;
                                    }
                                }
                                Text {
                                    text: "Policy: Prevent digital clipping on Virtual Surround DSP node; map levels directly to hardware DAC.";
                                    font-size: 11px;
                                    color: #a5adcb;
                                }
                            }
                        }

                        Rectangle { vertical-stretch: 1; }
                    }

                    // TAB 3: STREAM ROUTING POLICIES
                    if root.active_tab == 3 : VerticalLayout {
                        spacing: 14px;
                        vertical-stretch: 1;

                        Rectangle {
                            background: #1e2030;
                            border-radius: 12px;
                            border-width: 1px;
                            border-color: #363a4f;

                            VerticalLayout {
                                padding: 16px;
                                spacing: 10px;

                                Text {
                                    text: "PipeWire Audio Graph Policy Enforcement";
                                    font-size: 15px;
                                    font-weight: 700;
                                    color: #f5bde6;
                                }

                                Text {
                                    text: "• Virtual Surround Manager (effect_output) ➔ GoXLR Game (HiFi Line 1)";
                                    font-size: 12px;
                                    color: #a5adcb;
                                }

                                Text {
                                    text: "• Browser Streams (Zen) ➔ Isolated on GoXLR System (HiFi Line 2)";
                                    font-size: 12px;
                                    color: #a5adcb;
                                }

                                Text {
                                    text: "• Active Loopback Status: " + root.routing_state;
                                    font-size: 12px;
                                    color: #eed49f;
                                }

                                HorizontalLayout {
                                    alignment: end;
                                    Button {
                                        text: "Enforce Policies Now";
                                        clicked => { root.force_routing_refresh(); }
                                    }
                                }
                            }
                        }

                        Rectangle { vertical-stretch: 1; }
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
                            ui.set_headset_active(hs_active);
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
                        ui.set_headset_active(t.is_active);
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
