use slint::slint;
use std::sync::Arc;
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
                    value <=> root.volume;
                    is_muted: root.muted;
                    changed(val) => {
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

    component ChannelPill inherits Rectangle {
        in-out property <string> label;
        in-out property <bool> active: false;
        callback clicked();

        height: 24px;
        border-radius: 3px;
        background: root.active ? #1e2638 : (touch.has-hover ? #171922 : #13151a);
        border-width: 1px;
        border-color: root.active ? #3dd68c : #232733;

        HorizontalLayout {
            padding-left: 10px;
            padding-right: 10px;
            spacing: 5px;
            alignment: center;

            Rectangle {
                width: 4px;
                height: 4px;
                border-radius: 2px;
                background: root.active ? #3dd68c : transparent;
                y: 10px;
            }

            Text {
                text: root.label;
                font-size: 9px;
                font-weight: 700;
                font-family: "monospace";
                color: root.active ? #3dd68c : #7a8194;
                vertical-alignment: center;
            }
        }

        touch := TouchArea {
            clicked => { root.clicked(); }
        }
    }

    component IndustrialToggle inherits Rectangle {
        in-out property <bool> checked: true;
        in-out property <string> title;
        in-out property <string> subtitle;
        callback toggled();

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
                    text: root.title;
                    font-size: 11px;
                    font-weight: 800;
                    color: #e1e4ea;
                }

                Rectangle {
                    width: 54px;
                    height: 22px;
                    border-radius: 3px;
                    background: root.checked ? #1b3829 : #20232b;
                    border-width: 1px;
                    border-color: root.checked ? #3dd68c : #3a3f4d;

                    HorizontalLayout {
                        alignment: center;
                        Text {
                            text: root.checked ? "ACTIVE" : "OFF";
                            font-size: 9px;
                            font-weight: 800;
                            font-family: "monospace";
                            color: root.checked ? #3dd68c : #7a8194;
                            vertical-alignment: center;
                        }
                    }

                    TouchArea {
                        clicked => { root.toggled(); }
                    }
                }
            }

            Text {
                text: root.subtitle;
                font-size: 10px;
                font-family: "monospace";
                color: #7a8194;
            }
        }
    }

    component CategoryRow inherits Rectangle {
        in-out property <string> category_name;
        in-out property <string> current_target;
        in-out property <string> opt1: "SYSTEM";
        in-out property <string> opt2: "MUSIC";
        in-out property <string> opt3: "GAME";
        in-out property <string> opt4: "SURROUND";
        in-out property <bool> has_opt4: false;
        callback select(string);

        background: #15171d;
        border-radius: 4px;
        border-width: 1px;
        border-color: #232630;
        height: 48px;

        HorizontalLayout {
            padding-left: 14px;
            padding-right: 14px;
            alignment: space-between;

            VerticalLayout {
                alignment: center;
                spacing: 2px;
                Text {
                    text: root.category_name;
                    font-size: 11px;
                    font-weight: 800;
                    color: #d6d9e0;
                    letter-spacing: 0.5px;
                }
                Text {
                    text: "ROUTED TO: " + root.current_target;
                    font-size: 9px;
                    font-weight: 700;
                    font-family: "monospace";
                    color: #5d6475;
                }
            }

            HorizontalLayout {
                alignment: center;
                spacing: 6px;

                ChannelPill {
                    label: root.opt1;
                    active: root.current_target == root.opt1;
                    clicked => { root.select(root.opt1); }
                }
                ChannelPill {
                    label: root.opt2;
                    active: root.current_target == root.opt2;
                    clicked => { root.select(root.opt2); }
                }
                if root.opt3 != "" : ChannelPill {
                    label: root.opt3;
                    active: root.current_target == root.opt3;
                    clicked => { root.select(root.opt3); }
                }
                if root.has_opt4 : ChannelPill {
                    label: root.opt4;
                    active: root.current_target == root.opt4;
                    clicked => { root.select(root.opt4); }
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
        in-out property <float> mic_volume: 100.0;
        // Strip position (0-4) for each input channel; set from the GoXLR fader assignment.
        // Whether the headset's audio device exists; hides its strip and badge when absent.
        in property <bool> headset_present: false;
        in property <string> platform_label: "PIPEWIRE / LINUX";
        in property <int> mic_slot: 0;
        in property <int> system_slot: 1;
        in property <int> game_slot: 2;
        in property <int> chat_slot: 3;
        in property <int> music_slot: 4;

        in-out property <bool> speakers_muted: false;
        in-out property <bool> headset_active: false;
        in-out property <string> headset_status: "STANDBY";
        in-out property <string> headset_battery: "--";
        in-out property <string> routing_state: "IDLE";

        // Configurable Automation Rules Properties
        in-out property <bool> rule_headset_switcher_enabled: true;
        in-out property <bool> rule_surround_isolation_enabled: true;

        // Configurable Stream Routing Properties
        in-out property <bool> routing_auto_enabled: true;
        in-out property <string> target_browser: "SYSTEM";
        in-out property <string> target_game: "SURROUND";
        in-out property <string> target_music: "MUSIC";
        in-out property <string> target_chat: "CHAT";
        in-out property <string> target_production: "SYSTEM";
        in-out property <string> target_soundboard: "SAMPLE";

        callback volume_changed(float);
        callback channel_volume_changed(string, float);
        callback toggle_speakers();
        callback force_routing_refresh();
        callback toggle_rule(string);
        callback toggle_routing_enabled();
        callback set_routing_category(string, string);

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
                            text: root.platform_label;
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
                            if root.headset_present : Rectangle {
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

                        // 7 strips: inputs in hardware fader order (slots 0-4), then outputs.
                        // Placed by x, not a layout, so the order can follow the GoXLR.
                        Rectangle {
                            vertical-stretch: 1;
                            property <length> gap: 8px;
                            property <int> count: root.headset_present ? 7 : 6;
                            property <length> cell: (self.width - (self.count - 1) * self.gap) / self.count;

                            MixerStrip {
                                x: root.mic_slot * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "MIC";
                                subtitle: "INPUT";
                                volume <=> root.mic_volume;
                                muted: false;
                                volume_changed(v) => {
                                    root.channel_volume_changed("Mic", v);
                                }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                x: root.system_slot * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "SYSTEM";
                                subtitle: "CH 1-2";
                                volume <=> root.system_volume;
                                muted: false;
                                volume_changed(v) => {
                                    root.channel_volume_changed("System", v);
                                }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                x: root.game_slot * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "GAME";
                                subtitle: "VSM 7.1";
                                volume <=> root.game_volume;
                                muted: false;
                                volume_changed(v) => {
                                    root.channel_volume_changed("Game", v);
                                }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                x: root.chat_slot * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "CHAT";
                                subtitle: "VOIP";
                                volume <=> root.chat_volume;
                                muted: false;
                                volume_changed(v) => {
                                    root.channel_volume_changed("Chat", v);
                                }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                x: root.music_slot * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "MUSIC";
                                subtitle: "MEDIA";
                                volume <=> root.music_volume;
                                muted: false;
                                volume_changed(v) => {
                                    root.channel_volume_changed("Music", v);
                                }
                                toggle_mute() => {}
                            }

                            MixerStrip {
                                x: 5 * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                title: "SPEAKERS";
                                subtitle: "LINE OUT";
                                is_output: true;
                                volume <=> root.speaker_volume;
                                muted: root.speaker_volume <= 0.0;
                                volume_changed(v) => {
                                    root.channel_volume_changed("Headphones", v);
                                    root.volume_changed(v);
                                }
                                toggle_mute() => {
                                    root.toggle_speakers();
                                }
                            }

                            MixerStrip {
                                x: 6 * (parent.cell + parent.gap);
                                width: parent.cell;
                                height: parent.height;
                                visible: root.headset_present;
                                title: "NOVA 7";
                                subtitle: "WIRELESS";
                                is_output: true;
                                volume <=> root.headset_volume;
                                muted: !root.headset_active;
                                volume_changed(v) => {}
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
                                            !root.headset_present ? "NO HEADSET // SPEAKERS ACTIVE" :
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

                        IndustrialToggle {
                            title: "RULE: AUTO HEADSET SWITCHER";
                            subtitle: "IF headset == Active  ➔  Mute speakers, establish zero-latency loopback to Nova 7";
                            checked: root.rule_headset_switcher_enabled;
                            toggled => { root.toggle_rule("Auto Headset Switcher"); }
                        }

                        IndustrialToggle {
                            title: "RULE: BROWSER & APP SURROUND ISOLATION";
                            subtitle: "Keep-off list [Zen, Firefox, Chrome, Spotify, Discord]  ➔  Prevent 7.1 distortion";
                            checked: root.rule_surround_isolation_enabled;
                            toggled => { root.toggle_rule("Browser Stream Isolation"); }
                        }

                        IndustrialToggle {
                            title: "RULE: DOWNSTREAM VOLUME TRAVERSAL";
                            subtitle: "DSP: Unity Gain 100%  ➔  Direct physical DAC volume mapping (Omarchy Law)";
                            checked: true;
                            toggled => {}
                        }

                        Rectangle { vertical-stretch: 1; }
                    }

                    // TAB 3: STREAM ROUTING
                    if root.active_tab == 3 : VerticalLayout {
                        spacing: 8px;
                        vertical-stretch: 1;

                        // Master Toggle
                        IndustrialToggle {
                            title: "AUTOMATIC HEURISTIC STREAM ROUTING";
                            subtitle: "Continuously detect application categories and enforce target channels in PipeWire";
                            checked: root.routing_auto_enabled;
                            toggled => { root.toggle_routing_enabled(); }
                        }

                        // Category Route Matrix
                        VerticalLayout {
                            spacing: 6px;

                            CategoryRow {
                                category_name: "GAMES (PROTON / WINE / STEAM / EMULATORS)";
                                current_target: root.target_game;
                                opt1: "SURROUND";
                                opt2: "GAME";
                                opt3: "SYSTEM";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("game", t); }
                            }

                            CategoryRow {
                                category_name: "WEB BROWSERS (ZEN / FIREFOX / CHROMIUM / BRAVE)";
                                current_target: root.target_browser;
                                opt1: "SYSTEM";
                                opt2: "MUSIC";
                                opt3: "GAME";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("browser", t); }
                            }

                            CategoryRow {
                                category_name: "MUSIC PLAYERS (SPOTIFY / CIDER / VLC / TIDAL)";
                                current_target: root.target_music;
                                opt1: "MUSIC";
                                opt2: "SYSTEM";
                                opt3: "GAME";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("music", t); }
                            }

                            CategoryRow {
                                category_name: "VOIP & CHAT (DISCORD / TELEGRAM / SLACK / TEAMS)";
                                current_target: root.target_chat;
                                opt1: "CHAT";
                                opt2: "SYSTEM";
                                opt3: "";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("chat", t); }
                            }

                            CategoryRow {
                                category_name: "DAW & CONTENT CREATION (OBS / REAPER / AUDACITY)";
                                current_target: root.target_production;
                                opt1: "SYSTEM";
                                opt2: "GAME";
                                opt3: "";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("production", t); }
                            }

                            CategoryRow {
                                category_name: "SOUNDBOARD & SAMPLERS (SOUNDUX / SAMPLES)";
                                current_target: root.target_soundboard;
                                opt1: "SAMPLE";
                                opt2: "SYSTEM";
                                opt3: "";
                                has_opt4: false;
                                select(t) => { root.set_routing_category("soundboard", t); }
                            }
                        }

                        // Action Bar
                        Rectangle {
                            height: 38px;
                            background: #15171d;
                            border-radius: 4px;
                            border-width: 1px;
                            border-color: #232630;

                            HorizontalLayout {
                                padding-left: 12px;
                                padding-right: 12px;
                                alignment: space-between;

                                Text {
                                    text: "CONFIG: ~/.config/sonomux/config.toml  •  STATUS: LIVE SYNC";
                                    font-size: 10px;
                                    font-weight: 700;
                                    font-family: "monospace";
                                    color: #5d6475;
                                    vertical-alignment: center;
                                }

                                Button {
                                    text: "ENFORCE ROUTING NOW";
                                    clicked => { root.force_routing_refresh(); }
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
    async fn get_channel_volumes(&self) -> zbus::Result<std::collections::HashMap<String, f64>>;
    async fn set_channel_volume(&self, channel: String, volume_pct: f64) -> zbus::Result<()>;
    async fn set_speaker_volume(&self, volume: f64) -> zbus::Result<()>;
    async fn toggle_speaker_mute(&self) -> zbus::Result<bool>;
    async fn enforce_policies(&self) -> zbus::Result<()>;
    async fn get_routed_streams(&self) -> zbus::Result<Vec<(u64, String, String, String, String)>>;
    async fn get_routing_config(&self) -> zbus::Result<(bool, std::collections::HashMap<String, String>)>;
    async fn set_routing_enabled(&self, enabled: bool) -> zbus::Result<()>;
    async fn set_category_target(&self, category: String, target: String) -> zbus::Result<()>;
    async fn get_rules(&self) -> zbus::Result<Vec<(String, bool)>>;
    async fn set_rule_enabled(&self, rule_name: String, enabled: bool) -> zbus::Result<()>;
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

/// Input strip channels, in their order when none are on a fader.
const STRIP_CHANNELS: [&str; 5] = ["Mic", "System", "Game", "Chat", "Music"];

/// Slot (0-4) for each of `STRIP_CHANNELS`: fader channels first, in fader order (A-D),
/// then the rest in default order.
pub fn strip_slots(fader_channels: &[String]) -> [i32; 5] {
    let mut order: Vec<&str> = fader_channels
        .iter()
        .map(|c| c.as_str())
        .filter(|c| STRIP_CHANNELS.contains(c))
        .collect();
    order.dedup();
    for ch in STRIP_CHANNELS {
        if !order.contains(&ch) {
            order.push(ch);
        }
    }
    STRIP_CHANNELS.map(|ch| order.iter().position(|&c| c == ch).unwrap() as i32)
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
    if cfg!(target_os = "macos") {
        main_window.set_platform_label("COREAUDIO / MACOS".into());
    }

    let goxlr = sonomux_device_goxlr::GoXLRClient::new(sonomux_device_goxlr::DEFAULT_GOXLR_SOCKET);

    // Initial GoXLR hardware check
    if let Ok(serial) = goxlr.get_serial().await {
        main_window.set_goxlr_status("Active (Connected)".into());
        main_window.set_goxlr_serial(serial.into());
    } else {
        main_window.set_goxlr_status("Disconnected".into());
    }

    // Try connecting to daemon over session D-Bus
    // Retry briefly: the daemon may still be starting (e.g. right after login or install).
    let mut connection = None;
    for _ in 0..10 {
        connection = sonomux_core::ipc::connect().await.ok();
        if connection.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
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

    // Initialize rules and routing configuration from disk
    if let Ok((cfg, _)) = sonomux_core::AppConfig::load_or_create() {
        main_window.set_routing_auto_enabled(cfg.routing.enabled);
        for (cat, target) in &cfg.routing.category_targets {
            let t = target.to_uppercase();
            match cat.to_lowercase().as_str() {
                "browser" => main_window.set_target_browser(t.into()),
                "game" => main_window.set_target_game(t.into()),
                "music" => main_window.set_target_music(t.into()),
                "chat" => main_window.set_target_chat(t.into()),
                "production" => main_window.set_target_production(t.into()),
                "soundboard" => main_window.set_target_soundboard(t.into()),
                _ => {}
            }
        }
        for rule in &cfg.rules {
            match rule.name.as_str() {
                "Auto Headset Switcher" => main_window.set_rule_headset_switcher_enabled(rule.enabled),
                "Browser Stream Isolation" => main_window.set_rule_surround_isolation_enabled(rule.enabled),
                _ => {}
            }
        }
    }

    // State synchronization tracking & batching
    let last_user_touch = Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::<String, std::time::Instant>::new(),
    ));
    let pending_volumes = Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::<String, f64>::new(),
    ));
    let notify_volume = Arc::new(tokio::sync::Notify::new());

    // Worker that batches & serializes digital slider updates to D-Bus / GoXLR
    let worker_proxy = dbus_proxy.clone();
    let worker_goxlr = goxlr.clone();
    let worker_pending = Arc::clone(&pending_volumes);
    let worker_notify = Arc::clone(&notify_volume);
    tokio::spawn(async move {
        loop {
            worker_notify.notified().await;
            tokio::time::sleep(Duration::from_millis(15)).await;

            let batch: Vec<(String, f64)> = {
                let mut map = worker_pending.lock().unwrap();
                map.drain().collect()
            };

            for (ch_str, val) in batch {
                if let Some(ref p) = worker_proxy {
                    if p.set_channel_volume(ch_str.clone(), val).await.is_ok() {
                        continue;
                    }
                }
                let raw = ((val / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                let ch = match ch_str.as_str() {
                    "System" => sonomux_device_goxlr::Channel::System,
                    "Game" => sonomux_device_goxlr::Channel::Game,
                    "Chat" => sonomux_device_goxlr::Channel::Chat,
                    "Music" => sonomux_device_goxlr::Channel::Music,
                    "Headphones" | "Speakers" => sonomux_device_goxlr::Channel::Headphones,
                    "Mic" => sonomux_device_goxlr::Channel::Mic,
                    "LineIn" => sonomux_device_goxlr::Channel::LineIn,
                    "LineOut" => sonomux_device_goxlr::Channel::LineOut,
                    _ => continue,
                };
                let _ = worker_goxlr.set_volume_raw(ch, raw).await;
            }
        }
    });

    // Follow the GoXLR fader assignment so strips match the hardware order.
    let slot_handle = main_window.as_weak();
    let slot_goxlr = goxlr.clone();
    let headset_sink = sonomux_core::AppConfig::load_or_create()
        .map(|(cfg, _)| cfg.devices.headset_sink)
        .unwrap_or_default();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            // Also show hardware that is actually there: hide the headset when its device is gone.
            let present = sonomux_backend::Backend::sink_exists(&headset_sink).await;
            let weak = slot_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_headset_present(present);
                }
            });
            let Ok(faders) = slot_goxlr.get_fader_channels().await else {
                continue;
            };
            let [mic, system, game, chat, music] = strip_slots(&faders);
            let weak = slot_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_mic_slot(mic);
                    ui.set_system_slot(system);
                    ui.set_game_slot(game);
                    ui.set_chat_slot(chat);
                    ui.set_music_slot(music);
                }
            });
        }
    });

    // Spawn periodic state polling (either through D-Bus or direct hardware)
    let handle = main_window.as_weak();
    let proxy_for_poll = dbus_proxy.clone();
    let goxlr_for_poll = goxlr.clone();
    let touch_for_poll = Arc::clone(&last_user_touch);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;

            let now = std::time::Instant::now();
            let recent_channels: std::collections::HashSet<String> = {
                let map = touch_for_poll.lock().unwrap();
                map.iter()
                    .filter(|(_, &t)| now.duration_since(t).as_millis() < 350)
                    .map(|(k, _)| k.clone())
                    .collect()
            };

            if let Some(ref proxy) = proxy_for_poll {
                if let Ok((goxlr_up, _hs_status, hs_active, battery, spk_vol, loop_up)) =
                    proxy.get_status().await
                {
                    let ch_vols = proxy.get_channel_volumes().await.ok();
                    let weak = handle.clone();
                    let recent = recent_channels.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_daemon_status("ONLINE".into());
                            ui.set_daemon_color(slint::Color::from_rgb_u8(61, 214, 140));
                            ui.set_goxlr_status(if goxlr_up {
                                "ONLINE".into()
                            } else {
                                "OFFLINE".into()
                            });
                            ui.set_headset_active(hs_active);
                            ui.set_headset_status(if hs_active {
                                "ACTIVE".into()
                            } else {
                                "STANDBY".into()
                            });
                            ui.set_headset_battery(if hs_active && battery > 0 {
                                format!("{}%", battery).into()
                            } else if hs_active {
                                "ACTIVE".into()
                            } else {
                                "--".into()
                            });
                            if !recent.contains("Headphones") {
                                ui.set_speaker_volume(spk_vol as f32);
                            }
                            if let Some(ref map) = ch_vols {
                                if !recent.contains("System") {
                                    if let Some(&v) = map.get("System") {
                                        ui.set_system_volume(v as f32);
                                    }
                                }
                                if !recent.contains("Game") {
                                    if let Some(&v) = map.get("Game") {
                                        ui.set_game_volume(v as f32);
                                    }
                                }
                                if !recent.contains("Chat") {
                                    if let Some(&v) = map.get("Chat") {
                                        ui.set_chat_volume(v as f32);
                                    }
                                }
                                if !recent.contains("Music") {
                                    if let Some(&v) = map.get("Music") {
                                        ui.set_music_volume(v as f32);
                                    }
                                }
                                if !recent.contains("Mic") {
                                    if let Some(&v) = map.get("Mic") {
                                        ui.set_mic_volume(v as f32);
                                    }
                                }
                            }
                            ui.set_routing_state(if loop_up {
                                "STREAMING (LOOPBACK ACTIVE)".into()
                            } else {
                                "IDLE (LOOPBACK UNLOADED)".into()
                            });
                        }
                    });
                    continue;
                }
            }

            // Standalone fallback polling
            let weak = handle.clone();
            let goxlr_client = goxlr_for_poll.clone();
            let all_vols = goxlr_client.get_all_volumes().await.ok();
            let telemetry = sonomux_device_headset::HeadsetMonitor::probe_raw()
                .await
                .ok();
            let recent = recent_channels;

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_daemon_status("STANDALONE".into());
                    ui.set_daemon_color(slint::Color::from_rgb_u8(138, 144, 160));
                    if let Some(ref vols) = all_vols {
                        if !recent.contains("Headphones") {
                            if let Some(&vol) = vols.get("Headphones") {
                                ui.set_speaker_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                        if !recent.contains("System") {
                            if let Some(&vol) = vols.get("System") {
                                ui.set_system_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                        if !recent.contains("Game") {
                            if let Some(&vol) = vols.get("Game") {
                                ui.set_game_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                        if !recent.contains("Chat") {
                            if let Some(&vol) = vols.get("Chat") {
                                ui.set_chat_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                        if !recent.contains("Music") {
                            if let Some(&vol) = vols.get("Music") {
                                ui.set_music_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                        if !recent.contains("Mic") {
                            if let Some(&vol) = vols.get("Mic") {
                                ui.set_mic_volume((vol as f32 / 255.0) * 100.0);
                            }
                        }
                    }
                    if let Some(t) = telemetry {
                        ui.set_headset_active(t.is_active);
                        ui.set_headset_status(if t.is_active {
                            "ACTIVE".into()
                        } else {
                            "STANDBY".into()
                        });
                        ui.set_headset_battery(if let Some(b) = t.battery_percent {
                            format!("{}%", b).into()
                        } else if t.is_active {
                            "ACTIVE".into()
                        } else {
                            "--".into()
                        });
                    }
                }
            });
        }
    });

    // Callbacks
    let touch_for_vol = Arc::clone(&last_user_touch);
    let pending_for_vol = Arc::clone(&pending_volumes);
    let notify_for_vol = Arc::clone(&notify_volume);
    main_window.on_volume_changed(move |val| {
        let ch_str = "Headphones".to_string();
        {
            let mut touches = touch_for_vol.lock().unwrap();
            touches.insert(ch_str.clone(), std::time::Instant::now());
            let mut pending = pending_for_vol.lock().unwrap();
            pending.insert(ch_str, val as f64);
        }
        notify_for_vol.notify_one();
    });

    let touch_for_ch_vol = Arc::clone(&last_user_touch);
    let pending_for_ch_vol = Arc::clone(&pending_volumes);
    let notify_for_ch_vol = Arc::clone(&notify_volume);
    main_window.on_channel_volume_changed(move |channel, val| {
        let ch_str = channel.to_string();
        {
            let mut touches = touch_for_ch_vol.lock().unwrap();
            touches.insert(ch_str.clone(), std::time::Instant::now());
            let mut pending = pending_for_ch_vol.lock().unwrap();
            pending.insert(ch_str, val as f64);
        }
        notify_for_ch_vol.notify_one();
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
                    info!("Successfully requested daemon to enforce stream routing policies.");
                    return;
                }
            }
            if let Ok((cfg, _)) = sonomux_core::AppConfig::load_or_create() {
                let keep_off: Vec<&str> = cfg.policies.keep_off_surround.iter().map(|s| s.as_str()).collect();
                let _ = sonomux_backend::Backend::enforce_surround_policies(
                    &cfg.policies.surround_out_node,
                    &cfg.policies.surround_in_node,
                    &cfg.devices.goxlr_game_sink,
                    &cfg.devices.goxlr_system_sink,
                    &keep_off,
                )
                .await;
                let _ = sonomux_backend::Backend::enforce_stream_routing(
                    &cfg.routing,
                    &cfg.devices,
                    &cfg.policies,
                )
                .await;
            }
        });
    });

    let proxy_for_rule = dbus_proxy.clone();
    let weak_rule = main_window.as_weak();
    main_window.on_toggle_rule(move |rule_name| {
        let rule_str = rule_name.to_string();
        let mut new_val = true;
        if let Some(ui) = weak_rule.upgrade() {
            new_val = match rule_str.as_str() {
                "Auto Headset Switcher" => {
                    let v = !ui.get_rule_headset_switcher_enabled();
                    ui.set_rule_headset_switcher_enabled(v);
                    v
                }
                "Browser Stream Isolation" => {
                    let v = !ui.get_rule_surround_isolation_enabled();
                    ui.set_rule_surround_isolation_enabled(v);
                    v
                }
                _ => true,
            };
        }
        let proxy = proxy_for_rule.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                let _ = p.set_rule_enabled(rule_str, new_val).await;
            } else if let Ok((mut cfg, _)) = sonomux_core::AppConfig::load_or_create() {
                if let Some(r) = cfg.rules.iter_mut().find(|r| r.name == rule_str) {
                    r.enabled = new_val;
                    let _ = cfg.save();
                }
            }
        });
    });

    let proxy_for_routing = dbus_proxy.clone();
    let weak_routing = main_window.as_weak();
    main_window.on_toggle_routing_enabled(move || {
        let mut new_val = true;
        if let Some(ui) = weak_routing.upgrade() {
            new_val = !ui.get_routing_auto_enabled();
            ui.set_routing_auto_enabled(new_val);
        }
        let proxy = proxy_for_routing.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                let _ = p.set_routing_enabled(new_val).await;
            } else if let Ok((mut cfg, _)) = sonomux_core::AppConfig::load_or_create() {
                cfg.routing.enabled = new_val;
                let _ = cfg.save();
            }
        });
    });

    let proxy_for_cat = dbus_proxy.clone();
    let weak_cat = main_window.as_weak();
    main_window.on_set_routing_category(move |cat, target| {
        let cat_str = cat.to_string();
        let target_str = target.to_string();
        let cat_lower = cat_str.to_lowercase();
        let target_upper = target_str.to_uppercase();

        if let Some(ui) = weak_cat.upgrade() {
            match cat_lower.as_str() {
                "browser" => ui.set_target_browser(target_upper.clone().into()),
                "game" => ui.set_target_game(target_upper.clone().into()),
                "music" => ui.set_target_music(target_upper.clone().into()),
                "chat" => ui.set_target_chat(target_upper.clone().into()),
                "production" => ui.set_target_production(target_upper.clone().into()),
                "soundboard" => ui.set_target_soundboard(target_upper.clone().into()),
                _ => {}
            }
        }

        let proxy = proxy_for_cat.clone();
        tokio::spawn(async move {
            if let Some(ref p) = proxy {
                let _ = p.set_category_target(cat_lower, target_upper).await;
            } else if let Ok((mut cfg, _)) = sonomux_core::AppConfig::load_or_create() {
                cfg.routing.category_targets.insert(cat_lower, target_upper);
                let _ = cfg.save();
            }
        });
    });

    // Periodic synchronization for rules and routing config
    let sync_weak = main_window.as_weak();
    let sync_proxy = dbus_proxy.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1500));
        loop {
            interval.tick().await;
            if let Some(ref proxy) = sync_proxy {
                if let Ok((enabled, targets)) = proxy.get_routing_config().await {
                    let rules = proxy.get_rules().await.ok();
                    let weak = sync_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_routing_auto_enabled(enabled);
                            for (cat, target) in targets {
                                let t = target.to_uppercase();
                                match cat.to_lowercase().as_str() {
                                    "browser" => ui.set_target_browser(t.into()),
                                    "game" => ui.set_target_game(t.into()),
                                    "music" => ui.set_target_music(t.into()),
                                    "chat" => ui.set_target_chat(t.into()),
                                    "production" => ui.set_target_production(t.into()),
                                    "soundboard" => ui.set_target_soundboard(t.into()),
                                    _ => {}
                                }
                            }
                            if let Some(rule_list) = rules {
                                for (name, rule_enabled) in rule_list {
                                    match name.as_str() {
                                        "Auto Headset Switcher" => ui.set_rule_headset_switcher_enabled(rule_enabled),
                                        "Browser Stream Isolation" => ui.set_rule_surround_isolation_enabled(rule_enabled),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    });

    // Run Slint event loop
    main_window.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_slots_follow_fader_order() {
        let faders: Vec<String> = ["Mic", "Chat", "Music", "System"].map(String::from).into();
        // Mic, System, Game, Chat, Music
        assert_eq!(strip_slots(&faders), [0, 3, 4, 1, 2]);
        // Nothing on faders: default order. Unknown channels are skipped.
        assert_eq!(strip_slots(&[]), [0, 1, 2, 3, 4]);
        let odd: Vec<String> = ["LineIn", "Game"].map(String::from).into();
        assert_eq!(strip_slots(&odd), [1, 2, 0, 3, 4]);
    }

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
