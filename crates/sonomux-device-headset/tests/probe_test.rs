use sonomux_device_headset::HeadsetMonitor;

#[tokio::test]
async fn test_headset_probe() {
    match HeadsetMonitor::probe_raw().await {
        Ok(telemetry) => {
            println!("Headset live probe result: {:?}", telemetry);
        }
        Err(e) => {
            println!("headsetcontrol not available or error: {}", e);
        }
    }
}
