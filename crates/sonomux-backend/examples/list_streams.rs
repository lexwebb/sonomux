//! Prints what the macOS backend sees: each app playing audio, its category, and its device.
#[cfg(target_os = "macos")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cfg = sonomux_core::AppConfig::load_or_create().unwrap().0;
    for s in sonomux_backend::Backend::get_active_streams()
        .await
        .unwrap()
    {
        let cat = sonomux_core::AppClassifier::classify(&s, &cfg.routing.app_overrides);
        println!(
            "{:>6} {:<38} {:<30} {:<8} {}",
            s.pid.unwrap_or(0),
            s.app_name,
            s.binary,
            cat.to_string(),
            s.sink_name
        );
    }
    println!("--- all processes with audio clients (object, pid, bundle):");
    for (obj, pid, bundle) in sonomux_backend::coreaudio::CoreAudioManager::audio_processes() {
        println!("{:>6} {:>6} {}", obj, pid, bundle);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
