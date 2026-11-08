//! Native 7.1 Virtual Surround spatial audio engine for PipeWire.
//!
//! Replaces external tools like Virtual Surround Manager by embedding PipeWire's native
//! `libpipewire-module-filter-chain` spatial convolution directly into Sonomux.
//!
//! Uses 14-channel HeSuVi HRIR impulse responses (Atmos, DTS:X, SBX, etc.) to convolve 8-channel
//! 7.1 surround sound down into binaural 2-channel stereo, hardwired directly to the game channel.

use crate::BackendError;
use sonomux_core::config::SurroundConfig;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Standard directory locations searched for HeSuVi HRIR impulse response WAV files.
pub fn hrir_search_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data_home).join("sonomux").join("hrir"));
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("sonomux")
                .join("hrir"),
        );
    }

    dirs.push(PathBuf::from("/usr/share/sonomux/hrir"));
    dirs.push(PathBuf::from("assets/hrir"));
    dirs.push(PathBuf::from(
        "/usr/share/virtual-surround-manager/hrir_wav",
    ));

    dirs
}

/// Discover all available HRIR profile names across standard directories.
pub fn list_available_profiles() -> Vec<String> {
    let mut profiles = Vec::new();

    for dir in hrir_search_directories() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "wav") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let stem_str = stem.to_string();
                        if !profiles.contains(&stem_str) {
                            profiles.push(stem_str);
                        }
                    }
                }
            }
        }
    }

    profiles.sort();
    profiles
}

/// Resolve the absolute file path to the HRIR impulse response WAV file.
pub fn resolve_hrir_path(profile: &str, custom_wav: Option<&str>) -> Option<PathBuf> {
    if let Some(custom) = custom_wav {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }

    for dir in hrir_search_directories() {
        let wav_path = dir.join(format!("{}.wav", profile));
        if wav_path.is_file() {
            return Some(wav_path);
        }
    }

    // Fallback: look for atmos.wav in search dirs
    for dir in hrir_search_directories() {
        let fallback = dir.join("atmos.wav");
        if fallback.is_file() {
            return Some(fallback);
        }
    }

    None
}

