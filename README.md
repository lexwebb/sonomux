# Sonomux 🎛️

**Sonomux** (*Sono* + *Multiplexer*) is a unified, open-source audio coordinator, stream routing engine, and peripheral manager built with Rust.

Designed for modern Linux desktops (PipeWire / WirePlumber) and architected for cross-platform portability (macOS CoreAudio), Sonomux bridges the gap between operating system audio graphs, complex multi-channel hardware mixers (like the TC-Helicon GoXLR), and smart wireless peripherals.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Built with Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![GUI: Slint](https://img.shields.io/badge/GUI-Slint-green.svg)](https://slint.dev/)
[![Coverage: 90%](https://img.shields.io/badge/Coverage-90%25-brightgreen.svg)](scripts/coverage.sh)

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
- `HasFaders`: For devices with motorized or physical faders and mute buttons.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for full trait specifications and [docs/DEVICE_CONTRIBUTING.md](docs/DEVICE_CONTRIBUTING.md) for how to add support for new devices.

---

## Code-less / Declarative Configuration (Stretch Goal)

Sonomux is designed to support declarative configuration files (`config.toml`), allowing users and contributors to configure complex switching behaviors and device mappings without writing code:

```toml
[devices.goxlr]
type = "goxlr"
connection = "ipc"

[devices.headset]
type = "hid_battery"
vendor_id = 0x1038
product_id = 0x12ad

[[rules]]
name = "Auto Headset Switcher"
trigger.device = "headset"
trigger.condition = "status == 'Active'"
debounce_ms = 1500

[[rules.actions_on_enter]]
type = "loopback"
source = "goxlr:Line5"
sink = "headset:Sink"

[[rules.actions_on_enter]]
type = "hardware_volume"
device = "goxlr"
channel = "Headphones"
set_mute = true

[[rules.actions_on_exit]]
type = "hardware_volume"
device = "goxlr"
channel = "Headphones"
set_mute = false
restore_previous = true
```

---

## Roadmap

- [ ] **Phase 1: Core Trait Architecture & Linux Engine**
  - Define `audio-core` capability traits.
  - Implement `device-headset` (HID / HeadsetControl debounced presence detection).
  - Implement `device-goxlr` (GoXLR IPC bridge, router matrix setup, speaker mute/restore).
  - Implement `audio-backend-pipewire` (robust loopback management, stream pinning).
- [ ] **Phase 2: Slint Native GUI & Omarchy Integration**
  - Native Slint dashboard showing device status, battery level, active profiles, and manual override switches.
  - D-Bus interface for desktop status bars (Omarchy shell widget, Waybar, etc.).
- [ ] **Phase 3: Declarative Rule Engine**
  - TOML-based declarative configuration for zero-code custom device setup.
- [ ] **Phase 4: macOS CoreAudio Support**
  - Implement `audio-backend-coreaudio` aggregate device and routing layer.
  - Additional community device drivers (Elgato Wave XLR, Rodecaster Pro, Focusrite).

---

## Contributing

We welcome contributions from the community! Whether you want to add support for a new audio interface, fix a bug, or improve documentation, please read:

- [**CONTRIBUTING.md**](CONTRIBUTING.md): Code standards, review workflow, and our explicit **AI-assisted coding and human accountability policy**.
- [**docs/DEVICE_CONTRIBUTING.md**](docs/DEVICE_CONTRIBUTING.md): Architectural guide to implementing device capability traits (`AudioDevice`, `HasPresenceDetection`, `HasRoutingMatrix`, etc.).

---

## License

Sonomux is licensed under the **GNU General Public License v3.0 (GPL-3.0)**. See the [LICENSE](LICENSE) file for details. Copyleft ensures that this software and any future contributions remain open source and community-owned.
