use sonomux_core::rules::*;
use sonomux_core::traits::DeviceStatus;

#[test]
fn test_default_rules_generated() {
    let rules = default_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].name, "Auto Headset Switcher");
    assert!(rules[0].enabled);
    assert_eq!(rules[0].actions_on_enter.len(), 3);
    assert_eq!(rules[0].actions_on_exit.len(), 2);
}

#[test]
fn test_rule_default() {
    let rule = Rule::default();
    assert_eq!(rule.name, "Default Rule");
    assert!(rule.enabled);
}

#[test]
fn test_trigger_device_status() {
    let trigger = Trigger::DeviceStatus {
        device_id: "headset".to_string(),
        status: DeviceStatus::Active,
    };

    let ctx_active = RuleContext::new().with_device_status("headset", DeviceStatus::Active);
    assert!(RuleEngine::evaluate_trigger(&trigger, &ctx_active));

    let ctx_standby = RuleContext::new().with_device_status("headset", DeviceStatus::Standby);
    assert!(!RuleEngine::evaluate_trigger(&trigger, &ctx_standby));

    let ctx_empty = RuleContext::new();
    assert!(!RuleEngine::evaluate_trigger(&trigger, &ctx_empty));
}

#[test]
fn test_trigger_battery_level() {
    let trigger_low = Trigger::BatteryLevel {
        device_id: "headset".to_string(),
        below: Some(20),
        above: None,
    };
    let trigger_range = Trigger::BatteryLevel {
        device_id: "headset".to_string(),
        below: Some(80),
        above: Some(20),
    };

    let ctx_10 = RuleContext::new().with_device_battery("headset", 10);
    let ctx_50 = RuleContext::new().with_device_battery("headset", 50);
    let ctx_90 = RuleContext::new().with_device_battery("headset", 90);
    let ctx_none = RuleContext::new();

    assert!(RuleEngine::evaluate_trigger(&trigger_low, &ctx_10));
    assert!(!RuleEngine::evaluate_trigger(&trigger_low, &ctx_50));
    assert!(!RuleEngine::evaluate_trigger(&trigger_low, &ctx_none));

    assert!(!RuleEngine::evaluate_trigger(&trigger_range, &ctx_10));
    assert!(RuleEngine::evaluate_trigger(&trigger_range, &ctx_50));
    assert!(!RuleEngine::evaluate_trigger(&trigger_range, &ctx_90));
}

#[test]
fn test_trigger_node_presence() {
    let trigger_present = Trigger::NodePresence {
        node_name: "test_sink".to_string(),
        present: true,
    };
    let trigger_absent = Trigger::NodePresence {
        node_name: "test_sink".to_string(),
        present: false,
    };

    let ctx_with = RuleContext::new().with_node("test_sink", true);
    let ctx_without = RuleContext::new().with_node("test_sink", false);
    let ctx_empty = RuleContext::new();

    assert!(RuleEngine::evaluate_trigger(&trigger_present, &ctx_with));
    assert!(!RuleEngine::evaluate_trigger(
        &trigger_present,
        &ctx_without
    ));
    assert!(!RuleEngine::evaluate_trigger(&trigger_present, &ctx_empty));

    assert!(!RuleEngine::evaluate_trigger(&trigger_absent, &ctx_with));
    assert!(RuleEngine::evaluate_trigger(&trigger_absent, &ctx_without));
    assert!(RuleEngine::evaluate_trigger(&trigger_absent, &ctx_empty));
}

#[test]
fn test_trigger_composite_and_or() {
    let t1 = Trigger::DeviceStatus {
        device_id: "headset".to_string(),
        status: DeviceStatus::Active,
    };
    let t2 = Trigger::NodePresence {
        node_name: "headset_sink".to_string(),
        present: true,
    };

    let and_trigger = Trigger::And(vec![t1.clone(), t2.clone()]);
    let or_trigger = Trigger::Or(vec![t1.clone(), t2.clone()]);

    let ctx_both = RuleContext::new()
        .with_device_status("headset", DeviceStatus::Active)
        .with_node("headset_sink", true);

    let ctx_only_headset = RuleContext::new()
        .with_device_status("headset", DeviceStatus::Active)
        .with_node("headset_sink", false);

    let ctx_neither = RuleContext::new()
        .with_device_status("headset", DeviceStatus::Standby)
        .with_node("headset_sink", false);

    assert!(RuleEngine::evaluate_trigger(&and_trigger, &ctx_both));
    assert!(!RuleEngine::evaluate_trigger(
        &and_trigger,
        &ctx_only_headset
    ));

    assert!(RuleEngine::evaluate_trigger(&or_trigger, &ctx_both));
    assert!(RuleEngine::evaluate_trigger(&or_trigger, &ctx_only_headset));
    assert!(!RuleEngine::evaluate_trigger(&or_trigger, &ctx_neither));
}