/// Generate a complete standalone PipeWire filter-chain configuration string.
pub fn generate_filter_chain_config(
    hrir_wav_path: &Path,
    sink_name: &str,
    sink_description: &str,
    target_sink: &str,
) -> String {
    let hrir_str = hrir_wav_path.to_string_lossy();
    format!(
        r#"context.properties = {{ log.level = 0 }}
context.spa-libs = {{
    audio.convert.* = audioconvert/libspa-audioconvert
    support.*       = support/libspa-support
}}
context.modules = [
    {{ name = libpipewire-module-rt flags = [ ifexists nofail ] }}
    {{ name = libpipewire-module-protocol-native }}
    {{ name = libpipewire-module-client-node }}
    {{ name = libpipewire-module-adapter }}
    {{ name = libpipewire-module-filter-chain
        args = {{
            node.description = "{sink_description}"
            media.name       = "{sink_description}"
            filter.graph = {{
                nodes = [
                    {{ type = builtin label = copy name = copyFL  }}
                    {{ type = builtin label = copy name = copyFR  }}
                    {{ type = builtin label = copy name = copyFC  }}
                    {{ type = builtin label = copy name = copyRL  }}
                    {{ type = builtin label = copy name = copyRR  }}
                    {{ type = builtin label = copy name = copySL  }}
                    {{ type = builtin label = copy name = copySR  }}
                    {{ type = builtin label = copy name = copyLFE }}
                    {{ type = builtin label = convolver name = convFL_L config = {{ filename = "{hrir_str}" channel =  0 }} }}
                    {{ type = builtin label = convolver name = convFL_R config = {{ filename = "{hrir_str}" channel =  1 }} }}
                    {{ type = builtin label = convolver name = convSL_L config = {{ filename = "{hrir_str}" channel =  2 }} }}
                    {{ type = builtin label = convolver name = convSL_R config = {{ filename = "{hrir_str}" channel =  3 }} }}
                    {{ type = builtin label = convolver name = convRL_L config = {{ filename = "{hrir_str}" channel =  4 }} }}
                    {{ type = builtin label = convolver name = convRL_R config = {{ filename = "{hrir_str}" channel =  5 }} }}
                    {{ type = builtin label = convolver name = convFC_L config = {{ filename = "{hrir_str}" channel =  6 }} }}
                    {{ type = builtin label = convolver name = convFR_R config = {{ filename = "{hrir_str}" channel =  7 }} }}
                    {{ type = builtin label = convolver name = convFR_L config = {{ filename = "{hrir_str}" channel =  8 }} }}
                    {{ type = builtin label = convolver name = convSR_R config = {{ filename = "{hrir_str}" channel =  9 }} }}
                    {{ type = builtin label = convolver name = convSR_L config = {{ filename = "{hrir_str}" channel = 10 }} }}
                    {{ type = builtin label = convolver name = convRR_R config = {{ filename = "{hrir_str}" channel = 11 }} }}
                    {{ type = builtin label = convolver name = convRR_L config = {{ filename = "{hrir_str}" channel = 12 }} }}
                    {{ type = builtin label = convolver name = convFC_R config = {{ filename = "{hrir_str}" channel = 13 }} }}
                    {{ type = builtin label = convolver name = convLFE_L config = {{ filename = "{hrir_str}" channel =  6 }} }}
                    {{ type = builtin label = convolver name = convLFE_R config = {{ filename = "{hrir_str}" channel = 13 }} }}
                    {{ type = builtin label = mixer name = mixL }}
                    {{ type = builtin label = mixer name = mixR }}
                ]
                links = [
                    {{ output = "copyFL:Out"  input="convFL_L:In"  }}
                    {{ output = "copyFL:Out"  input="convFL_R:In"  }}
                    {{ output = "copySL:Out"  input="convSL_L:In"  }}
                    {{ output = "copySL:Out"  input="convSL_R:In"  }}
                    {{ output = "copyRL:Out"  input="convRL_L:In"  }}
                    {{ output = "copyRL:Out"  input="convRL_R:In"  }}
                    {{ output = "copyFC:Out"  input="convFC_L:In"  }}
                    {{ output = "copyFR:Out"  input="convFR_R:In"  }}
                    {{ output = "copyFR:Out"  input="convFR_L:In"  }}
                    {{ output = "copySR:Out"  input="convSR_R:In"  }}
                    {{ output = "copySR:Out"  input="convSR_L:In"  }}
                    {{ output = "copyRR:Out"  input="convRR_R:In"  }}
                    {{ output = "copyRR:Out"  input="convRR_L:In"  }}
                    {{ output = "copyFC:Out"  input="convFC_R:In"  }}
                    {{ output = "copyLFE:Out" input="convLFE_L:In" }}
                    {{ output = "copyLFE:Out" input="convLFE_R:In" }}
                    {{ output = "convFL_L:Out"  input="mixL:In 1" }}
                    {{ output = "convFL_R:Out"  input="mixR:In 1" }}
                    {{ output = "convSL_L:Out"  input="mixL:In 2" }}
                    {{ output = "convSL_R:Out"  input="mixR:In 2" }}
                    {{ output = "convRL_L:Out"  input="mixL:In 3" }}
                    {{ output = "convRL_R:Out"  input="mixR:In 3" }}
                    {{ output = "convFC_L:Out"  input="mixL:In 4" }}
                    {{ output = "convFC_R:Out"  input="mixR:In 4" }}
                    {{ output = "convFR_R:Out"  input="mixR:In 5" }}
                    {{ output = "convFR_L:Out"  input="mixL:In 5" }}
                    {{ output = "convSR_R:Out"  input="mixR:In 6" }}
                    {{ output = "convSR_L:Out"  input="mixL:In 6" }}
                    {{ output = "convRR_R:Out"  input="mixR:In 7" }}
                    {{ output = "convRR_L:Out"  input="mixL:In 7" }}
                    {{ output = "convLFE_R:Out" input="mixR:In 8" }}
                    {{ output = "convLFE_L:Out" input="mixL:In 8" }}
                ]
                inputs  = [ "copyFL:In" "copyFR:In" "copyFC:In" "copyLFE:In" "copyRL:In" "copyRR:In" "copySL:In" "copySR:In" ]
                outputs = [ "mixL:Out" "mixR:Out" ]
            }}
            capture.props = {{
                node.name      = "{sink_name}"
                media.class    = Audio/Sink
                audio.channels = 8
                audio.position = [ FL FR FC LFE RL RR SL SR ]
            }}
            playback.props = {{
                node.name      = "effect_output.sonomux-surround"
                node.passive   = true
                audio.channels = 2
                audio.position = [ FL FR ]
                target.object  = "{target_sink}"
            }}
        }}
    }}
]
"#
    )
}

