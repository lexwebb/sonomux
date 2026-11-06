use crate::BackendError;
use serde_json::Value;
use sonomux_core::{AppClassifier, DevicesConfig, PoliciesConfig, RoutingConfig, StreamInfo};
use std::collections::HashSet;
use tokio::process::Command;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Default)]
pub struct PulseManager;

impl PulseManager {
    pub fn new() -> Self {
        Self
    }

    /// Check if an audio sink exists in the system.
    pub async fn sink_exists(name: &str) -> bool {
        let output = Command::new("pactl")
            .args(["list", "sinks", "short"])
            .output()
            .await;

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Self::parse_exists(&stdout, name)
        } else {
            false
        }
    }

    /// Check if an audio source exists in the system.
    pub async fn source_exists(name: &str) -> bool {
        let output = Command::new("pactl")
            .args(["list", "sources", "short"])
            .output()
            .await;

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Self::parse_exists(&stdout, name)
        } else {
            false
        }
    }

    /// Pure parser: check if name appears as second column in short output.
    pub fn parse_exists(stdout: &str, name: &str) -> bool {
        stdout.lines().any(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.get(1).is_some_and(|&s| s == name)
        })
    }

    /// Find the PulseAudio module ID of an active module-loopback matching the given source.
    pub async fn find_loopback_module(source_name: &str) -> Option<u32> {
        let output = Command::new("pactl")
            .args(["list", "modules", "short"])
            .output()
            .await
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_loopback_module_id(&stdout, source_name)
    }

    /// Pure parser: Extract module-loopback ID from `pactl list modules short` output.
    pub fn parse_loopback_module_id(stdout: &str, source_name: &str) -> Option<u32> {
        let src_needle = format!("source={}", source_name);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && parts[1] == "module-loopback" && parts[2].contains(&src_needle) {
                if let Ok(id) = parts[0].parse::<u32>() {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Pure parser: Find source index from `pactl list sources short` output.
    pub fn parse_source_index(stdout: &str, expected_source: &str) -> Option<u32> {
        stdout.lines().find_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == expected_source {
                parts[0].parse::<u32>().ok()
            } else {
                None
            }
        })
    }

    /// Verify whether the loopback's capture stream is *actually* attached to the target source.
    /// This fixes the silent fallback bug where reconnecting GoXLR causes loopback to capture default mic.
    pub async fn is_loopback_healthy(module_id: u32, expected_source: &str) -> bool {
        // 1. Get index of the expected source
        let sources_out = Command::new("pactl")
            .args(["list", "sources", "short"])
            .output()
            .await;

        let target_idx = match sources_out {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                match Self::parse_source_index(&stdout, expected_source) {
                    Some(idx) => idx,
                    None => return false,
                }
            }
            Err(_) => return false,
        };

        // 2. Query source-outputs in JSON
        let outputs_out = Command::new("pactl")
            .args(["-f", "json", "list", "source-outputs"])
            .output()
            .await;

        if let Ok(out) = outputs_out {
            if let Ok(val) = serde_json::from_slice::<Value>(&out.stdout) {
                return Self::is_stream_on_source(&val, module_id, target_idx);
            }
        }

        false
    }

    /// Pure validator: verify that a source-output owned by `module_id` is recording from `target_idx`.
    pub fn is_stream_on_source(val: &Value, module_id: u32, target_idx: u32) -> bool {
        if let Some(list) = val.as_array() {
            for item in list {
                let owner = item.get("owner_module").and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                });
                let src = item.get("source").and_then(|v| v.as_u64());

                if owner == Some(module_id as u64) && src == Some(target_idx as u64) {
                    return true;
                }
            }
        }
        false
    }

    /// Load or verify low-latency loopback between source and sink.
    pub async fn ensure_loopback(
        source: &str,
        sink: &str,
        latency_ms: u32,
    ) -> Result<u32, BackendError> {
        let existing = Self::find_loopback_module(source).await;

        if let Some(mod_id) = existing {
            if Self::is_loopback_healthy(mod_id, source).await {
                debug!("Existing loopback module #{} is healthy.", mod_id);
                return Ok(mod_id);
            } else {
                warn!(
                    "Loopback module #{} is corrupt/detached from {}. Unloading...",
                    mod_id, source
                );
                let _ = Self::unload_module(mod_id).await;
            }
        }

        info!(
            "Loading module-loopback source={} sink={} latency_msec={}...",
            source, sink, latency_ms
        );
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-loopback",
                &format!("source={}", source),
                &format!("sink={}", sink),
                &format!("latency_msec={}", latency_ms),
            ])
            .output()
            .await
            .map_err(|e| BackendError::Connection(format!("Failed to load module: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Routing(format!(
                "Failed to load loopback: {}",
                err
            )));
        }

        let out_str = String::from_utf8_lossy(&output.stdout);
        let mod_id = out_str.trim().parse::<u32>().map_err(|_| {
            BackendError::Routing(format!("Invalid module id returned: {}", out_str))
        })?;

        info!("Loopback module #{} loaded successfully.", mod_id);
        Ok(mod_id)
    }

    /// Unload a PulseAudio / PipeWire module.
    pub async fn unload_module(module_id: u32) -> Result<(), BackendError> {
        debug!("Unloading module #{}", module_id);
        let output = Command::new("pactl")
            .args(["unload-module", &module_id.to_string()])
            .output()
            .await
            .map_err(|e| {
                BackendError::Connection(format!("Failed to execute pactl unload-module: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Routing(format!("Unload failed: {}", err)));
        }
        Ok(())
    }

    /// Pure logic: Calculate which streams need to be moved based on surround policies.
    pub fn compute_surround_moves(
        inputs_json: &Value,
        sinks_json: &Value,
        surround_out_node: &str,
        surround_in_node: &str,
        game_sink: &str,
        system_sink: &str,
        keep_off_apps: &[&str],
    ) -> Vec<(u64, String)> {
        let mut sink_id_to_name = std::collections::HashMap::new();
        if let Some(sinks) = sinks_json.as_array() {
            for s in sinks {
                if let (Some(idx), Some(name)) = (
                    s.get("index").and_then(|v| v.as_u64()),
                    s.get("name").and_then(|v| v.as_str()),
                ) {
                    sink_id_to_name.insert(idx, name.to_string());
                }
            }
        }

        let keep_off_set: HashSet<&str> = keep_off_apps.iter().copied().collect();
        let mut moves = Vec::new();

        if let Some(inputs) = inputs_json.as_array() {
            for inp in inputs {
                let stream_idx = match inp.get("index").and_then(|v| v.as_u64()) {
                    Some(i) => i,
                    None => continue,
                };
                let curr_sink_id = inp.get("sink").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
                let curr_sink_name = sink_id_to_name
                    .get(&curr_sink_id)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let props = inp.get("properties");
                let node_name = props
                    .and_then(|p| p.get("node.name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let app_name = props
                    .and_then(|p| p.get("application.name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // 1. Move VSM output to Game channel if misplaced
                if node_name == surround_out_node && curr_sink_name != game_sink {
                    moves.push((stream_idx, game_sink.to_string()));
                }
                // 2. Move isolated apps (e.g. Zen) back to System if VSM grabbed them
                else if keep_off_set.contains(app_name) && curr_sink_name == surround_in_node {
                    moves.push((stream_idx, system_sink.to_string()));
                }
            }
        }

        moves
    }

    /// Enforce Virtual Surround Manager routing policies:
    /// - `surround_out` is pinned to `game_sink`
    /// - Apps in `keep_off` are moved back to `system_sink` if surround grabbed them
    pub async fn enforce_surround_policies(
        surround_out_node: &str,
        surround_in_node: &str,
        game_sink: &str,
        system_sink: &str,
        keep_off_apps: &[&str],
    ) -> Result<(), BackendError> {
        let inputs_out = Command::new("pactl")
            .args(["-f", "json", "list", "sink-inputs"])
            .output()
            .await
            .map_err(|e| BackendError::Connection(e.to_string()))?;

        let sinks_out = Command::new("pactl")
            .args(["-f", "json", "list", "sinks"])
            .output()
            .await
            .map_err(|e| BackendError::Connection(e.to_string()))?;

        let sinks_json: Value = serde_json::from_slice(&sinks_out.stdout).unwrap_or(Value::Null);
        let inputs_json: Value = serde_json::from_slice(&inputs_out.stdout).unwrap_or(Value::Null);

        let moves = Self::compute_surround_moves(
            &inputs_json,
            &sinks_json,
            surround_out_node,
            surround_in_node,
            game_sink,
            system_sink,
            keep_off_apps,
        );

        for (stream_idx, target_sink) in moves {
            info!(
                "Enforcing policy: moving stream #{} to '{}'",
                stream_idx, target_sink
            );
            let _ = Command::new("pactl")
                .args(["move-sink-input", &stream_idx.to_string(), &target_sink])
                .status()
                .await;
        }

        Ok(())
    }

    /// Parse JSON output from `pactl -f json list sink-inputs` into structured `StreamInfo` items.
    pub fn parse_sink_inputs_json(
        inputs_json: &Value,
        sink_id_to_name: &std::collections::HashMap<u64, String>,
    ) -> Vec<StreamInfo> {
        let mut result = Vec::new();
        if let Some(inputs) = inputs_json.as_array() {
            for inp in inputs {
                let index = match inp.get("index").and_then(|v| v.as_u64()) {
                    Some(i) => i,
                    None => continue,
                };
                let sink_index = inp.get("sink").and_then(|v| v.as_u64()).unwrap_or(0);
                let sink_name = sink_id_to_name
                    .get(&sink_index)
                    .cloned()
                    .unwrap_or_default();
                let is_corked = inp.get("corked").and_then(|v| v.as_bool()).unwrap_or(false);
                let is_muted = inp.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);

                let props = inp.get("properties");
                let app_name = props
                    .and_then(|p| p.get("application.name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let binary = props
                    .and_then(|p| p.get("application.process.binary"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let media_role = props
                    .and_then(|p| p.get("media.role"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let media_name = props
                    .and_then(|p| p.get("media.name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let node_name = props
                    .and_then(|p| p.get("node.name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let pid = props
                    .and_then(|p| p.get("application.process.id"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u32>().ok());

                let mut cmdline = None;
                let mut exe_path = None;
                if let Some(p) = pid {
                    if let Ok(c) = std::fs::read_to_string(format!("/proc/{}/cmdline", p)) {
                        cmdline = Some(c.replace('\0', " "));
                    }
                    if let Ok(l) = std::fs::read_link(format!("/proc/{}/exe", p)) {
                        exe_path = Some(l.to_string_lossy().to_string());
                    }
                }

                result.push(StreamInfo {
                    index,
                    sink_index,
                    sink_name,
                    app_name,
                    binary,
                    media_role,
                    media_name,
                    node_name,
                    pid,
                    cmdline,
                    exe_path,
                    is_corked,
                    is_muted,
                });
            }
        }
        result
    }

    /// Calculate which sink-inputs must be moved according to the classification and routing rules.
    pub fn compute_routing_moves(
        streams: &[StreamInfo],
        routing_config: &RoutingConfig,
        devices: &DevicesConfig,
        policies: &PoliciesConfig,
        surround_available: bool,
    ) -> Vec<(u64, String, String)> {
        if !routing_config.enabled {
            return Vec::new();
        }

        let mut moves = Vec::new();
        let keep_off_set: HashSet<String> = policies
            .keep_off_surround
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        for stream in streams {
            let node_name = stream.node_name.as_deref().unwrap_or("");
            // Skip loopbacks, virtual surround output internal stream, and speech-dispatcher internal dummy
            if node_name == policies.surround_out_node
                || node_name.starts_with("output.loopback")
                || stream.app_name == "speech-dispatcher-dummy"
            {
                continue;
            }

            // Classify application using multi-signal heuristics
            let category = AppClassifier::classify(stream, &routing_config.app_overrides);

            // Determine target channel from configuration
            let cat_key = match category {
                sonomux_core::AppCategory::Game => "game",
                sonomux_core::AppCategory::Chat => "chat",
                sonomux_core::AppCategory::Music => "music",
                sonomux_core::AppCategory::Browser => "browser",
                sonomux_core::AppCategory::Production => "production",
                sonomux_core::AppCategory::Soundboard => "soundboard",
                sonomux_core::AppCategory::System => "system",
            };

            let target_channel = routing_config
                .category_targets
                .get(cat_key)
                .map(|s| s.as_str())
                .unwrap_or("System");

            // Resolve target channel to physical/DSP sink
            let target_sink = match target_channel {
                "Surround" => {
                    if surround_available {
                        &policies.surround_in_node
                    } else {
                        &devices.goxlr_game_sink
                    }
                }
                "Game" => &devices.goxlr_game_sink,
                "Chat" => &devices.goxlr_chat_sink,
                "Music" => &devices.goxlr_music_sink,
                "Sample" | "Soundboard" => &devices.goxlr_sample_sink,
                _ => &devices.goxlr_system_sink,
            };

            // Enforce keep_off_surround policy
            let is_in_keep_off = keep_off_set.contains(&stream.app_name.to_lowercase())
                || keep_off_set.contains(&stream.binary.to_lowercase());

            let effective_target = if is_in_keep_off && target_sink == &policies.surround_in_node {
                &devices.goxlr_system_sink
            } else {
                target_sink
            };

            if stream.sink_name != *effective_target {
                let reason = format!(
                    "{} (Cat: {}, Target: {})",
                    if stream.app_name.is_empty() {
                        &stream.binary
                    } else {
                        &stream.app_name
                    },
                    category,
                    target_channel
                );
                moves.push((stream.index, effective_target.to_string(), reason));
            }
        }

        moves
    }

    /// Fetch all active sink-inputs from PipeWire.
    pub async fn get_active_streams() -> Result<Vec<StreamInfo>, BackendError> {
        let inputs_out = Command::new("pactl")
            .args(["-f", "json", "list", "sink-inputs"])
            .output()
            .await
            .map_err(|e| BackendError::Connection(e.to_string()))?;

        let sinks_out = Command::new("pactl")
            .args(["-f", "json", "list", "sinks"])
            .output()
            .await
            .map_err(|e| BackendError::Connection(e.to_string()))?;

        let sinks_json: Value = serde_json::from_slice(&sinks_out.stdout).unwrap_or(Value::Null);
        let inputs_json: Value = serde_json::from_slice(&inputs_out.stdout).unwrap_or(Value::Null);

        let mut sink_id_to_name = std::collections::HashMap::new();
        if let Some(sinks) = sinks_json.as_array() {
            for s in sinks {
                if let (Some(idx), Some(name)) = (
                    s.get("index").and_then(|v| v.as_u64()),
                    s.get("name").and_then(|v| v.as_str()),
                ) {
                    sink_id_to_name.insert(idx, name.to_string());
                }
            }
        }

        Ok(Self::parse_sink_inputs_json(&inputs_json, &sink_id_to_name))
    }

    /// Apps for the GUI's routing list. On PipeWire, every sink-input is an app stream.
    pub async fn list_streams() -> Result<Vec<StreamInfo>, BackendError> {
        Self::get_active_streams().await
    }

    /// Move a specific stream (sink-input) to a destination sink.
    pub async fn move_stream(stream_idx: u64, target_sink: &str) -> Result<(), BackendError> {
        info!("Moving stream #{} to '{}'", stream_idx, target_sink);
        let output = Command::new("pactl")
            .args(["move-sink-input", &stream_idx.to_string(), target_sink])
            .output()
            .await
            .map_err(|e| BackendError::Connection(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Routing(format!(
                "move-sink-input failed: {}",
                err
            )));
        }
        Ok(())
    }

    /// Linux routing moves streams once; there is nothing live to release. Streams stay where
    /// they are, as before.
    pub async fn stop_stream_routing() -> usize {
        0
    }

    /// Enforce all stream classification & routing policies.
    pub async fn enforce_stream_routing(
        routing_config: &RoutingConfig,
        devices: &DevicesConfig,
        policies: &PoliciesConfig,
    ) -> Result<usize, BackendError> {
        let streams = Self::get_active_streams().await?;
        let surround_available = Self::sink_exists(&policies.surround_in_node).await;

        let moves = Self::compute_routing_moves(
            &streams,
            routing_config,
            devices,
            policies,
            surround_available,
        );

        let count = moves.len();
        for (idx, target_sink, reason) in moves {
            info!(
                "Enforcing routing: moving stream #{} -> '{}' ({})",
                idx, target_sink, reason
            );
            let _ = Self::move_stream(idx, &target_sink).await;
        }

        Ok(count)
    }

    /// Pure parser: follow sink-input lines to find which target sink index the virtual node feeds.
    pub fn parse_downstream_target_index(stdout: &str, virt_sink: &str) -> Option<String> {
        let mut current_target = String::new();

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Sink: ") {
                current_target = trimmed.trim_start_matches("Sink: ").to_string();
            } else if trimmed.starts_with("node.name = ") {
                let node_name = trimmed.trim_start_matches("node.name = ").trim_matches('"');
                if node_name.starts_with(virt_sink) && !current_target.is_empty() {
                    return Some(current_target);
                }
            }
        }
        None
    }

    /// Resolve a DSP sink down to its physical host sink (Omarchy volume law).
    pub async fn resolve_effective_sink(sink: &str) -> String {
        if sink.is_empty() || sink.starts_with("alsa_output.") {
            return sink.to_string();
        }

        let inputs_out = Command::new("pactl")
            .args(["list", "sink-inputs"])
            .output()
            .await;

        if let Ok(out) = inputs_out {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(target_idx) = Self::parse_downstream_target_index(&stdout, sink) {
                return Self::get_sink_name_by_index(&target_idx)
                    .await
                    .unwrap_or(sink.to_string());
            }
        }

        sink.to_string()
    }

    /// Pure parser: Find sink name from short output by index.
    pub fn parse_sink_name_by_index(stdout: &str, index: &str) -> Option<String> {
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == index {
                return Some(parts[1].to_string());
            }
        }
        None
    }

    pub async fn get_sink_name_by_index(index: &str) -> Option<String> {
        let output = Command::new("pactl")
            .args(["list", "sinks", "short"])
            .output()
            .await
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_sink_name_by_index(&stdout, index)
    }
}
