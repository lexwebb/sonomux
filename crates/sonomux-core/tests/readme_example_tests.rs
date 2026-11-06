//! Keeps the README's TOML rule example valid against the real config schema.

use sonomux_core::config::AppConfig;
use sonomux_core::rules::Trigger;
use sonomux_core::traits::DeviceStatus;

fn readme_example() -> String {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
        .expect("README.md should be readable");
    let mut rest = readme.as_str();
    while let Some(start) = rest.find("```toml\n") {
        let body = &rest[start + "```toml\n".len()..];
        let end = body.find("```").expect("unterminated toml fence in README");
        if body[..end].contains("[[rules]]") {
            return body[..end].to_string();
        }
        rest = &body[end..];
    }
    panic!("no ```toml block containing [[rules]] found in README.md");
}

/// Panics with the missing path if any key in `want` is absent from `got`.
fn assert_keys_present(want: &toml::Value, got: &toml::Value, path: &str) {
    match (want, got) {
        (toml::Value::Table(w), toml::Value::Table(g)) => {
            for (k, wv) in w {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                let gv = g
                    .get(k)
                    .unwrap_or_else(|| panic!("README key `{p}` is ignored by the config schema"));
                assert_keys_present(wv, gv, &p);
            }
        }
        (toml::Value::Array(w), toml::Value::Array(g)) => {
            for (i, wv) in w.iter().enumerate() {
                let p = format!("{path}[{i}]");
                let gv = g
                    .get(i)
                    .unwrap_or_else(|| panic!("README entry `{p}` is missing after round trip"));
                assert_keys_present(wv, gv, &p);
            }
        }
        _ => {}
    }
}

#[test]
fn readme_config_example_parses() {
    let cfg: AppConfig =
        toml::from_str(&readme_example()).expect("README config example no longer parses");
    assert_eq!(cfg.rules.len(), 1);
    assert!(matches!(
        &cfg.rules[0].trigger,
        Trigger::DeviceStatus { device_id, status }
            if device_id == "headset" && *status == DeviceStatus::Active
    ));
}

#[test]
fn readme_config_example_has_no_ignored_keys() {
    let text = readme_example();
    let cfg: AppConfig = toml::from_str(&text).expect("README config example no longer parses");
    let want: toml::Value = toml::from_str(&text).unwrap();
    let got = toml::Value::try_from(&cfg).unwrap();
    assert_keys_present(&want, &got, "");
}