/// Manages the runtime lifecycle of the PipeWire virtual surround filter-chain process.
pub struct SurroundManager {
    child: Option<tokio::process::Child>,
    config_path: PathBuf,
}

impl Default for SurroundManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SurroundManager {
    pub fn new() -> Self {
        let config_path = if let Ok(cfg_dir) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(cfg_dir)
                .join("sonomux")
                .join("surround-filter.conf")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".config")
                .join("sonomux")
                .join("surround-filter.conf")
        } else {
            PathBuf::from("/tmp/sonomux-surround-filter.conf")
        };

        Self {
            child: None,
            config_path,
        }
    }

    /// Check if the filter-chain background process is currently running.
    pub fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => true,
                Ok(Some(status)) => {
                    debug!("Surround filter process exited with status: {}", status);
                    self.child = None;
                    false
                }
                Err(e) => {
                    warn!("Surround filter try_wait error: {}", e);
                    self.child = None;
                    false
                }
            }
        } else {
            false
        }
    }

    /// Start the PipeWire virtual surround 7.1 filter-chain.
    #[cfg(not(target_os = "macos"))]
    pub async fn start(
        &mut self,
        config: &SurroundConfig,
        target_sink: &str,
    ) -> Result<(), BackendError> {
        if self.is_running() {
            debug!("SurroundManager: Filter chain is already running.");
            return Ok(());
        }

        let hrir_path = resolve_hrir_path(&config.profile, config.custom_wav.as_deref())
            .ok_or_else(|| {
                BackendError::NotFound(format!("HRIR profile '{}' not found", config.profile))
            })?;

        info!(
            "Starting 7.1 Virtual Surround: profile='{}' ({}), target='{}'...",
            config.profile,
            hrir_path.display(),
            target_sink
        );

        let conf_content = generate_filter_chain_config(
            &hrir_path,
            &config.sink_name,
            "Sonomux 7.1 Virtual Surround",
            target_sink,
        );

        if let Some(parent) = self.config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        std::fs::write(&self.config_path, conf_content).map_err(|e| {
            BackendError::Connection(format!("Failed to write surround conf: {}", e))
        })?;

        let child = tokio::process::Command::new("pipewire")
            .arg("-c")
            .arg(&self.config_path)
            .spawn()
            .map_err(|e| {
                BackendError::Connection(format!("Failed to spawn pipewire filter-chain: {}", e))
            })?;

        self.child = Some(child);
        info!("Sonomux 7.1 Virtual Surround filter-chain process started.");
        Ok(())
    }

    /// Stop the PipeWire virtual surround 7.1 filter-chain.
    #[cfg(not(target_os = "macos"))]
    pub async fn stop(&mut self) -> Result<(), BackendError> {
        if let Some(mut child) = self.child.take() {
            info!("Stopping Sonomux 7.1 Virtual Surround filter-chain...");
            let _ = child.kill().await;
            let _ = child.wait().await;
            info!("Sonomux 7.1 Virtual Surround filter-chain stopped.");
        }
        Ok(())
    }

    /// Restart with new configuration.
    #[cfg(not(target_os = "macos"))]
    pub async fn restart(
        &mut self,
        config: &SurroundConfig,
        target_sink: &str,
    ) -> Result<(), BackendError> {
        self.stop().await?;
        self.start(config, target_sink).await
    }

    /// macOS stub: CoreAudio has spatial audio or direct stereo routing.
    #[cfg(target_os = "macos")]
    pub async fn start(
        &mut self,
        _config: &SurroundConfig,
        _target_sink: &str,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    /// macOS stub.
    #[cfg(target_os = "macos")]
    pub async fn stop(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    /// macOS stub.
    #[cfg(target_os = "macos")]
    pub async fn restart(
        &mut self,
        _config: &SurroundConfig,
        _target_sink: &str,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

impl Drop for SurroundManager {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_filter_chain_config() {
        let hrir = PathBuf::from("/usr/share/sonomux/hrir/atmos.wav");
        let conf = generate_filter_chain_config(
            &hrir,
            "effect_input.sonomux-surround",
            "Sonomux 7.1 Virtual Surround",
            "alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink",
        );

        assert!(conf.contains("effect_input.sonomux-surround"));
        assert!(conf.contains("effect_output.sonomux-surround"));
        assert!(conf.contains("alsa_output.usb-TC-Helicon_GoXLRMini-00.HiFi__Line1__sink"));
        assert!(conf.contains("/usr/share/sonomux/hrir/atmos.wav"));
        assert!(conf.contains("audio.channels = 8"));
        assert!(conf.contains("[ FL FR FC LFE RL RR SL SR ]"));
        assert!(conf.contains("libpipewire-module-filter-chain"));
    }

    #[test]
    fn test_list_and_resolve_profiles() {
        let profiles = list_available_profiles();
        // Since assets/hrir is in repo, it should find atmos, dtshx, etc.
        assert!(profiles.contains(&"atmos".to_string()));
        assert!(profiles.contains(&"dtshx".to_string()));

        let resolved = resolve_hrir_path("atmos", None);
        assert!(resolved.is_some());
        assert!(resolved.unwrap().to_string_lossy().contains("atmos.wav"));

        let non_existent = resolve_hrir_path("non_existent_profile_xyz", None);
        // Falls back to atmos.wav
        assert!(non_existent.is_some());
        assert!(non_existent
            .unwrap()
            .to_string_lossy()
            .contains("atmos.wav"));
    }

    #[tokio::test]
    async fn test_surround_manager_constructor() {
        let mut mgr = SurroundManager::default();
        assert!(!mgr.is_running());
        assert!(mgr.stop().await.is_ok());
    }

    #[test]
    fn test_custom_wav_resolution() {
        // Anchor to the crate: unit tests run with the crate dir as cwd, so a bare
        // "assets/hrir/atmos.wav" never exists and an installed profile would win.
        let custom = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/hrir/atmos.wav");
        let resolved = resolve_hrir_path("ignored", Some(custom));
        assert_eq!(resolved, Some(PathBuf::from(custom)));

        let missing_custom = "non_existent_folder/custom.wav";
        let resolved_fallback = resolve_hrir_path("atmos", Some(missing_custom));
        assert!(resolved_fallback.is_some());
        assert!(resolved_fallback
            .unwrap()
            .to_string_lossy()
            .contains("atmos.wav"));
    }

    #[test]
    fn test_hrir_search_directories() {
        let dirs = hrir_search_directories();
        assert!(!dirs.is_empty());
        assert!(dirs.iter().any(|d| d.ends_with("assets/hrir")));
    }
}
