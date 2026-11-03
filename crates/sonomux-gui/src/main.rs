use slint::slint;

slint! {
    import { Button, Slider, VerticalBox, HorizontalBox, GroupBox } from "std-widgets.slint";

    export component MainWindow inherits Window {
        title: "Sonomux Audio Coordinator";
        min-width: 500px;
        min-height: 580px;
        background: #181926;

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
                Text {
                    text: "Sonomux 🎛️";
                    font-size: 22px;
                    font-weight: 700;
                    color: #cad3f5;
                }
                Text {
                    text: "v0.1.0";
                    font-size: 14px;
                    color: #8087a2;
                    vertical-alignment: center;
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
                        text: "Persistent USB dongle detected. Presence evaluated via HID telemetry.";
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
                        text: "• Sampler Feed -> Headset Loopback (When Active)";
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let main_window = MainWindow::new()?;

    let goxlr = sonomux_device_goxlr::GoXLRClient::new(sonomux_device_goxlr::DEFAULT_GOXLR_SOCKET);
    let _headset = sonomux_device_headset::HeadsetMonitor::new("SteelSeries Arctis Nova 7", 2);

    // Initial state query
    if let Ok(serial) = goxlr.get_serial().await {
        main_window.set_goxlr_status("Active (Connected)".into());
        main_window.set_goxlr_serial(serial.into());
    } else {
        main_window.set_goxlr_status("Disconnected".into());
    }

    if let Ok(hp_vol) = goxlr.get_headphones_volume().await {
        let pct = (hp_vol as f32 / 255.0) * 100.0;
        main_window.set_speaker_volume(pct);
    }

    // Probe headset status
    if let Ok(telemetry) = sonomux_device_headset::HeadsetMonitor::probe_raw().await {
        if telemetry.is_active {
            main_window.set_headset_status("Active (Powered On)".into());
            let bat_str = telemetry
                .battery_percent
                .map(|b| format!("{}%", b))
                .unwrap_or_else(|| "Connected".into());
            main_window.set_headset_battery(bat_str.into());
        } else {
            main_window.set_headset_status("Standby (Headset Off)".into());
            main_window.set_headset_battery("Unavailable".into());
        }
    }

    // Callbacks
    let goxlr_clone = goxlr.clone();
    main_window.on_volume_changed(move |val| {
        let client = goxlr_clone.clone();
        tokio::spawn(async move {
            let raw = ((val / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let _ = client
                .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, raw)
                .await;
        });
    });

    let goxlr_clone2 = goxlr.clone();
    let handle = main_window.as_weak();
    main_window.on_toggle_speakers(move || {
        let client = goxlr_clone2.clone();
        let weak = handle.clone();
        tokio::spawn(async move {
            if let Ok(curr) = client.get_headphones_volume().await {
                let target = if curr > 0 { 0 } else { 255 };
                let _ = client
                    .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, target)
                    .await;
                if let Some(ui) = weak.upgrade() {
                    ui.set_speaker_volume(if target > 0 { 100.0 } else { 0.0 });
                }
            }
        });
    });

    main_window.on_force_routing_refresh(|| {
        tokio::spawn(async {
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
