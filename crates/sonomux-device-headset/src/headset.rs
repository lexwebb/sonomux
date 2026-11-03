use async_trait::async_trait;
use sonomux_core::traits::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct HeadsetTelemetry {
    pub is_active: bool,
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
}

#[derive(Clone)]
pub struct HeadsetMonitor {
    metadata: DeviceMetadata,
    telemetry: Arc<Mutex<HeadsetTelemetry>>,
    event_tx: broadcast::Sender<DeviceEvent>,
    debounce_threshold: usize,
}

impl HeadsetMonitor {
    pub fn new(name: &str, debounce_threshold: usize) -> Self {
        let (tx, _) = broadcast::channel(32);
        Self {
            metadata: DeviceMetadata {
                id: "headset".to_string(),
                name: name.to_string(),
                vendor: "SteelSeries".to_string(),
                capabilities: vec!["presence_detection".to_string(), "battery".to_string()],
            },
            telemetry: Arc::new(Mutex::new(HeadsetTelemetry {
                is_active: false,
                battery_percent: None,
                is_charging: None,
            })),
            event_tx: tx,
            debounce_threshold: debounce_threshold.max(1),
        }
    }

    /// Read single raw status snapshot using `headsetcontrol -o env -b`.
    pub async fn probe_raw() -> Result<HeadsetTelemetry, DeviceError> {
        let output = Command::new("headsetcontrol")
            .args(["-o", "env", "-b"])
            .output()
            .await
            .map_err(|e| DeviceError::Communication(format!("Failed to execute headsetcontrol: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut status_str = String::new();
        let mut battery_level: i32 = -1;

        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("DEVICE_0_BATTERY_STATUS=") {
                status_str = rest.trim_matches('"').to_string();
            } else if let Some(rest) = line.strip_prefix("DEVICE_0_BATTERY_LEVEL=") {
                if let Ok(lvl) = rest.trim().parse::<i32>() {
                    battery_level = lvl;
                }
            }
        }

        let is_active = !status_str.is_empty() && status_str != "BATTERY_UNAVAILABLE";
        let is_charging = if status_str == "BATTERY_CHARGING" {
            Some(true)
        } else if is_active {
            Some(false)
        } else {
            None
        };

        let battery_percent = if is_active && battery_level >= 0 {
            Some(battery_level.clamp(0, 100) as u8)
        } else {
            None
        };

        Ok(HeadsetTelemetry {
            is_active,
            battery_percent,
            is_charging,
        })
    }

    /// Spawn a background debounced polling loop.
    pub fn start_monitor(&self, interval: Duration) -> tokio::task::JoinHandle<()> {
        let telemetry_arc = Arc::clone(&self.telemetry);
        let tx = self.event_tx.clone();
        let threshold = self.debounce_threshold;
        let device_id = self.metadata.id.clone();

        tokio::spawn(async move {
            let mut pending_active: Option<bool> = None;
            let mut pending_count = 0;
            let mut last_confirmed_active: Option<bool> = None;
            let mut last_battery: Option<u8> = None;

            loop {
                match Self::probe_raw().await {
                    Ok(current) => {
                        // Debounce state transitions
                        if pending_active == Some(current.is_active) {
                            pending_count += 1;
                        } else {
                            pending_active = Some(current.is_active);
                            pending_count = 1;
                        }

                        if pending_count >= threshold {
                            let newly_active = current.is_active;
                            if last_confirmed_active != Some(newly_active) {
                                last_confirmed_active = Some(newly_active);
                                let new_status = if newly_active {
                                    DeviceStatus::Active
                                } else {
                                    DeviceStatus::Standby
                                };
                                info!("Headset state confirmed: {:?}", new_status);

                                let _ = tx.send(DeviceEvent::StatusChanged {
                                    id: device_id.clone(),
                                    status: new_status,
                                });
                            }

                            // Update battery telemetry if changed
                            if current.battery_percent != last_battery && current.battery_percent.is_some() {
                                last_battery = current.battery_percent;
                                if let Some(pct) = current.battery_percent {
                                    debug!("Headset battery updated: {}%", pct);
                                    let _ = tx.send(DeviceEvent::BatteryChanged {
                                        id: device_id.clone(),
                                        percentage: pct,
                                    });
                                }
                            }

                            // Update cached telemetry
                            let mut guard = telemetry_arc.lock().await;
                            *guard = current;
                        }
                    }
                    Err(e) => {
                        debug!("Headset probe error: {}", e);
                    }
                }

                tokio::time::sleep(interval).await;
            }
        })
    }
}

#[async_trait]
impl AudioDevice for HeadsetMonitor {
    fn metadata(&self) -> &DeviceMetadata {
        &self.metadata
    }

    async fn status(&self) -> DeviceStatus {
        let guard = self.telemetry.lock().await;
        if guard.is_active {
            DeviceStatus::Active
        } else {
            DeviceStatus::Standby
        }
    }

    async fn subscribe_events(&self) -> broadcast::Receiver<DeviceEvent> {
        self.event_tx.subscribe()
    }
}

#[async_trait]
impl HasPresenceDetection for HeadsetMonitor {
    async fn is_physically_active(&self) -> bool {
        let guard = self.telemetry.lock().await;
        guard.is_active
    }

    async fn battery_level(&self) -> Option<u8> {
        let guard = self.telemetry.lock().await;
        guard.battery_percent
    }

    async fn is_charging(&self) -> Option<bool> {
        let guard = self.telemetry.lock().await;
        guard.is_charging
    }
}
