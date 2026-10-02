//! Keeps the Claude Code plugin files consistent with the crate.

use serde_json::Value;
use std::{fs, path::Path};

fn json(path: &str) -> Value {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

#[test]
fn plugin_version_matches_crate_version() {
    let plugin = json(".claude-plugin/plugin.json");
    assert_eq!(plugin["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn marketplace_lists_the_plugin_at_the_repo_root() {
    let plugin = json(".claude-plugin/plugin.json");
    let market = json(".claude-plugin/marketplace.json");
    let entry = &market["plugins"][0];
    assert_eq!(entry["name"], plugin["name"]);
    assert_eq!(entry["source"], ".");
}

#[test]
fn hook_runs_sessions_hook_on_every_session_start_source() {
    let hooks = json("hooks/hooks.json");
    let entry = &hooks["hooks"]["SessionStart"][0];
    let matcher = entry["matcher"].as_str().unwrap();
    for source in ["startup", "resume", "clear", "compact", "fork"] {
        assert!(matcher.split('|').any(|m| m == source), "missing {source}");
    }
    let command = entry["hooks"][0]["command"].as_str().unwrap();
    assert!(command.contains("sessions hook"));
    // A missing binary must never break session start.
    assert!(command.trim_end().ends_with("exit 0"));
}
