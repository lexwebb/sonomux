use serde_json::json;
use sonomux_core::traits::{AudioDevice, DeviceStatus, HasHardwareVolume, HasRoutingMatrix};
use sonomux_device_goxlr::{Channel, GoXLRClient, InputDevice, OutputDevice};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

#[tokio::test]
async fn test_goxlr_with_mock_server() {
    let temp_dir = std::env::temp_dir();
    let socket_path = temp_dir.join(format!("mock-goxlr-{}.socket", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind mock socket");
    let received_commands = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&received_commands);

    // Spawn mock server task
    let server_task = tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                let cnt = Arc::clone(&counter);
                tokio::spawn(async move {
                    // Read length prefix (4 bytes BE)
                    let mut len_buf = [0u8; 4];
                    if stream.read_exact(&mut len_buf).await.is_err() {
                        return;
                    }
                    let req_len = u32::from_be_bytes(len_buf) as usize;
                    let mut req_buf = vec![0u8; req_len];
                    if stream.read_exact(&mut req_buf).await.is_err() {
                        return;
                    }

                    cnt.fetch_add(1, Ordering::SeqCst);
                    let req_str = String::from_utf8_lossy(&req_buf);

                    let resp_payload = if req_str.contains("GetStatus") {
                        json!({
                            "Status": {
                                "mixers": {
                                    "MOCK_SERIAL_123": {
                                        "levels": {
                                            "volumes": {
                                                "Headphones": 200,
                                                "Game": 150
                                            }
                                        },
                                        "router": {
                                            "Microphone": {
                                                "Sampler": false,
                                                "Headphones": true
                                            }
                                        }
                                    }
                                }
                            }
                        })
                    } else {
                        // Default "Ok" for SetVolume and SetRouter
                        json!("Ok")
                    };

                    let resp_bytes = serde_json::to_vec(&resp_payload).unwrap();
                    let resp_len = (resp_bytes.len() as u32).to_be_bytes();
                    let _ = stream.write_all(&resp_len).await;
                    let _ = stream.write_all(&resp_bytes).await;
                    let _ = stream.flush().await;
                });
            }
        }
    });

    let mut client = GoXLRClient::new(&socket_path);

    // 1. AudioDevice trait
    assert_eq!(client.metadata().id, "goxlr");
    assert_eq!(client.status().await, DeviceStatus::Active);
    let _rx = client.subscribe_events().await;

    // 2. Serial discovery
    let serial = client
        .get_serial()
        .await
        .expect("Failed to get serial from mock");
    assert_eq!(serial, "MOCK_SERIAL_123");

    // 3. Headphone volume query
    let hp_vol = client
        .get_headphones_volume()
        .await
        .expect("Failed to read volume");
    assert_eq!(hp_vol, 200);

    // 4. Volume commands
    client
        .set_volume_raw(Channel::Headphones, 0)
        .await
        .expect("Failed to set volume");

    // 5. HasHardwareVolume trait
    let norm_vol = client
        .get_channel_volume("Headphones")
        .await
        .expect("Failed to get channel volume");
    assert!((norm_vol - (200.0 / 255.0)).abs() < 0.01);
    client
        .set_channel_volume("Headphones", 0.5)
        .await
        .expect("Failed to set normalized volume");
    client
        .set_channel_mute("Headphones", true)
        .await
        .expect("Failed to mute channel");
    client
        .set_channel_mute("Headphones", false)
        .await
        .expect("Failed to unmute channel");

    // 6. HasRoutingMatrix trait
    assert!(!client.available_inputs().await.is_empty());
    assert!(!client.available_outputs().await.is_empty());
    let routed = client
        .is_routed("Microphone", "Headphones")
        .await
        .expect("Failed to check routing");
    assert!(routed);
    let not_routed = client
        .is_routed("Microphone", "Sampler")
        .await
        .expect("Failed to check routing");
    assert!(!not_routed);
    HasRoutingMatrix::set_route(&mut client, "Microphone", "Sampler", true)
        .await
        .expect("Failed to set trait route");

    // 7. Sampler routing matrix setup
    client
        .setup_sampler_routing()
        .await
        .expect("Failed to setup sampler submix routing");

    // Clean up
    server_task.abort();
    let _ = std::fs::remove_file(&socket_path);

    assert!(received_commands.load(Ordering::SeqCst) >= 8);
}

#[tokio::test]
async fn test_goxlr_disconnected_handling() {
    let non_existent = std::path::PathBuf::from("/tmp/sonomux_nonexistent_test.socket");
    let mut client = GoXLRClient::new(&non_existent);

    assert_eq!(client.status().await, DeviceStatus::Disconnected);
    assert!(client.get_serial().await.is_err());
    assert!(client.get_channel_volume("Headphones").await.is_err());
    assert!(client.set_channel_volume("Headphones", 0.5).await.is_err());
    assert!(client.set_channel_mute("Headphones", true).await.is_err());
    assert!(client.is_routed("Microphone", "Headphones").await.is_err());
    assert!(
        HasRoutingMatrix::set_route(&mut client, "Microphone", "Headphones", true)
            .await
            .is_err()
    );
    assert!(client.setup_sampler_routing().await.is_err());
}

#[tokio::test]
async fn test_goxlr_default_constructor() {
    let client = GoXLRClient::default();
    assert_eq!(client.metadata().id, "goxlr");
}

#[test]
fn test_channel_as_str_all() {
    let channels = [
        Channel::Mic,
        Channel::LineIn,
        Channel::Console,
        Channel::System,
        Channel::Game,
        Channel::Chat,
        Channel::Sample,
        Channel::Music,
        Channel::Headphones,
        Channel::MicMonitor,
        Channel::LineOut,
    ];
    for ch in channels {
        assert!(!ch.as_str().is_empty());
    }

    let inputs = [
        InputDevice::Microphone,
        InputDevice::Chat,
        InputDevice::Music,
        InputDevice::Game,
        InputDevice::Console,
        InputDevice::LineIn,
        InputDevice::System,
        InputDevice::Samples,
    ];
    for inp in inputs {
        assert!(!inp.as_str().is_empty());
    }

    let outputs = [
        OutputDevice::Headphones,
        OutputDevice::BroadcastMix,
        OutputDevice::ChatMic,
        OutputDevice::Sampler,
        OutputDevice::LineOut,
        OutputDevice::StreamMix2,
    ];
    for out in outputs {
        assert!(!out.as_str().is_empty());
    }
}
