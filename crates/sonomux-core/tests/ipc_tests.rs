//! Own test binary: it sets XDG_STATE_HOME, which must not race other tests.

use sonomux_core::ipc;

struct Ping;

#[zbus::interface(name = "org.sonomux.Test")]
impl Ping {
    fn ping(&self) -> String {
        "pong".into()
    }
}

#[tokio::test]
async fn socket_path_and_connect() {
    let dir = std::env::temp_dir().join(format!("sonomux-ipc-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_STATE_HOME", &dir) };
    let path = ipc::socket_path();
    assert_eq!(path, dir.join("sonomux").join("sonomuxd.sock"));

    // Linux connects to the session bus, which CI may not have; only macOS uses the socket.
    #[cfg(target_os = "macos")]
    {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = tokio::task::spawn_blocking(move || listener.accept().unwrap().0);

        let client = tokio::spawn(ipc::connect());
        let stream = server.await.unwrap();
        let _server_conn = zbus::connection::Builder::async_io_unix_stream(stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(ipc::OBJECT_PATH, Ping)
            .unwrap()
            .build()
            .await
            .unwrap();
        let conn = client.await.unwrap().expect("connect over socket");

        let reply: String = conn
            .call_method(
                None::<&str>,
                ipc::OBJECT_PATH,
                Some("org.sonomux.Test"),
                "Ping",
                &(),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(reply, "pong");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
