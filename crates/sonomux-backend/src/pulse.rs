use crate::BackendError;
use serde_json::Value;
use std::collections::HashSet;
use tokio::process::Command;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
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
            stdout.lines().any(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                parts.get(1).map_or(false, |&s| s == name)
            })
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
            stdout.lines().any(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                parts.get(1).map_or(false, |&s| s == name)
            })
        } else {
            false
        }
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

    /// Verify whether the loopback's capture stream is *actually* attached to the target source.
    /// This fixes the silent fallback bug where reconnecting GoXLR causes loopback to capture default mic.
    pub async fn is_loopback_healthy(module_id: u32, expected_source: &str) -> bool {
        // 1. Get index of the expected source
        let sources_out = Command::new("pactl")
            .args(["list", "sources", "short"])
            .output()
            .await;

        let expected_index = if let Ok(out) = sources_out {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().find_map(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == expected_source {
                    parts[0].parse::<u32>().ok()
                } else {
                    None
                }
            })
        } else {
            None
        };

        let target_idx = match expected_index {
            Some(idx) => idx,
            None => return false,
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
                    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
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
                warn!("Loopback module #{} is corrupt/detached from {}. Unloading...", mod_id, source);
                let _ = Self::unload_module(mod_id).await;
            }
        }

        info!("Loading module-loopback source={} sink={} latency_msec={}...", source, sink, latency_ms);
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
            return Err(BackendError::Routing(format!("Failed to load loopback: {}", err)));
        }

        let out_str = String::from_utf8_lossy(&output.stdout);
        let mod_id = out_str.trim().parse::<u32>()
            .map_err(|_| BackendError::Routing(format!("Invalid module id returned: {}", out_str)))?;

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
            .map_err(|e| BackendError::Connection(format!("Failed to execute pactl unload-module: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Routing(format!("Unload failed: {}", err)));
        }
        Ok(())
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

        let mut sink_id_to_name = std::collections::HashMap::new();
        if let Some(sinks) = sinks_json.as_array() {
            for s in sinks {
                if let (Some(idx), Some(name)) = (s.get("index").and_then(|v| v.as_u64()), s.get("name").and_then(|v| v.as_str())) {
                    sink_id_to_name.insert(idx, name.to_string());
                }
            }
        }

        let keep_off_set: HashSet<&str> = keep_off_apps.iter().copied().collect();

        if let Some(inputs) = inputs_json.as_array() {
            for inp in inputs {
                let stream_idx = match inp.get("index").and_then(|v| v.as_u64()) {
                    Some(i) => i,
                    None => continue,
                };
                let curr_sink_id = inp.get("sink").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
                let curr_sink_name = sink_id_to_name.get(&curr_sink_id).map(|s| s.as_str()).unwrap_or("");
                let props = inp.get("properties");
                let node_name = props.and_then(|p| p.get("node.name")).and_then(|v| v.as_str()).unwrap_or("");
                let app_name = props.and_then(|p| p.get("application.name")).and_then(|v| v.as_str()).unwrap_or("");

                // 1. Move VSM output to Game channel if misplaced
                if node_name == surround_out_node && curr_sink_name != game_sink {
                    info!("Moving VSM stream #{} from '{}' to '{}'", stream_idx, curr_sink_name, game_sink);
                    let _ = Command::new("pactl")
                        .args(["move-sink-input", &stream_idx.to_string(), game_sink])
                        .status()
                        .await;
                }
                // 2. Move isolated apps (e.g. Zen) back to System if VSM grabbed them
                else if keep_off_set.contains(app_name) && curr_sink_name == surround_in_node {
                    info!("Moving app '{}' stream #{} back to '{}'", app_name, stream_idx, system_sink);
                    let _ = Command::new("pactl")
                        .args(["move-sink-input", &stream_idx.to_string(), system_sink])
                        .status()
                        .await;
                }
            }
        }

        Ok(())
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
            let mut current_target = String::new();

            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Sink: ") {
                    current_target = trimmed.trim_start_matches("Sink: ").to_string();
                } else if trimmed.starts_with("node.name = ") {
                    let node_name = trimmed.trim_start_matches("node.name = ").trim_matches('"');
                    if node_name.starts_with(sink) && !current_target.is_empty() {
                        return Self::get_sink_name_by_index(&current_target).await.unwrap_or(sink.to_string());
                    }
                }
            }
        }

        sink.to_string()
    }

    async fn get_sink_name_by_index(index: &str) -> Option<String> {
        let output = Command::new("pactl")
            .args(["list", "sinks", "short"])
            .output()
            .await
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == index {
                return Some(parts[1].to_string());
            }
        }
        None
    }
}
