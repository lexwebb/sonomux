//! Prototype: route one app to one GoXLR channel via a process tap.
//! Usage: cargo run -p sonomux-backend --example tap_route -- <bundle-id-prefix|pid> <channel> [seconds]
//! Channels: system game chat music sample (GoXLRMini output pairs 1/2 .. 9/10).

#[cfg(target_os = "macos")]
fn main() {
    use sonomux_backend::coreaudio::CoreAudioManager;
    use sonomux_backend::coreaudio_tap::{AppRoute, TapSource};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (who, channel) = (args[0].as_str(), args[1].as_str());
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    let first = match channel {
        "system" => 0,
        "game" => 2,
        "chat" => 4,
        "music" => 6,
        "sample" => 8,
        _ => panic!("unknown channel {channel}"),
    };

    let procs: Vec<u32> = CoreAudioManager::audio_processes()
        .into_iter()
        .filter(|(_, pid, bundle)| pid.to_string() == who || bundle.starts_with(who))
        .map(|(p, pid, bundle)| {
            println!("tapping process object {p} (pid {pid}, {bundle})");
            p
        })
        .collect();
    let goxlr = CoreAudioManager::devices()
        .into_iter()
        .find(|d| d.name == "GoXLRMini")
        .expect("GoXLRMini not connected");

    // Tap the app where it plays now, at full width, reading its device's preferred pair.
    let src = CoreAudioManager::process_output_device(procs[0]).expect("app is not playing");
    let src_first = CoreAudioManager::preferred_stereo_first(src.id);
    println!(
        "app plays on {} (channels {}/{})",
        src.name,
        src_first + 1,
        src_first + 2
    );
    let source = if std::env::var_os("MIXDOWN").is_some() {
        TapSource::Mixdown
    } else {
        TapSource::Device {
            uid: &src.uid,
            first: src_first,
        }
    };
    let route = AppRoute::start(&procs, source, &goxlr.uid, first).expect("route failed");
    println!(
        "routing to {} channels {}/{} for {secs}s",
        goxlr.name,
        first + 1,
        first + 2
    );
    for _ in 0..secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let (frames, peak) = route.stats();
        println!("frames={frames} peak={peak:.3}");
    }
    drop(route);
    println!("route removed");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
