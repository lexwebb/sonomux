use zbus::proxy;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let connection = match sonomux_core::ipc::connect().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error connecting to sonomuxd: {}", e);
            eprintln!("Is 'sonomuxd' running? Linux: systemctl --user start sonomuxd, macOS: launchctl kickstart gui/$UID/org.sonomux.sonomuxd");
            std::process::exit(1);
        }
    };

    let proxy = match SonomuxDaemonProxy::new(&connection).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to connect to Sonomux daemon over D-Bus: {}", e);
            eprintln!("Please ensure 'sonomuxd' is started.");
            std::process::exit(1);
        }
    };

    let is_json = args.iter().any(|a| a == "--json");
    let cmd = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("status");

    match cmd {
        "status" => match proxy.get_status().await {
            Ok((goxlr_up, hs_status, hs_active, battery, spk_vol, loop_up)) => {
                if is_json {
                    let sum = sonomux_core::status::summarize(
                        goxlr_up, &hs_status, hs_active, battery, spk_vol, loop_up,
                    );
                    let json_out = serde_json::json!({
                        "text": format!("{} {}", sum.indicator.glyph(), sum.label),
                        "alt": sum.indicator.glyph(),
                        "tooltip": format!(
                            "{}\n\nClick: Open Sonomux\nRight-Click: Toggle Mute",
                            sum.lines.join("\n")
                        ),
                        "class": sum.indicator.class(),
                        "percentage": sum.percentage,
                    });
                    println!("{}", json_out);
                    return Ok(());
                }

                println!("🎛️ Sonomux Audio Status:");
                println!(
                    "  GoXLR Hardware:   {}",
                    if goxlr_up {
                        "● Active (Connected)"
                    } else {
                        "○ Disconnected"
                    }
                );
                println!("  Wireless Headset: {} (Active: {})", hs_status, hs_active);
                if hs_active && battery > 0 {
                    println!("  Headset Battery:  {}%", battery);
                } else {
                    println!("  Headset Battery:  Unavailable (Standby)");
                }
                println!("  Desk Speakers:    {:.0}%", spk_vol);
                println!(
                    "  Headset Feed:     {}",
                    if loop_up {
                        "Streaming (Loopback Active)"
                    } else {
                        "Off (Speakers Active)"
                    }
                );
            }
            Err(e) => {
                if is_json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "text": "󰓄 Sonomux Offline",
                            "tooltip": format!("Daemon error: {}", e),
                            "class": "error"
                        })
                    );
                    return Ok(());
                }
                eprintln!("Failed to query status from daemon: {}", e);
                std::process::exit(1);
            }
        },
        "mute" | "toggle-mute" => match proxy.toggle_speaker_mute().await {
            Ok(muted) => {
                if muted {
                    println!("🔇 Desk speakers muted (0%)");
                } else {
                    println!("🔊 Desk speakers unmuted (100%)");
                }
            }
            Err(e) => {
                eprintln!("Failed to toggle speaker mute: {}", e);
                std::process::exit(1);
            }
        },
        "volume" | "vol" => {
            if let Some(val_str) = args.get(1) {
                if let Ok(val) = val_str.parse::<f64>() {
                    let clamped = val.clamp(0.0, 100.0);
                    if let Err(e) = proxy.set_speaker_volume(clamped).await {
                        eprintln!("Failed to set volume: {}", e);
                    } else {
                        println!("🔊 Desk speaker volume set to {:.0}%", clamped);
                    }
                } else {
                    eprintln!("Usage: sonomuxctl volume <0-100>");
                }
            } else {
                eprintln!("Usage: sonomuxctl volume <0-100>");
            }
        }
        "enforce" | "refresh" => {
            if let Err(e) = proxy.enforce_policies().await {
                eprintln!("Failed to enforce policies: {}", e);
            } else {
                println!("✅ Audio stream policies enforced successfully.");
            }
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        unknown => {
            eprintln!("Unknown command: '{}'", unknown);
            print_help();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_help() {
    println!("Sonomux Audio CLI (sonomuxctl)\n");
    println!("USAGE:");
    println!("  sonomuxctl [COMMAND]\n");
    println!("COMMANDS:");
    println!(
        "  status         Display current GoXLR, headset, volume, and routing state (default)"
    );
    println!("  mute           Toggle mute on desk speakers (GoXLR headphone output)");
    println!("  volume <N>     Set desk speaker volume (0 to 100)");
    println!("  enforce        Re-enforce Virtual Surround Manager and stream isolation policies");
    println!("  help           Show this help message");
}
