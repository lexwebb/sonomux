# AGENTS.md — Sonomux Audio Coordinator

> `CLAUDE.md` is maintained as a symlink to this file for tools that expect it.

**Sonomux** (*Sono* + *Multiplexer*) is a unified audio coordinator, stream routing engine, and peripheral manager written in Rust. It coordinates multi-channel hardware mixers (TC-Helicon GoXLR Mini), smart wireless headsets (SteelSeries Arctis Nova 7), and Linux audio servers (PipeWire / WirePlumber / PulseAudio) via a declarative rule engine, session D-Bus interface, Slint native GUI, and Omarchy status bar integration.

---

## Workspace & Crate Architecture

The repository is structured as a Cargo workspace with distinct separation of concerns:

```
crates/
├── sonomux-core/            # Capability traits (AudioDevice, HasPresenceDetection, etc.),
│                            # configuration schemas (config.rs), and declarative rule engine (rules.rs).
│                            # Pure Rust domain logic; no direct hardware or I/O calls.
├── sonomux-backend/         # Audio graph backends behind `sonomux_backend::Backend`:
│                            # Linux: PipeWire / PulseAudio (PulseManager): loopbacks, stream pinning, volume traversal.
│                            # macOS: CoreAudio (CoreAudioManager): devices, playing apps, per-channel GoXLR devices
│                            # (coreaudio_channels), per-app routing via process taps (coreaudio_tap).
├── sonomux-device-headset/  # Headset presence detection & battery telemetry via HID / headsetcontrol.
│                            # State debouncing and event streaming (HeadsetMonitor).
├── sonomux-device-goxlr/    # GoXLR Mini hardware client (GoXlrClient) via Unix domain socket IPC (/tmp/goxlr.socket).
├── sonomux-daemon/          # Background service (sonomuxd).
│                            # Orchestrates device loops, executes declarative rules (ActionExecutor),
│                            # and hosts the session D-Bus interface (org.sonomux.Daemon via zbus).
├── sonomux-gui/             # Native Slint GUI (sonomux). Real-time fader control, battery indicators,
│                            # mute toggles, and live D-Bus connection badge.
└── sonomux-cli/             # CLI controller (sonomuxctl). Interacts with sonomuxd via D-Bus IPC.

dist/                        # Deployment, packaging, and integration assets:
├── systemd/                 # User systemd service (sonomuxd.service)
├── launchd/                 # macOS LaunchAgent (org.sonomux.sonomuxd.plist; install.sh fills paths)
├── dbus/                    # Session D-Bus auto-activation service (org.sonomux.Daemon.service)
├── desktop/                 # XDG application desktop entry (org.sonomux.Sonomux.desktop)
├── omarchy/                 # Omarchy status bar polling script (sonomux-bar)
└── arch/                    # Arch Linux PKGBUILD

scripts/
└── coverage.sh              # Local LLVM coverage runner and README badge updater.
```

---

## Verification & Standard Commands

Always validate changes with these commands from the repository root:

| Step | Command | Purpose / Requirement |
|------|---------|-----------------------|
| 1. Format Check | `cargo fmt --check` | Enforces standard Rust formatting. |
| 2. Linting | `cargo clippy --workspace --all-targets -- -D warnings` | Zero clippy warnings allowed across workspace. |
| 3. Unit & Integration Tests | `cargo test --workspace` | Fast workspace test suite (all tests must pass). |
| 4. Test Coverage | `./scripts/coverage.sh` | Enforces the **$\ge 85\%$ test coverage soft boundary** and updates README badge. |
| 5. Hands-Free Install | `./install.sh` | Builds release workspace and installs to `~/.local/bin` and systemd user services. |

---

## Critical Rules & Guidelines for AI Agents

### 1. Code Coverage Soft Boundary ($\ge 85\%$)
- Maintain a minimum of **85.0% line coverage** across the workspace.
- When introducing new features, state transitions, or rule actions, implement comprehensive unit and integration tests covering both happy and error paths.
- Run `./scripts/coverage.sh` prior to concluding tasks to confirm that workspace coverage stays at or above 85% and that the `README.md` badge remains synchronized.

### 2. Process & Binary Safety
- On macOS the daemon is a launchd agent (`launchctl print gui/$UID/org.sonomux.sonomuxd`), logging to `~/Library/Logs/sonomuxd.log`. `./install.sh` stops it before copying binaries.
- The `sonomuxd` daemon runs as an active systemd user service (`systemctl --user status sonomuxd.service`).
- **Never attempt to overwrite or compile binaries directly into `~/.local/bin/` while `sonomuxd` is running**; it will fail with `ETXTBUSY` ("Text file busy").
- Use `./install.sh` (which handles stopping, copying, reloading, and restarting cleanly) or run:
  ```bash
  systemctl --user stop sonomuxd.service
  cp target/release/sonomux* ~/.local/bin/
  systemctl --user start sonomuxd.service
  ```

### 3. Tokio & D-Bus Threading (zbus)
- macOS has no session bus. `sonomuxd` serves the same zbus interface peer-to-peer on a Unix socket (`~/.local/state/sonomux/sonomuxd.sock`). Clients must connect through `sonomux_core::ipc::connect()`, never `zbus::Connection::session()` directly.
- `zbus::connection::Builder` spawns internal executor threads that are outside Tokio's ambient runtime context.
- Calling async functions requiring a Tokio reactor (such as Tokio's `UnixStream::connect`) directly from a zbus method handler will panic with:
  `there is no reactor running, must be called from the context of a Tokio 1.x runtime`.
- **Solution**: Pass `tokio::runtime::Handle` into `DbusServer` and dispatch async operations using:
  ```rust
  let handle = self.tokio_handle.clone();
  handle.spawn(async move { ... }).await
  ```
  Do **not** hold an `EnterGuard` across an `.await` boundary, as it is `!Send`.

### 4. Omarchy Desktop & Status Bar Integration
- Never modify files in `/usr/share/omarchy/`. That directory is managed by the system package manager.
- Status bar configuration lives in `~/.config/omarchy/shell.json`.
- The `sonomux` bar widget supports:
  - `onClick`: `"sonomux"` (opens native Slint GUI)
  - `onRightClick`: `"sonomuxctl mute"` (toggles desk speaker mute)
- To reload the Omarchy bar after modifying `shell.json`, run `omarchy restart shell`.

### 5. Physical Hardware vs Mock Testing
- The primary developer workstation has physical hardware connected:
  - TC-Helicon GoXLR Mini on USB with active daemon IPC socket at `/tmp/goxlr.socket`.
  - SteelSeries Arctis Nova 7 USB 2.4GHz wireless dongle.
- Unit and mock tests (`mock_goxlr_test.rs`, `telemetry_tests.rs`) isolate I/O and do not depend on hardware.
- Live integration tests (`live_goxlr_test.rs`, `probe_test.rs`) detect if physical devices are available and perform safe queries without altering production routing states.
