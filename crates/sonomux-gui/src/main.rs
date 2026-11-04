//! Sonomux GUI: a native-styled window over the daemon. The layout lives in ui/app.slint;
//! build.rs picks the platform's widget style.

slint::include_modules!();

mod hardware;

use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::info;
use zbus::proxy;

#[proxy(
    interface = "org.sonomux.Daemon",
    default_service = "org.sonomux.Daemon",
    default_path = "/org/sonomux/Daemon"
)]
trait SonomuxDaemon {
    async fn get_status(&self) -> zbus::Result<(bool, String, bool, u8, f64, bool)>;
    async fn get_channel_volumes(&self) -> zbus::Result<HashMap<String, f64>>;
    async fn set_channel_volume(&self, channel: String, volume_pct: f64) -> zbus::Result<()>;
    async fn toggle_speaker_mute(&self) -> zbus::Result<bool>;
    async fn get_channel_mutes(&self) -> zbus::Result<HashMap<String, bool>>;
    async fn set_channel_muted(&self, channel: String, muted: bool) -> zbus::Result<()>;
    async fn get_routing_config(&self) -> zbus::Result<(bool, HashMap<String, String>)>;
    async fn set_routing_enabled(&self, enabled: bool) -> zbus::Result<()>;
    async fn set_category_target(&self, category: String, target: String) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)] // tuple mirrors the daemon's D-Bus signature
    async fn get_apps(&self) -> zbus::Result<Vec<(String, String, String, String, bool)>>;
    async fn set_app_override(&self, id: String, target: String) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)]
    async fn get_audio_devices(&self) -> zbus::Result<(Vec<String>, Vec<String>, String, String)>;
    async fn get_device_settings(&self) -> zbus::Result<(bool, bool)>;
    async fn set_default_device(&self, name: String, input: bool) -> zbus::Result<()>;
    async fn set_keep_defaults(&self, keep: bool) -> zbus::Result<()>;
    async fn set_split_unpaired(&self, enabled: bool) -> zbus::Result<()>;
    #[allow(clippy::type_complexity)]
    async fn get_split_devices(
        &self,
    ) -> zbus::Result<(Vec<(String, String, String, String)>, Vec<(String, String)>)>;
    async fn get_rules(&self) -> zbus::Result<Vec<(String, bool)>>;
    async fn set_rule_enabled(&self, rule_name: String, enabled: bool) -> zbus::Result<()>;
}

/// Searches Hyprland's clients JSON for an existing Sonomux window address.
pub fn find_existing_hyprland_address(clients_json: &str) -> Option<String> {
    let clients: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let array = clients.as_array()?;
    for client in array {
        let title = client.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let init_title = client
            .get("initialTitle")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let class = client.get("class").and_then(|v| v.as_str()).unwrap_or("");
        let init_class = client
            .get("initialClass")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if title == "Sonomux Audio Coordinator"
            || init_title == "Sonomux Audio Coordinator"
            || class == "sonomux"
            || init_class == "sonomux"
        {
            if let Some(addr) = client.get("address").and_then(|v| v.as_str()) {
                return Some(addr.to_string());
            }
        }
    }
    None
}

/// Input strip channels, in their order when none are on a fader.
const STRIP_CHANNELS: [&str; 5] = ["Mic", "System", "Game", "Chat", "Music"];

/// Slot (0-4) for each of `STRIP_CHANNELS`: fader channels first, in fader order (A-D),
/// then the rest in default order.
pub fn strip_slots(fader_channels: &[String]) -> [i32; 5] {
    let mut order: Vec<&str> = fader_channels
        .iter()
        .map(|c| c.as_str())
        .filter(|c| STRIP_CHANNELS.contains(c))
        .collect();
    order.dedup();
    for ch in STRIP_CHANNELS {
        if !order.contains(&ch) {
            order.push(ch);
        }
    }
    STRIP_CHANNELS.map(|ch| order.iter().position(|&c| c == ch).unwrap() as i32)
}

