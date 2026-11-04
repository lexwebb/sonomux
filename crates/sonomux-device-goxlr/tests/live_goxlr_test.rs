use sonomux_device_goxlr::{GoXLRClient, DEFAULT_GOXLR_SOCKET};
use std::path::Path;

#[tokio::test]
async fn test_live_goxlr_connection() {
    if !Path::new(DEFAULT_GOXLR_SOCKET).exists() {
        println!(
            "Skipping live test: GoXLR socket not found at {}",
            DEFAULT_GOXLR_SOCKET
        );
        return;
    }

    let client = GoXLRClient::new(DEFAULT_GOXLR_SOCKET);
    let serial = match client.get_serial().await {
        Ok(s) => s,
        Err(e) => {
            println!(
                "Skipping live test: GoXLR daemon running but no hardware device found: {}",
                e
            );
            return;
        }
    };
    println!("Live test connected to GoXLR Serial: {}", serial);
    assert!(!serial.is_empty());

    let status = client.get_status().await.expect("Failed to get status");
    assert!(status.mixers.contains_key(&serial));

    let hp_vol = client
        .get_headphones_volume()
        .await
        .expect("Failed to read headphone volume");
    println!("Current live headphone volume: {}", hp_vol);
}
