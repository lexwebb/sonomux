use crate::traits::DeviceStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_latency() -> u32 {
    20
}

fn default_volume() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

/// A declarative automation rule that responds to hardware and graph events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub trigger: Trigger,
    #[serde(default)]
    pub actions_on_enter: Vec<Action>,
    #[serde(default)]
    pub actions_on_exit: Vec<Action>,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            name: "Default Rule".to_string(),
            enabled: true,
            trigger: Trigger::DeviceStatus {
                device_id: "headset".to_string(),
                status: DeviceStatus::Active,
            },
            actions_on_enter: Vec::new(),
            actions_on_exit: Vec::new(),
        }
    }
}

pub fn default_rules() -> Vec<Rule> {
    vec![Rule {
        name: "Auto Headset Switcher".to_string(),
        enabled: true,
        trigger: Trigger::DeviceStatus {
            device_id: "headset".to_string(),
            status: DeviceStatus::Active,
        },
        actions_on_enter: vec![
            Action::SaveHardwareVolume {
                device_id: "goxlr".to_string(),
                channel: "Headphones".to_string(),
                slot: "speaker_volume".to_string(),
            },
            Action::SetHardwareVolume {
                device_id: "goxlr".to_string(),
                channel: "Headphones".to_string(),
                level: 0.0,
            },
            Action::EnsureLoopback {
                source: "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source".to_string(),
                sink: "alsa_output.usb-SteelSeries_Arctis_Nova_7-00.analog-stereo".to_string(),
                latency_ms: 20,
            },
        ],
        actions_on_exit: vec![
            Action::DestroyLoopback {
                source: "alsa_input.usb-TC-Helicon_GoXLRMini-00.HiFi__Line5__source".to_string(),
            },
            Action::RestoreHardwareVolume {
                device_id: "goxlr".to_string(),
                channel: "Headphones".to_string(),
                slot: "speaker_volume".to_string(),
                default_level: 1.0,
            },
        ],
    }]
}

/// Trigger condition for a rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Matches when a device enters a specific status (e.g. Active, Standby, Disconnected).
    DeviceStatus {
        device_id: String,
        status: DeviceStatus,
    },
    /// Matches when a device battery percentage is within thresholds.
    BatteryLevel {
        device_id: String,
        #[serde(default)]
        below: Option<u8>,
        #[serde(default)]
        above: Option<u8>,
    },
    /// Matches when an audio node/sink/source is present.
    NodePresence { node_name: String, present: bool },
    /// Matches when an active audio stream in a specific category is detected (e.g. "Game", "Music").
    StreamCategoryActive { category: String },
    /// Composite AND trigger: all conditions must match.
    And(Vec<Trigger>),
    /// Composite OR trigger: at least one condition must match.
    Or(Vec<Trigger>),
}

/// Action executed when a rule triggers or untriggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Load or verify low-latency loopback between source and sink.
    EnsureLoopback {
        source: String,
        sink: String,
        #[serde(default = "default_latency")]
        latency_ms: u32,
    },
    /// Remove an active loopback module for the given source.
    DestroyLoopback { source: String },
    /// Set a hardware channel volume level (0.0 to 1.0).
    SetHardwareVolume {
        device_id: String,
        channel: String,
        level: f32,
    },
    /// Save a hardware channel volume level into a named state slot.
    SaveHardwareVolume {
        device_id: String,
        channel: String,
        slot: String,
    },
    /// Restore a hardware channel volume level from a named state slot.
    RestoreHardwareVolume {
        device_id: String,
        channel: String,
        slot: String,
        #[serde(default = "default_volume")]
        default_level: f32,
    },
    /// Set channel mute state on a hardware device.
    SetHardwareMute {
        device_id: String,
        channel: String,
        muted: bool,
    },
    /// Set a cross-point matrix route on a multi-channel mixer.
    SetRouting {
        device_id: String,
        input: String,
        output: String,
        enabled: bool,
    },
    /// Setup submix sampler routing matrix on GoXLR or similar device.
    SetupSubmixMatrix { device_id: String },
    /// Enforce Virtual Surround pinning policies.
    EnforceSurround {
        surround_out: String,
        surround_in: String,
        game_sink: String,
        system_sink: String,
        #[serde(default)]
        keep_off_apps: Vec<String>,
    },
    /// Execute an external notification or shell command.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// Route an application or stream to a specific target sink.
    RouteStream {
        app_or_binary: String,
        target_sink: String,
    },
}

