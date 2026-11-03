# Sonomux Architecture Specification

This document details the architectural layers, trait definitions, and data flow of **Sonomux**.

---

## 1. System Layers

```
┌─────────────────────────────────────────────────────────────┐
│                       Presentation Layer                    │
│   - Slint Native GUI (Linux / macOS Native Look & Feel)     │
│   - D-Bus Service (Status bar widgets, OSD triggers)        │
│   - CLI Tool (`sonomuxctl`)                                 │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────┐
│                    Sonomux Daemon Engine                    │
│   - Policy Engine (Evaluates conditions & state machines)   │
│   - Volume & DSP Coordinator (Downstream sink resolution)   │
│   - State Store (Persists volume levels & active profiles)  │
└──────────────┬───────────────────────────────┬──────────────┘
               │                               │
┌──────────────▼──────────────┐ ┌──────────────▼──────────────┐
│     Audio Server Backend    │ │    Hardware Device Plugins  │
│  - PipeWire / Pulse (Linux) │ │  - GoXLR (IPC / WebSocket)  │
│  - CoreAudio (macOS)        │ │  - HID Battery & Peripheral │
│  - Stream routing & loops   │ │  - Extensible Community API │
└─────────────────────────────┘ └─────────────────────────────┘
```

---

## 2. Core Abstractions (`audio-core`)

The core library defines abstractions without binding to a specific operating system or hardware protocol.

### 2.1 Device State & Lifecycle

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    Disconnected,
    /// The physical interface/dongle is connected, but the peripheral is asleep or turned off.
    Standby,
    /// The device is powered on, responding, and available for audio.
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub capabilities: Vec<String>,
}

#[async_trait]
pub trait AudioDevice: Send + Sync {
    fn metadata(&self) -> &DeviceMetadata;
    async fn status(&self) -> DeviceStatus;
    async fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<DeviceEvent>;
}
```

### 2.2 Capability Traits

Hardware varies widely in feature sets. Sonomux uses granular capability traits:

#### `HasPresenceDetection`
Solves the "ghost dongle" issue by querying low-level battery or telemetry endpoints.
```rust
#[async_trait]
pub trait HasPresenceDetection: AudioDevice {
    async fn is_physically_active(&self) -> bool;
    async fn battery_level(&self) -> Option<u8>;
    async fn is_charging(&self) -> Option<bool>;
}
```

#### `HasRoutingMatrix`
For multi-channel devices featuring internal cross-point mixers.
```rust
#[async_trait]
pub trait HasRoutingMatrix: AudioDevice {
    async fn available_inputs(&self) -> Vec<String>;
    async fn available_outputs(&self) -> Vec<String>;
    async fn is_routed(&self, input: &str, output: &str) -> Result<bool, DeviceError>;
    async fn set_route(&mut self, input: &str, output: &str, enabled: bool) -> Result<(), DeviceError>;
}
```

#### `HasHardwareVolume`
For devices managing internal DAC levels or separate physical outputs (like headphone vs line out).
```rust
#[async_trait]
pub trait HasHardwareVolume: AudioDevice {
    async fn get_channel_volume(&self, channel: &str) -> Result<f32, DeviceError>;
    async fn set_channel_volume(&mut self, channel: &str, level: f32) -> Result<(), DeviceError>;
    async fn set_channel_mute(&mut self, channel: &str, muted: bool) -> Result<(), DeviceError>;
}
```

---

## 3. Audio Graph Backend Abstraction

The audio backend connects Sonomux's policy engine to the host OS sound server.

```rust
#[async_trait]
pub trait AudioBackend: Send + Sync {
    /// Watch for sink/source lifecycle events.
    async fn watch_graph(&self) -> Result<Receiver<GraphEvent>, BackendError>;
    
    /// Resolve a virtual/DSP sink down to its physical host sink.
    async fn resolve_effective_sink(&self, sink_name: &str) -> Result<String, BackendError>;
    
    /// Pin an application stream (by process or app name) to a specific sink.
    async fn route_stream(&self, app_name: &str, target_sink: &str) -> Result<(), BackendError>;
    
    /// Ensure a low-latency loopback exists between a source and sink, re-linking if severed.
    async fn ensure_loopback(&self, source_name: &str, sink_name: &str, latency_ms: u32) -> Result<LoopbackId, BackendError>;
    async fn remove_loopback(&self, id: LoopbackId) -> Result<(), BackendError>;
}
```

### 3.1 Linux Implementation: PipeWire & Libpulse
- Monitors PipeWire graph changes using libpipewire / libpulse event loops.
- Replaces periodic polling with event-driven reactions: when a stream is announced, its metadata is compared against user policy rules immediately.
- Implements the Omarchy volume law: follows `sink-input` streams from virtual nodes (Virtual Surround, EasyEffects) down to the underlying ALSA hardware node.

### 3.2 macOS Implementation: CoreAudio (Future)
- Uses CoreAudio HAL and Aggregate Audio Device APIs.
- Creates virtual multi-channel routing harnesses without requiring external proprietary software.

---

## 4. Policy Engine & State Synchronization

The **Policy Engine** continuously synchronizes peripheral state with audio graph routing:

```
  [ Headset Turns ON ]
          │
          ▼ (Debounce check: 2 consecutive positive reads)
  [ Policy Trigger: Headset Active ]
          ├─► GoXLR Driver: Route Sampler Mix -> Loopback
          ├─► Audio Backend: Create & Verify Loopback (GoXLR Sampler -> Arctis Nova 7)
          ├─► GoXLR Driver: Save Desk Speaker Volume & Mute GoXLR Headphone Jack
          └─► Audio Backend: Verify VSM output on Game channel; Zen browser on System channel
```

```
  [ Headset Turns OFF ]
          │
          ▼
  [ Policy Trigger: Headset Standby ]
          ├─► Audio Backend: Remove Loopback
          ├─► GoXLR Driver: Restore Desk Speaker Volume on GoXLR Headphone Jack
          └─► Notify Desktop (D-Bus / OSD)
```
