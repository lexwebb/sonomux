# Sonomux 🎛️

**Sonomux** (*Sono* + *Multiplexer*) is a unified, open-source audio coordinator, stream routing engine, and peripheral manager built with Rust.

Designed for modern Linux desktops (PipeWire / WirePlumber) and architected for cross-platform portability (macOS CoreAudio), Sonomux bridges the gap between operating system audio graphs, complex multi-channel hardware mixers (like the TC-Helicon GoXLR), and smart wireless peripherals.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Built with Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![GUI: Slint](https://img.shields.io/badge/GUI-Slint-green.svg)](https://slint.dev/)
[![Coverage: 63%](https://img.shields.io/badge/Coverage-63%25-brightgreen.svg)](scripts/coverage.sh)

> [!NOTE]
> **Project Transparency & AI Collaboration**: Sonomux is developed using human-AI pair programming. Every architectural decision, code change, and hardware integration is reviewed, tested on physical equipment, and maintained by human engineers. We welcome both human and AI-assisted community contributions, provided contributors take 100% personal responsibility for reviewing, understanding, and testing their code. See our [Contributing Guidelines](CONTRIBUTING.md) for full details.

---

## The Problems Sonomux Solves

Desktop audio on modern systems—especially with multi-channel mixers, virtual DSP chains, and wireless peripherals—suffers from real-world coordination problems:

1. **The "Ghost Dongle" Problem**:
   Wireless gaming headsets (e.g. SteelSeries Arctis Nova, Logitech Lightspeed) keep their 2.4GHz USB dongle connected 24/7. Linux and ALSA treat the audio sink as active and connected even when the headset is turned off on your desk. Sonomux monitors true physical state (via USB HID and battery telemetry) to auto-switch routing only when the headset is genuinely powered on.

2. **Downstream DSP Volume Traversal**:
   When using virtual sinks (Virtual Surround Manager, EasyEffects, or system speaker tuning), adjusting volume on the virtual sink either causes clipping, distorts limiter/compressor curves, or contradicts volume keys. Sonomux walks the audio graph downwards, directing volume commands to the physical hardware sink while keeping DSP processing nodes at unity gain.

3. **Loopback Fallback Resilience**:
   In setups where a sub-mix (e.g. GoXLR Sampler mix minus microphone) is fed to a wireless headset via loopback, device suspend/resume or USB re-enumeration often causes the loopback stream to silently fall back to the default microphone. Sonomux actively monitors audio graph topology and deterministically re-links the correct source.

4. **Stream Isolation & Policy Enforcement**:
   Virtual surround managers and spatializers often capture all unassigned application streams. Sonomux enforces deterministic stream pinning (e.g., ensuring browser audio stays on the System channel while game audio passes through spatial processing to the Game channel).

5. **Hardware vs. Software Volume Harmonization**:
   Hardware mixers maintain internal DSP levels (e.g. GoXLR's 0–255 volume scale and hardware headphone jacks) alongside OS software volumes. Sonomux unifies these domains so actions like "mute desk speakers when headset turns on" and "restore previous loudness when headset turns off" happen automatically and safely.

---

## Pluggable Architecture

Sonomux is designed from the ground up as a **modular community platform**. Hardware support is decoupled from audio server backends through a capability-based trait system:

```
                    ┌───────────────────────────────────┐
                    │            Sonomux GUI            │
                    │   (Native Slint UI: Linux/macOS)  │
                    └─────────────────┬─────────────────┘
                                      │ IPC / D-Bus
                    ┌─────────────────▼─────────────────┐
                    │          Sonomux Daemon           │
                    │   (Policy Engine & Rule Matcher)  │
                    └────────┬─────────────────┬────────┘
                             │                 │
             ┌───────────────▼────────┐   ┌────▼────────────────────────┐
             │  Audio Graph Backend   │   │     Hardware Drivers        │
             │  - Linux: PipeWire     │   │  (Extensible Rust Traits)   │
             │  - macOS: CoreAudio    │   │  - GoXLR / GoXLR Mini       │
             └────────────────────────┘   │  - HID Headset Watcher      │
                                          │  - Future: Elgato, Rode...  │
                                          └─────────────────────────────┘
```

### Capability Traits

Devices implement modular capability traits rather than monolithic drivers:

- `AudioDevice`: Core device lifecycle, state querying, and event streams.
- `HasPresenceDetection`: For wireless peripherals requiring HID / battery probing to determine true physical presence.
- `HasRoutingMatrix`: For devices with internal hardware routing (GoXLR, Rodecaster, BEACN).
- `HasHardwareVolume`: For devices with discrete hardware DAC / headphone volume registers.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for full trait specifications and [docs/DEVICE_CONTRIBUTING.md](docs/DEVICE_CONTRIBUTING.md) for how to add support for new devices.

---

## Declarative Rule Engine

Sonomux features a built-in declarative rule engine configured via TOML (`config.toml`), allowing users to automate routing changes, speaker mutes, volume levels, and loopbacks based on hardware state transitions without writing code:

This mirrors the built-in default rule; the full schema lives in `crates/sonomux-core/src/rules.rs`.

```toml
[daemon]
check_interval_ms = 1500

[devices]
headset_sink = "alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo"
goxlr_mix_source = "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source"

[[rules]]
name = "Auto Headset Switcher"
enabled = true

[rules.trigger]
type = "device_status"
device_id = "headset"
status = "Active"

[[rules.actions_on_enter]]
type = "save_hardware_volume"
device_id = "goxlr"
channel = "Headphones"
slot = "speaker_volume"

[[rules.actions_on_enter]]
type = "set_hardware_volume"
device_id = "goxlr"
channel = "Headphones"
level = 0.0

[[rules.actions_on_enter]]
type = "ensure_loopback"
source = "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source"
sink = "alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo"
latency_ms = 20

[[rules.actions_on_exit]]
type = "destroy_loopback"
source = "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source"

[[rules.actions_on_exit]]
type = "restore_hardware_volume"
device_id = "goxlr"
channel = "Headphones"
slot = "speaker_volume"
default_level = 1.0
```

---

## Roadmap

- [x] **Phase 1: Core Trait Architecture & Linux Engine**
  - [x] Capability trait system (`AudioDevice`, `HasPresenceDetection`, `HasHardwareVolume`, `HasRoutingMatrix`).
  - [x] SteelSeries Arctis Nova 7 wireless watcher with debounced HID/battery presence telemetry.
  - [x] TC-Helicon GoXLR Mini client via Unix domain socket IPC (`/tmp/goxlr.socket`).
  - [x] PipeWire / PulseAudio graph manager with automated loopback fallback recovery and stream pinning.
- [x] **Phase 2: Slint Native GUI & Omarchy Integration**
  - [x] Native Slint GUI dashboard (`sonomux-gui`) with real-time volume faders, mute controls, battery indicators, and live daemon status badge.
  - [x] Session D-Bus service (`org.sonomux.Daemon`) and CLI controller (`sonomuxctl`).
  - [x] Omarchy status bar widget with dynamic icons, tooltip telemetry, left-click GUI launcher, and right-click mute toggle.
  - [x] Hands-free installer (`install.sh`), systemd user service (`sonomuxd.service`), XDG desktop launcher, and Arch packaging (`PKGBUILD`).
- [x] **Phase 3: Declarative Rule Engine**
  - [x] TOML-based declarative configuration schema (`config.toml`).
  - [x] Composable triggers: device presence, battery level thresholds, audio node presence, and boolean logic.
  - [x] Action executor: loopback creation/destruction, volume ramping, mute toggles, and state restoration on exit.
  - [x] Test suite with a coverage ratchet (the gate only ever goes up) enforced in CI, plus coverage badges.
- [ ] **Phase 4: macOS CoreAudio Support & Hardware Ecosystem**
  - [x] CoreAudio backend: per-channel split devices, per-app routing via process taps, default-device management, and a menu bar extra.
  - [ ] Additional community device drivers (Elgato Wave XLR, Rodecaster Pro, BEACN, Focusrite).
  - [ ] Visual audio graph routing canvas within Slint GUI.

---

## Contributing

We welcome contributions from the community! Whether you want to add support for a new audio interface, fix a bug, or improve documentation, please read:

- [**CONTRIBUTING.md**](CONTRIBUTING.md): Code standards, review workflow, and our explicit **AI-assisted coding and human accountability policy**.
- [**docs/DEVICE_CONTRIBUTING.md**](docs/DEVICE_CONTRIBUTING.md): Architectural guide to implementing device capability traits (`AudioDevice`, `HasPresenceDetection`, `HasRoutingMatrix`, etc.).

---

## License

Sonomux is licensed under the **GNU General Public License v3.0 (GPL-3.0)**. See the [LICENSE](LICENSE) file for details. Copyleft ensures that this software and any future contributions remain open source and community-owned.
