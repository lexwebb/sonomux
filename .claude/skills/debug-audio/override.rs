// Set or clear an app routing override through the daemon's IPC: override <app> <target>
// (target "" clears). Copy to crates/sonomux-daemon/examples/ to build; never commit it there.
#[zbus::proxy(
    interface = "org.sonomux.Daemon",
    default_service = "org.sonomux.Daemon",
    default_path = "/org/sonomux/Daemon"
)]
trait D {
    async fn set_app_override(&self, id: String, target: String) -> zbus::Result<()>;
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let conn = sonomux_core::ipc::connect().await.unwrap();
    DProxy::new(&conn)
        .await
        .unwrap()
        .set_app_override(a[1].clone(), a[2].clone())
        .await
        .unwrap();
}