#[test]
fn test_rule_engine_transitions() {
    let rule = Rule {
        name: "Test Rule".to_string(),
        enabled: true,
        trigger: Trigger::DeviceStatus {
            device_id: "headset".to_string(),
            status: DeviceStatus::Active,
        },
        actions_on_enter: vec![Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 0.0,
        }],
        actions_on_exit: vec![Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 1.0,
        }],
    };

    let mut engine = RuleEngine::new(vec![rule]);
    assert_eq!(engine.rules().len(), 1);
    assert!(!engine.is_rule_active("Test Rule"));

    // 1. Initial evaluation with Standby: no actions
    let ctx_standby = RuleContext::new().with_device_status("headset", DeviceStatus::Standby);
    let acts1 = engine.evaluate(&ctx_standby);
    assert!(acts1.is_empty());
    assert!(!engine.is_rule_active("Test Rule"));

    // 2. Transition to Active: yields enter actions
    let ctx_active = RuleContext::new().with_device_status("headset", DeviceStatus::Active);
    let acts2 = engine.evaluate(&ctx_active);
    assert_eq!(acts2.len(), 1);
    assert_eq!(
        acts2[0],
        Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 0.0,
        }
    );
    assert!(engine.is_rule_active("Test Rule"));

    // 3. Second evaluation while still Active: no actions (already active)
    let acts3 = engine.evaluate(&ctx_active);
    assert!(acts3.is_empty());
    assert!(engine.is_rule_active("Test Rule"));

    // 4. Transition back to Standby: yields exit actions
    let acts4 = engine.evaluate(&ctx_standby);
    assert_eq!(acts4.len(), 1);
    assert_eq!(
        acts4[0],
        Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 1.0,
        }
    );
    assert!(!engine.is_rule_active("Test Rule"));
}

#[test]
fn test_disabled_rule_is_ignored() {
    let rule = Rule {
        enabled: false,
        actions_on_enter: vec![Action::DestroyLoopback {
            source: "test".to_string(),
        }],
        ..Rule::default()
    };

    let mut engine = RuleEngine::new(vec![rule]);
    let ctx_active = RuleContext::new().with_device_status("headset", DeviceStatus::Active);
    let acts = engine.evaluate(&ctx_active);
    assert!(acts.is_empty());
}

#[test]
fn test_volume_slots() {
    let mut engine = RuleEngine::default();
    assert_eq!(engine.get_volume_slot("slot1"), None);

    engine.save_volume_slot("slot1", 0.75);
    assert_eq!(engine.get_volume_slot("slot1"), Some(0.75));

    engine.save_volume_slot("slot1", 0.33);
    assert_eq!(engine.get_volume_slot("slot1"), Some(0.33));
}

#[test]
fn test_toml_rule_serialization_roundtrip() {
    let rule = Rule {
        name: "Custom Surround Pinning".to_string(),
        enabled: true,
        trigger: Trigger::NodePresence {
            node_name: "effect_output.vsm".to_string(),
            present: true,
        },
        actions_on_enter: vec![
            Action::EnsureLoopback {
                source: "src_1".to_string(),
                sink: "sink_1".to_string(),
                latency_ms: 15,
            },
            Action::SetHardwareMute {
                device_id: "goxlr".to_string(),
                channel: "LineOut".to_string(),
                muted: true,
            },
            Action::SetRouting {
                device_id: "goxlr".to_string(),
                input: "Mic".to_string(),
                output: "Sampler".to_string(),
                enabled: true,
            },
            Action::SetupSubmixMatrix {
                device_id: "goxlr".to_string(),
            },
            Action::EnforceSurround {
                surround_out: "vsm_out".to_string(),
                surround_in: "vsm_in".to_string(),
                game_sink: "game".to_string(),
                system_sink: "sys".to_string(),
                keep_off_apps: vec!["Zen".to_string(), "Discord".to_string()],
            },
            Action::Command {
                program: "notify-send".to_string(),
                args: vec!["Sonomux".to_string(), "Headset active".to_string()],
            },
        ],
        actions_on_exit: vec![Action::DestroyLoopback {
            source: "src_1".to_string(),
        }],
    };

    let toml_str = toml::to_string_pretty(&rule).expect("Failed to serialize rule");
    let parsed: Rule = toml::from_str(&toml_str).expect("Failed to deserialize rule");
    assert_eq!(parsed, rule);
}

#[test]
fn test_trigger_stream_category_active() {
    let trigger = Trigger::StreamCategoryActive {
        category: "Game".to_string(),
    };

    let ctx_with = RuleContext::new().with_stream_category("Game", true);
    let ctx_without = RuleContext::new().with_stream_category("Game", false);
    let ctx_empty = RuleContext::new();

    assert!(RuleEngine::evaluate_trigger(&trigger, &ctx_with));
    assert!(!RuleEngine::evaluate_trigger(&trigger, &ctx_without));
    assert!(!RuleEngine::evaluate_trigger(&trigger, &ctx_empty));
}