/// Snapshot of system context evaluated against triggers.
#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    pub device_statuses: HashMap<String, DeviceStatus>,
    pub device_batteries: HashMap<String, u8>,
    pub present_nodes: HashMap<String, bool>,
    pub active_stream_categories: HashMap<String, bool>,
}

impl RuleContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_device_status(mut self, device_id: &str, status: DeviceStatus) -> Self {
        self.device_statuses.insert(device_id.to_string(), status);
        self
    }

    pub fn with_device_battery(mut self, device_id: &str, battery: u8) -> Self {
        self.device_batteries.insert(device_id.to_string(), battery);
        self
    }

    pub fn with_node(mut self, node: &str, present: bool) -> Self {
        self.present_nodes.insert(node.to_string(), present);
        self
    }

    pub fn with_stream_category(mut self, category: &str, active: bool) -> Self {
        self.active_stream_categories
            .insert(category.to_string(), active);
        self
    }
}

/// Engine that stores rules and evaluates transitions based on system context.
#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    rules: Vec<Rule>,
    active_rules: HashMap<String, bool>,
    volume_slots: HashMap<String, f32>,
}

impl RuleEngine {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self {
            rules,
            active_rules: HashMap::new(),
            volume_slots: HashMap::new(),
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn is_rule_active(&self, rule_name: &str) -> bool {
        self.active_rules.get(rule_name).copied().unwrap_or(false)
    }

    pub fn save_volume_slot(&mut self, slot: &str, level: f32) {
        self.volume_slots.insert(slot.to_string(), level);
    }

    pub fn get_volume_slot(&self, slot: &str) -> Option<f32> {
        self.volume_slots.get(slot).copied()
    }

    /// Evaluate a single trigger against the current system context.
    pub fn evaluate_trigger(trigger: &Trigger, ctx: &RuleContext) -> bool {
        match trigger {
            Trigger::DeviceStatus { device_id, status } => {
                ctx.device_statuses.get(device_id) == Some(status)
            }
            Trigger::BatteryLevel {
                device_id,
                below,
                above,
            } => {
                if let Some(&level) = ctx.device_batteries.get(device_id) {
                    if let Some(min) = above {
                        if level <= *min {
                            return false;
                        }
                    }
                    if let Some(max) = below {
                        if level >= *max {
                            return false;
                        }
                    }
                    true
                } else {
                    false
                }
            }
            Trigger::NodePresence { node_name, present } => {
                let actual = ctx.present_nodes.get(node_name).copied().unwrap_or(false);
                actual == *present
            }
            Trigger::StreamCategoryActive { category } => {
                ctx.active_stream_categories
                    .get(category)
                    .copied()
                    .unwrap_or(false)
            }
            Trigger::And(triggers) => triggers.iter().all(|t| Self::evaluate_trigger(t, ctx)),
            Trigger::Or(triggers) => triggers.iter().any(|t| Self::evaluate_trigger(t, ctx)),
        }
    }

    /// Evaluate all rules against the context, returning enter/exit actions for any state transitions.
    pub fn evaluate(&mut self, ctx: &RuleContext) -> Vec<Action> {
        let mut actions = Vec::new();

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            let matches = Self::evaluate_trigger(&rule.trigger, ctx);
            let was_active = self.is_rule_active(&rule.name);

            if matches && !was_active {
                // Rule entered
                self.active_rules.insert(rule.name.clone(), true);
                for act in &rule.actions_on_enter {
                    actions.push(act.clone());
                }
            } else if !matches && was_active {
                // Rule exited
                self.active_rules.insert(rule.name.clone(), false);
                for act in &rule.actions_on_exit {
                    actions.push(act.clone());
                }
            }
        }

        actions
    }
}