/// Attempts to focus an existing window if Sonomux is already running.
/// Returns true if an existing window was found and focused.
pub fn try_focus_existing_instance() -> bool {
    // 1. Hyprland compositor
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Ok(output) = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
        {
            if output.status.success() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Some(addr) = find_existing_hyprland_address(&json_str) {
                        info!(
                            "Found existing Sonomux window in Hyprland at {}, focusing...",
                            addr
                        );
                        // Try modern Lua dispatch first (Hyprland >= 0.56)
                        let lua_cmd = format!("hl.dsp.focus({{ window = \"address:{}\" }})", addr);
                        let status = std::process::Command::new("hyprctl")
                            .args(["dispatch", &lua_cmd])
                            .status();

                        if let Ok(s) = status {
                            if s.success() {
                                return true;
                            }
                        }

                        // Fallback to classic focuswindow
                        let _ = std::process::Command::new("hyprctl")
                            .args(["dispatch", "focuswindow", &format!("address:{}", addr)])
                            .status();
                        return true;
                    }
                }
            }
        }
    }

    // 2. Generic X11 / xdotool fallback
    if let Ok(status) = std::process::Command::new("xdotool")
        .args([
            "search",
            "--name",
            "Sonomux Audio Coordinator",
            "windowactivate",
        ])
        .status()
    {
        if status.success() {
            info!("Found existing Sonomux window via xdotool, focused.");
            return true;
        }
    }

    false
}

/// Channels an app or category can be sent to. Linux also has the surround stage.
fn channel_choices() -> Vec<&'static str> {
    let mut v = vec!["System", "Game", "Chat", "Music", "Sample"];
    if !cfg!(target_os = "macos") {
        v.push("Surround");
    }
    v
}

/// Routing categories shown under "Category defaults": (config key, label).
const CATEGORIES: [(&str, &str); 6] = [
    ("browser", "Browser"),
    ("music", "Music"),
    ("chat", "Chat"),
    ("game", "Game"),
    ("soundboard", "Soundboard"),
    ("system", "Everything else"),
];

/// Position of `value` in `list`, ignoring case; 0 if absent.
fn index_of(list: &[&str], value: &str) -> i32 {
    list.iter()
        .position(|c| c.eq_ignore_ascii_case(value))
        .unwrap_or(0) as i32
}

/// Fader letter per input strip channel, from the fader order (A, B, C, D).
fn fader_letters(fader_channels: &[String]) -> HashMap<String, String> {
    fader_channels
        .iter()
        .zip(["A", "B", "C", "D"])
        .map(|(c, l)| (c.clone(), l.to_string()))
        .collect()
}

/// Plain data for the slow-changing parts of the window. Compared between polls so list
/// models are only replaced when something changed (replacing resets open pop-ups).
#[derive(Clone, PartialEq, Default)]
struct Slow {
    faders: Vec<String>,
    mutes: Vec<(String, bool)>,
    headset_present: bool,
    routing_auto: bool,
    /// (name, id, category, choices, choice, playing)
    apps: Vec<(String, String, String, Vec<String>, i32, bool)>,
    categories: Vec<i32>,
    outputs: Vec<String>,
    inputs: Vec<String>,
    default_out: i32,
    default_in: i32,
    keep_defaults: bool,
    split_unpaired: bool,
    split_title: String,
    splits: Vec<(String, String, String)>,
    other: Vec<(String, String)>,
    rules: Vec<(String, bool)>,
    /// (helper index, device, status, button label)
    helpers: Vec<(i32, String, String, String)>,
}

/// In-progress or failed helper installs, by helper index, shown instead of the usual status.
type InstallNotes = Arc<Mutex<HashMap<usize, String>>>;

fn helper_rows(notes: &InstallNotes) -> Vec<(i32, String, String, String)> {
    let notes = notes.lock().unwrap();
    hardware::detect()
        .into_iter()
        .map(|(i, installed)| {
            let h = &hardware::HELPERS[i];
            let (status, action) = if installed {
                (format!("Connected · {} installed", h.name), String::new())
            } else if let Some(note) = notes.get(&i) {
                (note.clone(), String::new())
            } else {
                (
                    format!("Connected · needs {} for full support", h.name),
                    hardware::action_label(h),
                )
            };
            (i as i32, h.device.to_string(), status, action)
        })
        .collect()
}

