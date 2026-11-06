use sonomux_core::traits::{AudioDevice, DeviceStatus};
use sonomux_device_goxlr::GoXLRClient;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

fn sock(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("mock-goxlr-{}-{}.socket", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

async fn read_request(s: &mut tokio::net::UnixStream) {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.unwrap();
    let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut body).await.unwrap();
}

#[tokio::test]
async fn test_request_times_out_when_utility_hangs() {
    let path = sock("silent");
    let listener = UnixListener::bind(&path).unwrap();
    let task = tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                read_request(&mut s).await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            });
        }
    });

    let client = GoXLRClient::new(&path).with_timeout(Duration::from_millis(200));
    let start = Instant::now();
    let res = client.get_status().await;
    let elapsed = start.elapsed();
    let err = res.expect_err("hung utility must error").to_string();
    assert!(err.contains("did not answer"), "got: {err}");
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    assert_eq!(client.status().await, DeviceStatus::Disconnected);

    task.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn test_oversized_reply_is_rejected() {
    let path = sock("huge");
    let listener = UnixListener::bind(&path).unwrap();
    let task = tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                read_request(&mut s).await;
                s.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
                tokio::time::sleep(Duration::from_secs(10)).await;
            });
        }
    });

    let client = GoXLRClient::new(&path);
    let err = client
        .get_status()
        .await
        .expect_err("must reject")
        .to_string();
    assert!(err.contains("too large"), "got: {err}");

    task.abort();
    let _ = std::fs::remove_file(&path);
}
