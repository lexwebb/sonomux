use slint::slint;
use std::time::Duration;
use tracing::info;
use zbus::proxy;

slint! {
    import { Button, Slider, VerticalBox, HorizontalBox } from "std-widgets.slint";

    component NavItem inherits Rectangle {
        in-out property <string> label;
        in-out property <bool> active: false;
        callback clicked();

        height: 36px;
        border-radius: 4px;
        background: root.active ? #1e212b : (touch.has-hover ? #171920 : transparent);

        HorizontalLayout {
            padding-left: 12px;
            padding-right: 12px;
            spacing: 8px;

            Rectangle {
                width: 3px;
                height: 16px;
                y: 10px;
                border-radius: 1px;
                background: root.active ? #5a647e : transparent;
            }

            Text {
                text: root.label;
                font-size: 11px;
                font-weight: root.active ? 700 : 600;
                color: root.active ? #e1e4ea : #707688;
                vertical-alignment: center;
                letter-spacing: 0.5px;
            }
        }

        touch := TouchArea {
            clicked => { root.clicked(); }
        }
    }

    component ConsoleFader inherits Rectangle {
        in-out property <float> value: 100.0;
        in-out property <bool> is_muted: false;
        callback changed(float);

        width: 32px;
        height: 220px;
        background: transparent;

        // Vertical guide line
        Rectangle {
            width: 4px;
            height: parent.height - 20px;
            x: (parent.width - self.width) / 2;
            y: 10px;
            background: #0d0e12;
            border-radius: 2px;
            border-width: 1px;
            border-color: #21242d;

            // Fill bar
            Rectangle {
                y: parent.height - self.height;
                width: parent.width;
                height: Math.max(0px, parent.height * (root.value / 100.0));
                background: root.is_muted ? #b83535 : #3dd68c;
                border-radius: 1px;
            }
        }

        // Tactile Aluminum Fader Knob
        Rectangle {
            y: (parent.height - 24px) * (1.0 - (root.value / 100.0));
            width: 28px;
            x: 2px;
            height: 22px;
            border-radius: 3px;
            background: #2e323e;
            border-width: 1px;
            border-color: #4a5062;

            // Center index line
            Rectangle {
                y: (parent.height - 2px) / 2;
                width: parent.width - 6px;
                x: 3px;
                height: 2px;
                background: #ffffff;
                border-radius: 1px;
            }
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

    component MixerStrip inherits Rectangle {
        in-out property <string> title;
        in-out property <string> subtitle;
        in-out property <float> volume: 100.0;
        in-out property <bool> muted: false;
        in-out property <bool> is_output: false;
        callback volume_changed(float);
        callback toggle_mute();

        background: #15171d;
        border-radius: 6px;
        border-width: 1px;
        border-color: #232630;

        VerticalLayout {
            padding-top: 12px;
            padding-bottom: 12px;
            padding-left: 8px;
            padding-right: 8px;
            spacing: 8px;
            alignment: space-between;

            // Channel Header
            VerticalLayout {
                spacing: 2px;
                Text {
                    text: root.title;
                    font-size: 11px;
                    font-weight: 800;
                    color: #d6d9e0;
                    horizontal-alignment: center;
                    letter-spacing: 0.6px;
                }
                Text {
                    text: root.subtitle;
                    font-size: 9px;
                    font-weight: 600;
                    color: #5d6373;
                    horizontal-alignment: center;
                }
            }

            // Fader Area
            HorizontalLayout {
                alignment: center;
                ConsoleFader {
                    value: root.volume;
                    is_muted: root.muted;
                    changed(val) => {
                        root.volume = val;
                        root.volume_changed(val);
                    }
                }
            }

            // Monospace numeric level
            Text {
                text: root.muted ? "OFF" : Math.round(root.volume) + "%";
                font-size: 11px;
                font-weight: 700;
                font-family: "monospace";
                color: root.muted ? #d44545 : #a1a7b8;
                horizontal-alignment: center;
            }

            // Mute Switch
            Rectangle {
                height: 26px;
                border-radius: 3px;
                background: root.muted ? #9e2b2b : #20232b;
                border-width: 1px;
                border-color: root.muted ? #c23838 : #2e323e;

                HorizontalLayout {
                    alignment: center;
                    spacing: 5px;

                    // Indicator LED
                    Rectangle {
                        width: 5px;
                        height: 5px;
                        border-radius: 2.5px;
                        y: 10px;
                        background: root.muted ? #ffffff : #3dd68c;
                    }

                    Text {
                        text: root.muted ? "MUTED" : "MUTE";
                        font-size: 10px;
                        font-weight: 700;
                        color: root.muted ? #ffffff : #7a8194;
                        vertical-alignment: center;
                        letter-spacing: 0.5px;
                    }
                }

                TouchArea {
                    clicked => { root.toggle_mute(); }
                }
            }
        }
    }

    export component MainWindow inherits Window {
        title: "Sonomux Audio Coordinator";
        min-width: 900px;
        min-height: 600px;
        background: #0f1013;

        in-out property <int> active_tab: 0;

        in-out property <string> daemon_status: "CONNECTING";
        in-out property <color> daemon_color: #e5a83b;
        in-out property <string> goxlr_status: "OFFLINE";
        in-out property <string> goxlr_serial: "--";
        in-out property <float> speaker_volume: 100.0;
        in-out property <float> headset_volume: 80.0;
        in-out property <float> system_volume: 100.0;
        in-out property <float> game_volume: 85.0;
        in-out property <float> chat_volume: 90.0;
        in-out property <float> music_volume: 70.0;

        in-out property <bool> speakers_muted: false;
        in-out property <bool> headset_active: false;
        in-out property <string> headset_status: "STANDBY";
        in-out property <string> headset_battery: "--";
        in-out property <string> routing_state: "IDLE";

        callback volume_changed(float);
        callback toggle_speakers();
        callback force_routing_refresh();

        HorizontalLayout {
            // Technical Left Sidebar
            Rectangle {
                width: 170px;
                background: #13151a;
                border-width: 1px;
                border-color: #1f222b;

                VerticalLayout {
                    padding-top: 14px;
                    padding-bottom: 14px;
                    padding-left: 8px;
                    padding-right: 8px;
                    spacing: 4px;

                    // Brand Header
                    VerticalLayout {
                        padding-left: 10px;
                        padding-bottom: 12px;
                        spacing: 2px;
                        Text {
                            text: "SONOMUX";
                            font-size: 14px;
                            font-weight: 900;
                            color: #e1e4ea;
                            letter-spacing: 1px;
                        }
                        Text {
                            text: "AUDIO COORDINATOR";
                            font-size: 9px;
                            font-weight: 700;
                            color: #525766;
                            letter-spacing: 0.8px;
                        }
                    }

                    Rectangle {
                        height: 1px;
                        background: #1f222b;
                    }

                    // Navigation Tabs
                    NavItem {
                        label: "MIXER CONSOLE";
                        active: root.active_tab == 0;
                        clicked => { root.active_tab = 0; }
                    }

                    NavItem {
                        label: "HARDWARE";
                        active: root.active_tab == 1;
                        clicked => { root.active_tab = 1; }
                    }

                    NavItem {
                        label: "RULE ENGINE";
                        active: root.active_tab == 2;
                        clicked => { root.active_tab = 2; }
                    }

                    NavItem {
                        label: "STREAM ROUTING";
                        active: root.active_tab == 3;
                        clicked => { root.active_tab = 3; }
                    }

                    Rectangle { vertical-stretch: 1; }

                    Rectangle {
                        height: 1px;
                        background: #1f222b;
                    }

                    VerticalLayout {
                        padding-top: 8px;
                        padding-left: 8px;
                        spacing: 2px;
                        Text {
                            text: "RELEASE v0.1.0";
                            font-size: 9px;
                            font-weight: 700;
                            font-family: "monospace";
                            color: #4a4f5d;
                        }
                        Text {
                            text: "PIPEWIRE / LINUX";
                            font-size: 8px;
                            font-weight: 700;
                            font-family: "monospace";
                            color: #3b3f4c;
                        }
                    }
                }
            }

            // Main Console Surface
            Rectangle {
                horizontal-stretch: 1;
                background: #0f1013;

                VerticalLayout {
                    padding-top: 14px;
                    padding-bottom: 14px;
                    padding-left: 18px;
                    padding-right: 18px;
                    spacing: 12px;

                    // Top Telemetry Header
                    HorizontalLayout {
                        alignment: space-between;

                        Text {
                            text: root.active_tab == 0 ? "DESKTOP MIXER" :
                                  root.active_tab == 1 ? "HARDWARE TELEMETRY" :
                                  root.active_tab == 2 ? "ACTIVE RULES" : "AUDIO GRAPH ROUTING";
                            font-size: 13px;
                            font-weight: 800;
                            color: #e1e4ea;
                            vertical-alignment: center;
                            letter-spacing: 0.8px;
                        }

                        HorizontalLayout {
                            spacing: 12px;
                            alignment: center;

                            // Headset Telemetry Pill
                            Rectangle {
                                background: #15171d;
                                border-radius: 3px;
                                border-width: 1px;
                                border-color: #232630;
                                height: 24px;
                                HorizontalLayout {
                                    padding-left: 8px;
                                    padding-right: 8px;
                                    spacing: 6px;
                                    Rectangle {
                                        width: 5px;
                                        height: 5px;
                                        border-radius: 2.5px;
                                        background: root.headset_active ? #3dd68c : #e5a83b;
                                        y: 9px;
                                    }
                                    Text {
                                        text: root.headset_active ? "NOVA 7: " + root.headset_battery : "NOVA 7: STANDBY";
                                        font-size: 10px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #b0b5c4;
                                        vertical-alignment: center;
                                    }
                                }
                            }

                            // Daemon Telemetry Pill
                            Rectangle {
                                background: #15171d;
                                border-radius: 3px;
                                border-width: 1px;
                                border-color: #232630;
                                height: 24px;
                                HorizontalLayout {
                                    padding-left: 8px;
                                    padding-right: 8px;
                                    spacing: 6px;
                                    Rectangle {
                                        width: 5px;
                                        height: 5px;
                                        border-radius: 2.5px;
                                        background: root.daemon_color;
                                        y: 9px;
                                    }
                                    Text {
                                        text: root.daemon_status;
                                        font-size: 10px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #b0b5c4;
                                        vertical-alignment: center;
                                    }
                                }
                            }
                        }
                    }

                    // TAB 0: MIXER CONSOLE
                    if root.active_tab == 0 : VerticalLayout {
                        spacing: 12px;
                        vertical-stretch: 1;

                        // 6 Channel Strips
                        HorizontalLayout {
                            spacing: 8px;
                            vertical-stretch: 1;

                            MixerStrip {
                                title: "SYSTEM";
                                subtitle: "CH 1-2";
                                volume: root.system_volume;
                                muted: false;
                                volume_changed(v) => { root.system_volume = v; }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                title: "GAME";
                                subtitle: "VSM 7.1";
                                volume: root.game_volume;
                                muted: false;
                                volume_changed(v) => { root.game_volume = v; }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                title: "CHAT";
                                subtitle: "VOIP";
                                volume: root.chat_volume;
                                muted: false;
                                volume_changed(v) => { root.chat_volume = v; }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                title: "MUSIC";
                                subtitle: "MEDIA";
                                volume: root.music_volume;
                                muted: false;
                                volume_changed(v) => { root.music_volume = v; }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                title: "SPEAKERS";
                                subtitle: "LINE OUT";
                                is_output: true;
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

                            MixerStrip {
                                title: "NOVA 7";
                                subtitle: "WIRELESS";
                                is_output: true;
                                volume: root.headset_volume;
                                muted: !root.headset_active;
                                volume_changed(v) => { root.headset_volume = v; }
                                toggle_mute() => {}
                            }
                        }

                        // Bottom Status Bar
                        Rectangle {
                            height: 30px;
                            background: #13151a;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #1f222b;

                            HorizontalLayout {
                                padding-left: 12px;
                                padding-right: 12px;
                                alignment: space-between;

                                HorizontalLayout {
                                    spacing: 8px;
                                    Text {
                                        text: "STATUS:";
                                        font-size: 10px;
                                        font-weight: 800;
                                        font-family: "monospace";
                                        color: #616778;
                                        vertical-alignment: center;
                                    }
                                    Text {
                                        text: root.headset_active ?
                                            "HEADSET ACTIVE // SPEAKERS MUTED // LOOPBACK STREAMING" :
                                            "HEADSET STANDBY // SPEAKERS ACTIVE (100%) // LOOPBACK IDLE";
                                        font-size: 10px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: root.headset_active ? #3dd68c : #b0b5c4;
                                        vertical-alignment: center;
                                    }
                                }

                                Text {
                                    text: "DSP: UNITY GAIN";
                                    font-size: 9px;
                                    font-weight: 700;
                                    font-family: "monospace";
                                    color: #4a4f5d;
                                    vertical-alignment: center;
                                }
                            }
                        }
                    }

                    // TAB 1: HARDWARE TELEMETRY
                    if root.active_tab == 1 : VerticalLayout {
                        spacing: 10px;
                        vertical-stretch: 1;

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 8px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "TC-HELICON GOXLR MINI";
                                        font-size: 12px;
                                        font-weight: 800;
                                        color: #e1e4ea;
                                    }
                                    Text {
                                        text: root.goxlr_status;
                                        font-size: 10px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #3dd68c;
                                    }
                                }

                                Text {
                                    text: "SERIAL: " + root.goxlr_serial + "  •  IPC: /tmp/goxlr.socket  •  DAC REGISTER: 0-255";
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #7a8194;
                                }
                            }
                        }

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 8px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "STEELSERIES ARCTIS NOVA 7";
                                        font-size: 12px;
                                        font-weight: 800;
                                        color: #e1e4ea;
                                    }
                                    Text {
                                        text: root.headset_status;
                                        font-size: 10px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: root.headset_active ? #3dd68c : #e5a83b;
                                    }
                                }

                                Text {
                                    text: "BATTERY: " + root.headset_battery + "  •  USB HID: 1038:12ad  •  TELEMETRY: ACTIVE";
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #7a8194;
                                }
                            }
                        }

                        Rectangle { vertical-stretch: 1; }
                    }

                    // TAB 2: RULE ENGINE
                    if root.active_tab == 2 : VerticalLayout {
                        spacing: 8px;
                        vertical-stretch: 1;

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 12px;
                                spacing: 4px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "RULE: AUTO HEADSET SWITCHER";
                                        font-size: 11px;
                                        font-weight: 800;
                                        color: #e1e4ea;
                                    }
                                    Text {
                                        text: "ACTIVE";
                                        font-size: 9px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #3dd68c;
                                    }
                                }
                                Text {
                                    text: "IF headset == Active (debounce: 1500ms)  ➔  Speakers: 0%, Loopback: Loaded";
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #7a8194;
                                }
                            }
                        }

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 12px;
                                spacing: 4px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "RULE: BROWSER STREAM ISOLATION";
                                        font-size: 11px;
                                        font-weight: 800;
                                        color: #e1e4ea;
                                    }
                                    Text {
                                        text: "ACTIVE";
                                        font-size: 9px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #3dd68c;
                                    }
                                }
                                Text {
                                    text: "IF app == 'Zen Browser'  ➔  Pin to GoXLR System (bypass DSP)";
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #7a8194;
                                }
                            }
                        }

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 12px;
                                spacing: 4px;

                                HorizontalLayout {
                                    alignment: space-between;
                                    Text {
                                        text: "RULE: DOWNSTREAM VOLUME TRAVERSAL";
                                        font-size: 11px;
                                        font-weight: 800;
                                        color: #e1e4ea;
                                    }
                                    Text {
                                        text: "ACTIVE";
                                        font-size: 9px;
                                        font-weight: 700;
                                        font-family: "monospace";
                                        color: #3dd68c;
                                    }
                                }
                                Text {
                                    text: "DSP: Unity Gain 100%  ➔  Direct hardware DAC mapping";
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #7a8194;
                                }
                            }
                        }

                        Rectangle { vertical-stretch: 1; }
                    }

                    // TAB 3: STREAM ROUTING
                    if root.active_tab == 3 : VerticalLayout {
                        spacing: 10px;
                        vertical-stretch: 1;

                        Rectangle {
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            VerticalLayout {
                                padding: 14px;
                                spacing: 8px;

                                Text {
                                    text: "ACTIVE PIPEWIRE TOPOLOGY";
                                    font-size: 11px;
                                    font-weight: 800;
                                    color: #e1e4ea;
                                }

                                Text {
                                    text: "• effect_output.virtual-surround-manager  ➔  GoXLR Game (HiFi Line 1)\n• Zen Browser Streams  ➔  GoXLR System (HiFi Line 2)\n• Loopback Status: " + root.routing_state;
                                    font-size: 10px;
                                    font-family: "monospace";
                                    color: #8b92a4;
                                }

                                HorizontalLayout {
                                    alignment: end;
                                    Button {
                                        text: "ENFORCE GRAPH";
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