#[test]
fn test_action_route_stream_serialization() {
    let act = Action::RouteStream {
        app_or_binary: "spotify".to_string(),
        target_sink: "goxlr_music".to_string(),
    };

    let toml_str = toml::to_string(&act).expect("Failed to serialize action");
    let parsed: Action = toml::from_str(&toml_str).expect("Failed to deserialize action");
    assert_eq!(parsed, act);
}

fn sample_rule(enabled: bool) -> Rule {
    Rule {
        name: "R".to_string(),
        enabled,
        trigger: Trigger::DeviceStatus {
            device_id: "headset".to_string(),
            status: DeviceStatus::Active,
        },
        actions_on_enter: vec![Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 0.0,
        }],
        actions_on_exit: vec![Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 1.0,
        }],
    }
}

fn active_ctx() -> RuleContext {
    RuleContext::new().with_device_status("headset", DeviceStatus::Active)
}

#[test]
fn test_set_rules_keeps_active_state() {
    let mut e = RuleEngine::new(vec![sample_rule(true)]);
    assert_eq!(e.evaluate(&active_ctx()).len(), 1);
    assert!(e.set_rules(vec![sample_rule(true)]).is_empty());
    assert!(e.is_rule_active("R"));
    assert!(e.evaluate(&active_ctx()).is_empty());
}

#[test]
fn test_disabling_active_rule_runs_exit_actions() {
    let mut e = RuleEngine::new(vec![sample_rule(true)]);
    e.evaluate(&active_ctx());
    let acts = e.set_rules(vec![sample_rule(false)]);
    assert_eq!(acts, sample_rule(true).actions_on_exit);
    assert!(!e.is_rule_active("R"));
}

#[test]
fn test_removing_active_rule_runs_exit_actions() {
    let mut e = RuleEngine::new(vec![sample_rule(true)]);
    e.evaluate(&active_ctx());
    assert_eq!(e.set_rules(vec![]), sample_rule(true).actions_on_exit);
}

#[test]
fn test_disabling_inactive_rule_runs_nothing() {
    let mut e = RuleEngine::new(vec![sample_rule(true)]);
    assert!(e.set_rules(vec![sample_rule(false)]).is_empty());
}

#[test]
fn test_reenabled_rule_enters_on_next_evaluate() {
    let mut e = RuleEngine::new(vec![sample_rule(true)]);
    e.evaluate(&active_ctx());
    assert_eq!(e.set_rules(vec![sample_rule(false)]).len(), 1);
    assert!(e.set_rules(vec![sample_rule(true)]).is_empty());
    assert_eq!(
        e.evaluate(&active_ctx()),
        sample_rule(true).actions_on_enter
    );
}

#[test]
fn test_default_rule_loses_loopback_dependent_actions() {
    let original = default_rules();
    let (rules, changed) = without_loopback_actions(original.clone());
    assert_eq!(rules.len(), 1);
    assert!(rules[0].actions_on_enter.is_empty());
    assert!(rules[0].actions_on_exit.is_empty());
    assert_eq!(changed, vec!["Auto Headset Switcher".to_string()]);
    assert_eq!(rules[0].name, original[0].name);
    assert_eq!(rules[0].trigger, original[0].trigger);
    assert_eq!(rules[0].enabled, original[0].enabled);
}

#[test]
fn test_rule_without_loopback_is_untouched() {
    let rule = Rule {
        name: "Volume only".to_string(),
        actions_on_enter: vec![Action::SetHardwareVolume {
            device_id: "goxlr".to_string(),
            channel: "Headphones".to_string(),
            level: 0.5,
        }],
        ..Rule::default()
    };
    let (rules, changed) = without_loopback_actions(vec![rule.clone()]);
    assert_eq!(rules, vec![rule]);
    assert!(changed.is_empty());
}

#[test]
fn test_other_actions_are_kept() {
    let command = Action::Command {
        program: "notify-send".to_string(),
        args: vec!["hi".to_string()],
    };
    let rule = Rule {
        name: "Mixed".to_string(),
        actions_on_enter: vec![
            Action::EnsureLoopback {
                source: "a".to_string(),
                sink: "b".to_string(),
                latency_ms: 20,
            },
            command.clone(),
            Action::SetHardwareVolume {
                device_id: "goxlr".to_string(),
                channel: "Headphones".to_string(),
                level: 0.0,
            },
        ],
        ..Rule::default()
    };
    let (rules, _) = without_loopback_actions(vec![rule]);
    assert_eq!(rules[0].actions_on_enter, vec![command]);
}