async fn read_slow(
    proxy: Option<&SonomuxDaemonProxy<'_>>,
    goxlr: &sonomux_device_goxlr::GoXLRClient,
    headset_sink: &str,
    notes: &InstallNotes,
) -> Slow {
    let mut s = Slow {
        faders: goxlr.get_fader_channels().await.unwrap_or_default(),
        headset_present: sonomux_backend::Backend::sink_exists(headset_sink).await,
        helpers: helper_rows(notes),
        ..Default::default()
    };
    let mutes = match proxy {
        Some(p) => p.get_channel_mutes().await.unwrap_or_default(),
        None => goxlr.get_channel_mutes().await.unwrap_or_default(),
    };
    s.mutes = mutes.into_iter().collect();
    s.mutes.sort();

    let Some(p) = proxy else { return s };
    let channels = channel_choices();
    if let Ok((enabled, targets)) = p.get_routing_config().await {
        s.routing_auto = enabled;
        s.categories = CATEGORIES
            .iter()
            .map(|(k, _)| index_of(&channels, targets.get(*k).map_or("System", |t| t)))
            .collect();
        if let Ok(apps) = p.get_apps().await {
            s.apps = apps
                .into_iter()
                .map(|(name, id, category, target, playing)| {
                    let auto = targets
                        .get(&category.to_lowercase())
                        .cloned()
                        .unwrap_or_else(|| "System".into());
                    let mut choices = vec![format!("Automatic ({})", auto)];
                    choices.extend(channels.iter().map(|c| c.to_string()));
                    let choice = if target.is_empty() {
                        0
                    } else {
                        1 + index_of(&channels, &target)
                    };
                    (name, id, category, choices, choice, playing)
                })
                .collect();
        }
    }
    if let Ok((outs, ins, dout, din)) = p.get_audio_devices().await {
        let pos = |list: &[String], v: &str| list.iter().position(|d| d == v).unwrap_or(0) as i32;
        s.default_out = pos(&outs, &dout);
        s.default_in = pos(&ins, &din);
        s.outputs = outs;
        s.inputs = ins;
    }
    if let Ok((keep, unpaired)) = p.get_device_settings().await {
        s.keep_defaults = keep;
        s.split_unpaired = unpaired;
    }
    if let Ok((rows, other)) = p.get_split_devices().await {
        let hw: Vec<&String> = rows.iter().map(|r| &r.0).collect();
        s.split_title = match hw.first() {
            Some(h) => format!("{} · {} channel devices", h, rows.len()),
            None => "Channel devices".into(),
        };
        s.splits = rows.into_iter().map(|(_, d, p, r)| (d, p, r)).collect();
        s.other = other;
    }
    s.rules = p.get_rules().await.unwrap_or_default();
    s
}

