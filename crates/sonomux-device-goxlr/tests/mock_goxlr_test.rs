use serde_json::json;
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
                                                "Sampler": false
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

    let client = GoXLRClient::new(&socket_path);

    // 1. Test status & serial discovery
    let serial = client.get_serial().await.expect("Failed to get serial from mock");
    assert_eq!(serial, "MOCK_SERIAL_123");

    // 2. Test headphone volume query
    let hp_vol = client.get_headphones_volume().await.expect("Failed to read volume");
    assert_eq!(hp_vol, 200);

    // 3. Test volume command
    client.set_volume_raw(Channel::Headphones, 0).await.expect("Failed to set volume");

    // 4. Test route command
    client.set_route(InputDevice::Microphone, OutputDevice::Sampler, false).await.expect("Failed to set route");

    // Clean up
    server_task.abort();
    let _ = std::fs::remove_file(&socket_path);

    assert!(received_commands.load(Ordering::SeqCst) >= 4);
}

#[test]
fn test_channel_as_str() {
    assert_eq!(Channel::Headphones.as_str(), "Headphones");
    assert_eq!(Channel::Mic.as_str(), "Mic");
    assert_eq!(Channel::Game.as_str(), "Game");
    assert_eq!(InputDevice::Microphone.as_str(), "Microphone");
    assert_eq!(OutputDevice::Sampler.as_str(), "Sampler");
}