fn apply_slow(ui: &MainWindow, s: &Slow, prev: Option<&Slow>) {
    let [mic, system, game, chat, music] = strip_slots(&s.faders);
    ui.set_mic_slot(mic);
    ui.set_system_slot(system);
    ui.set_game_slot(game);
    ui.set_chat_slot(chat);
    ui.set_music_slot(music);
    let letters = fader_letters(&s.faders);
    let letter = |c: &str| SharedString::from(letters.get(c).cloned().unwrap_or_default());
    ui.set_mic_fader(letter("Mic"));
    ui.set_chat_fader(letter("Chat"));
    ui.set_music_fader(letter("Music"));
    ui.set_system_fader(letter("System"));
    ui.set_game_fader(letter("Game"));
    let muted = |c: &str| s.mutes.iter().any(|(n, m)| n == c && *m);
    ui.set_mic_muted(muted("Mic"));
    ui.set_chat_muted(muted("Chat"));
    ui.set_music_muted(muted("Music"));
    ui.set_system_muted(muted("System"));
    ui.set_game_muted(muted("Game"));
    ui.set_headset_present(s.headset_present);

    ui.set_routing_auto(s.routing_auto);
    ui.set_keep_defaults(s.keep_defaults);
    ui.set_split_unpaired(s.split_unpaired);
    ui.set_split_title(s.split_title.clone().into());

    // Lists: only replace when their data changed.
    if prev.is_none_or(|p| p.apps != s.apps) {
        let rows: Vec<AppRow> = s
            .apps
            .iter()
            .map(|(name, id, category, choices, choice, playing)| AppRow {
                name: name.into(),
                id: id.into(),
                category: category.into(),
                choices: ModelRc::new(VecModel::from(
                    choices.iter().map(SharedString::from).collect::<Vec<_>>(),
                )),
                choice: *choice,
                playing: *playing,
            })
            .collect();
        ui.set_apps(ModelRc::new(VecModel::from(rows)));
    }
    if prev.is_none_or(|p| p.categories != s.categories) {
        let rows: Vec<CategoryRow> = CATEGORIES
            .iter()
            .zip(s.categories.iter().chain(std::iter::repeat(&0)))
            .map(|((key, label), choice)| CategoryRow {
                key: (*key).into(),
                label: (*label).into(),
                choice: *choice,
            })
            .collect();
        ui.set_categories(ModelRc::new(VecModel::from(rows)));
    }
    if prev.is_none_or(|p| p.outputs != s.outputs || p.default_out != s.default_out) {
        ui.set_output_devices(strings(&s.outputs));
        ui.set_default_output_index(s.default_out);
    }
    if prev.is_none_or(|p| p.inputs != s.inputs || p.default_in != s.default_in) {
        ui.set_input_devices(strings(&s.inputs));
        ui.set_default_input_index(s.default_in);
    }
    if prev.is_none_or(|p| p.splits != s.splits) {
        ui.set_split_rows(table(
            s.splits
                .iter()
                .map(|(d, p, r)| vec![d.as_str(), p.as_str(), r.as_str()]),
        ));
    }
    if prev.is_none_or(|p| p.other != s.other) {
        ui.set_other_rows(table(
            s.other
                .iter()
                .map(|(n, note)| vec![n.as_str(), note.as_str()]),
        ));
    }
    if prev.is_none_or(|p| p.helpers != s.helpers) {
        let rows: Vec<HelperRow> = s
            .helpers
            .iter()
            .map(|(index, device, status, action)| HelperRow {
                index: *index,
                device: device.into(),
                status: status.into(),
                action: action.into(),
            })
            .collect();
        ui.set_helpers(ModelRc::new(VecModel::from(rows)));
    }
    if prev.is_none_or(|p| p.rules != s.rules) {
        let rows: Vec<RuleRow> = s
            .rules
            .iter()
            .map(|(n, e)| RuleRow {
                name: n.into(),
                enabled: *e,
            })
            .collect();
        ui.set_rules(ModelRc::new(VecModel::from(rows)));
    }
}

/// Rows of text cells for a StandardTableView.
fn table<'a>(rows: impl Iterator<Item = Vec<&'a str>>) -> ModelRc<ModelRc<StandardListViewItem>> {
    let rows: Vec<ModelRc<StandardListViewItem>> = rows
        .map(|cells| {
            ModelRc::new(VecModel::from(
                cells
                    .into_iter()
                    .map(StandardListViewItem::from)
                    .collect::<Vec<_>>(),
            ))
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn strings(v: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        v.iter().map(SharedString::from).collect::<Vec<_>>(),
    ))
}

/// Sets one channel's volume property on the window.
type VolumeSetter = fn(&MainWindow, f32);

fn goxlr_channel(name: &str) -> Option<sonomux_device_goxlr::Channel> {
    use sonomux_device_goxlr::Channel;
    Some(match name {
        "System" => Channel::System,
        "Game" => Channel::Game,
        "Chat" => Channel::Chat,
        "Music" => Channel::Music,
        "Mic" => Channel::Mic,
        "Headphones" => Channel::Headphones,
        _ => return None,
    })
}

/// Run `f` with the daemon proxy on the Tokio runtime, then refresh the slow data soon.
fn with_proxy<F, Fut>(proxy: &Option<SonomuxDaemonProxy<'static>>, refresh: &Arc<Notify>, f: F)
where
    F: FnOnce(SonomuxDaemonProxy<'static>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    if let Some(p) = proxy.clone() {
        let refresh = Arc::clone(refresh);
        tokio::spawn(async move {
            f(p).await;
            refresh.notify_one();
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    if try_focus_existing_instance() {
        info!("Existing Sonomux instance focused. Exiting duplicate process.");
        return Ok(());
    }

    // Skia draws text far better than the default renderer, which looks rough on non-Retina
    // screens. macOS only: on Linux the Qt style needs Slint's Qt backend. SLINT_BACKEND wins.
    #[cfg(target_os = "macos")]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .select()?;
    }

    let ui = MainWindow::new()?;
    ui.set_devices_supported(cfg!(target_os = "macos"));
    // Start on another screen (0 Mixer, 1 App routing, 2 Devices, 3 Rules), e.g. for screenshots.
    if let Some(page) = std::env::var("SONOMUX_PAGE")
        .ok()
        .and_then(|p| p.parse().ok())
    {
        ui.set_page(page);
    }
    ui.set_channel_choices(strings(
        &channel_choices()
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>(),
    ));

    let goxlr = sonomux_device_goxlr::GoXLRClient::new(sonomux_device_goxlr::DEFAULT_GOXLR_SOCKET);
    let headset_sink = sonomux_core::AppConfig::load_or_create()
        .map(|(cfg, _)| cfg.devices.headset_sink)
        .unwrap_or_default();

    // The daemon may still be starting (e.g. right after login or install).
    let mut connection = None;
    for _ in 0..10 {
        connection = sonomux_core::ipc::connect().await.ok();
        if connection.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let proxy: Option<SonomuxDaemonProxy<'static>> = match connection {
        Some(ref conn) => SonomuxDaemonProxy::new(conn).await.ok(),
        None => None,
    };
    ui.set_daemon_online(proxy.is_some());

    // Volume moves: coalesced and sent in order, so a fast drag doesn't flood the GoXLR.
    let touched: Arc<Mutex<HashMap<String, Instant>>> = Arc::default();
    let pending: Arc<Mutex<HashMap<String, f64>>> = Arc::default();
    let send_now = Arc::new(Notify::new());
    {
        let (proxy, goxlr, pending, send_now) = (
            proxy.clone(),
            goxlr.clone(),
            Arc::clone(&pending),
            Arc::clone(&send_now),
        );
        tokio::spawn(async move {
            loop {
                send_now.notified().await;
                tokio::time::sleep(Duration::from_millis(15)).await;
                let batch: Vec<(String, f64)> = pending.lock().unwrap().drain().collect();
                for (ch, pct) in batch {
                    if let Some(ref p) = proxy {
                        if p.set_channel_volume(ch.clone(), pct).await.is_ok() {
                            continue;
                        }
                    }
                    if let Some(c) = goxlr_channel(&ch) {
                        let raw = ((pct / 100.0) * 255.0).round().clamp(0.0, 255.0) as u8;
                        let _ = goxlr.set_volume_raw(c, raw).await;
                    }
                }
            }
        });
    }

    // Fast poll: volumes and status (the daemon reads the GoXLR every 50 ms).
    {
        let (weak, proxy, goxlr, touched) = (
            ui.as_weak(),
            proxy.clone(),
            goxlr.clone(),
            Arc::clone(&touched),
        );
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tick.tick().await;
                let mut status = None;
                let vols: Option<HashMap<String, f64>> = match proxy {
                    Some(ref p) => {
                        status = p.get_status().await.ok();
                        p.get_channel_volumes().await.ok()
                    }
                    None => goxlr.get_all_volumes().await.ok().map(|m| {
                        m.into_iter()
                            .map(|(k, v)| (k, v as f64 / 255.0 * 100.0))
                            .collect()
                    }),
                };
                let goxlr_up = status.as_ref().map_or(vols.is_some(), |s| s.0);
                // Don't fight a strip the user is dragging.
                let busy: Vec<String> = touched
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, t)| t.elapsed() < Duration::from_millis(350))
                    .map(|(k, _)| k.clone())
                    .collect();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_goxlr_online(goxlr_up);
                    if let Some((_, _, hs_active, battery, _, _)) = status {
                        ui.set_headset_active(hs_active);
                        ui.set_headset_battery(format!("{}%", battery).into());
                    }
                    let Some(v) = vols else { return };
                    let get = |c: &str| v.get(c).map(|x| *x as f32);
                    let free = |c: &str| !busy.iter().any(|b| b == c);
                    let setters: [(&str, VolumeSetter); 6] = [
                        ("Mic", MainWindow::set_mic_volume),
                        ("Chat", MainWindow::set_chat_volume),
                        ("Music", MainWindow::set_music_volume),
                        ("System", MainWindow::set_system_volume),
                        ("Game", MainWindow::set_game_volume),
                        ("Headphones", MainWindow::set_speaker_volume),
                    ];
                    for (ch, set) in setters {
                        if let (true, Some(x)) = (free(ch), get(ch)) {
                            set(&ui, x);
                        }
                    }
                });
            }
        });
    }

    // Slow poll: fader order, mutes, apps, devices, rules. Wakes early after a change.
    let refresh = Arc::new(Notify::new());
    let notes: InstallNotes = Arc::default();
    {
        let (weak, proxy, goxlr, refresh, notes) = (
            ui.as_weak(),
            proxy.clone(),
            goxlr.clone(),
            Arc::clone(&refresh),
            Arc::clone(&notes),
        );
        tokio::spawn(async move {
            let mut prev: Option<Slow> = None;
            loop {
                let s = read_slow(proxy.as_ref(), &goxlr, &headset_sink, &notes).await;
                if prev.as_ref() != Some(&s) {
                    let (s2, p2) = (s.clone(), prev.clone());
                    let _ = weak.upgrade_in_event_loop(move |ui| apply_slow(&ui, &s2, p2.as_ref()));
                    prev = Some(s);
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = refresh.notified() => {}
                }
            }
        });
    }

    // Callbacks
    {
        let (touched, pending, send_now) = (
            Arc::clone(&touched),
            Arc::clone(&pending),
            Arc::clone(&send_now),
        );
        ui.on_channel_volume_changed(move |ch, v| {
            touched
                .lock()
                .unwrap()
                .insert(ch.to_string(), Instant::now());
            pending.lock().unwrap().insert(ch.to_string(), v as f64);
            send_now.notify_one();
        });
    }
    {
        let (proxy, goxlr, refresh) = (proxy.clone(), goxlr.clone(), Arc::clone(&refresh));
        ui.on_channel_mute(move |ch, muted| {
            let (proxy, goxlr, refresh, ch) = (
                proxy.clone(),
                goxlr.clone(),
                Arc::clone(&refresh),
                ch.to_string(),
            );
            tokio::spawn(async move {
                let done = match proxy {
                    Some(ref p) => p.set_channel_muted(ch.clone(), muted).await.is_ok(),
                    None => false,
                };
                if !done {
                    let _ = goxlr.set_channel_muted(&ch, muted).await;
                }
                refresh.notify_one();
            });
        });
    }
    {
        let (proxy, goxlr, weak) = (proxy.clone(), goxlr.clone(), ui.as_weak());
        ui.on_toggle_speakers(move || {
            let (proxy, goxlr, weak) = (proxy.clone(), goxlr.clone(), weak.clone());
            tokio::spawn(async move {
                let muted = match proxy {
                    Some(ref p) => p.toggle_speaker_mute().await.ok(),
                    None => None,
                };
                let muted = match muted {
                    Some(m) => m,
                    None => {
                        let curr = goxlr.get_headphones_volume().await.unwrap_or(255);
                        let target = if curr > 0 { 0 } else { 255 };
                        let _ = goxlr
                            .set_volume_raw(sonomux_device_goxlr::Channel::Headphones, target)
                            .await;
                        target == 0
                    }
                };
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_speaker_volume(if muted { 0.0 } else { 100.0 })
                });
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_set_routing_auto(move |on| {
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_routing_enabled(on).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_app_channel(move |id, idx| {
            let target = match idx {
                0 => String::new(),
                i => channel_choices()
                    .get(i as usize - 1)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            };
            let id = id.to_string();
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_app_override(id, target).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_category_channel(move |key, idx| {
            let target = channel_choices()
                .get(idx as usize)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "System".into());
            let key = key.to_string();
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_category_target(key, target).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_default_device(move |name, input| {
            let name = name.to_string();
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_default_device(name, input).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_set_keep_defaults(move |on| {
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_keep_defaults(on).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_set_split_unpaired(move |on| {
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_split_unpaired(on).await;
            });
        });
    }
    {
        let (proxy, refresh) = (proxy.clone(), Arc::clone(&refresh));
        ui.on_rule_enabled(move |name, on| {
            let name = name.to_string();
            with_proxy(&proxy, &refresh, move |p| async move {
                let _ = p.set_rule_enabled(name, on).await;
            });
        });
    }

    {
        let (notes, refresh) = (Arc::clone(&notes), Arc::clone(&refresh));
        ui.on_install_helper(move |index| {
            let i = index as usize;
            let Some(h) = hardware::HELPERS.get(i) else {
                return;
            };
            notes
                .lock()
                .unwrap()
                .insert(i, format!("Installing {}…", h.name));
            refresh.notify_one();
            let (notes, refresh) = (Arc::clone(&notes), Arc::clone(&refresh));
            tokio::task::spawn_blocking(move || {
                let mut n = notes.lock().unwrap();
                match hardware::install(h) {
                    // The system installer or terminal takes it from here; the row turns to
                    // "installed" once the helper appears.
                    Ok(()) => n.insert(
                        i,
                        format!("Finish installing {} in the window that opened", h.name),
                    ),
                    Err(e) => n.insert(i, format!("Couldn't install {}: {}", h.name, e)),
                };
                drop(n);
                refresh.notify_one();
            });
        });
    }

    info!("Starting Sonomux GUI...");
    ui.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_slots_follow_fader_order() {
        let faders: Vec<String> = ["Mic", "Chat", "Music", "System"].map(String::from).into();
        // Mic, System, Game, Chat, Music
        assert_eq!(strip_slots(&faders), [0, 3, 4, 1, 2]);
        // Nothing on faders: default order. Unknown channels are skipped.
        assert_eq!(strip_slots(&[]), [0, 1, 2, 3, 4]);
        let odd: Vec<String> = ["LineIn", "Game"].map(String::from).into();
        assert_eq!(strip_slots(&odd), [1, 2, 0, 3, 4]);
    }

    #[test]
    fn test_find_existing_hyprland_address_matches_title() {
        let json = r#"[
            {
                "address": "0x580f2432bce0",
                "class": "discord",
                "title": "Discord",
                "initialTitle": "Discord"
            },
            {
                "address": "0x580f2480a390",
                "class": "",
                "title": "Sonomux Audio Coordinator",
                "initialTitle": "Sonomux Audio Coordinator"
            }
        ]"#;

        let addr = find_existing_hyprland_address(json);
        assert_eq!(addr, Some("0x580f2480a390".to_string()));
    }

    #[test]
    fn test_find_existing_hyprland_address_matches_class() {
        let json = r#"[
            {
                "address": "0x12345678",
                "class": "sonomux",
                "title": "Audio Settings",
                "initialTitle": "Audio Settings"
            }
        ]"#;

        let addr = find_existing_hyprland_address(json);
        assert_eq!(addr, Some("0x12345678".to_string()));
    }

    #[test]
    fn test_find_existing_hyprland_address_none_when_empty_or_different() {
        let json = r#"[
            {
                "address": "0x580f2432bce0",
                "class": "discord",
                "title": "Discord"
            }
        ]"#;

        assert_eq!(find_existing_hyprland_address(json), None);
        assert_eq!(find_existing_hyprland_address("[]"), None);
        assert_eq!(find_existing_hyprland_address("invalid json"), None);
    }
}
