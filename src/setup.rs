//! `sessions setup`: registers the SessionStart hook in Claude Code's `settings.json`.

use crate::store;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Every source on which Claude Code starts a session, so resumes and forks are registered too.
const MATCHER: &str = "startup|resume|clear|compact|fork";

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    AlreadyPresent,
    /// The plugin ships the same hook; adding it again would run it twice.
    PluginInstalled,
}

pub fn settings_path() -> PathBuf {
    store::claude_dir().join("settings.json")
}

/// The shell command Claude Code runs; the path is absolute because the hook's PATH is minimal.
pub fn hook_command(exe: &Path) -> String {
    let path = exe.display().to_string();
    if path.contains(' ') {
        format!("\"{path}\" hook")
    } else {
        format!("{path} hook")
    }
}

/// The SessionStart commands that run `sessions hook`, however they were installed.
pub fn hook_commands(settings: &Value) -> Vec<String> {
    settings["hooks"]["SessionStart"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str())
        .map(str::trim)
        .filter(|cmd| cmd.ends_with(" hook") && cmd.contains("sessions"))
        .map(str::to_string)
        .collect()
}

fn has_hook(settings: &Value) -> bool {
    !hook_commands(settings).is_empty()
}

/// Whether the sessions plugin is enabled, which ships the same hook.
pub fn plugin_enabled(settings: &Value) -> bool {
    settings["enabledPlugins"]
        .as_object()
        .is_some_and(|plugins| {
            plugins
                .iter()
                .any(|(name, on)| name.starts_with("sessions@") && on.as_bool() == Some(true))
        })
}

/// Adds the hook to `settings`. Only `Outcome::Added` changes it; everything else is left as is.
pub fn add_hook(settings: &mut Value, command: &str) -> Result<Outcome> {
    if !settings.is_object() {
        bail!("settings.json is not a JSON object");
    }
    if plugin_enabled(settings) {
        return Ok(Outcome::PluginInstalled);
    }
    if has_hook(settings) {
        return Ok(Outcome::AlreadyPresent);
    }
    let root = settings.as_object_mut().expect("checked above");
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let events = hooks
        .as_object_mut()
        .context("\"hooks\" in settings.json is not an object")?;
    let list = events.entry("SessionStart").or_insert_with(|| json!([]));
    list.as_array_mut()
        .context("\"hooks.SessionStart\" in settings.json is not an array")?
        .push(json!({
            "matcher": MATCHER,
            "hooks": [{ "type": "command", "command": command, "timeout": 10 }]
        }));
    Ok(Outcome::Added)
}

/// Reads `path`, adds the hook and writes it back. The old file is kept as `<name>.bak-<ts>`, and
/// a file that doesn't parse is never touched.
pub fn setup_at(path: &Path, command: &str) -> Result<Outcome> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut settings = match existing.as_deref().map(str::trim) {
        None | Some("") => json!({}),
        Some(text) => serde_json::from_str(text).with_context(|| {
            format!(
                "{} is not valid JSON; fix it first, nothing was changed",
                path.display()
            )
        })?,
    };

    let outcome = add_hook(&mut settings, command)?;
    if outcome == Outcome::Added {
        if existing.is_some() {
            let backup =
                path.with_extension(format!("json.bak-{}", chrono::Utc::now().timestamp()));
            fs::copy(path, &backup)
                .with_context(|| format!("backing up to {}", backup.display()))?;
        } else if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Written in place rather than renamed, so a settings.json that is a symlink into a
        // dotfiles repo stays a symlink.
        fs::write(path, serde_json::to_string_pretty(&settings)? + "\n")
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(outcome)
}

pub fn run() -> Result<()> {
    let path = settings_path();
    let command = hook_command(&std::env::current_exe()?);
    match setup_at(&path, &command)? {
        Outcome::Added => {
            println!("Added the SessionStart hook to {}", path.display());
            println!("It takes effect in new Claude Code sessions.");
            println!("Run `sessions import` to add sessions from before the hook existed.");
        }
        Outcome::AlreadyPresent => println!("The hook is already configured in {}", path.display()),
        Outcome::PluginInstalled => {
            println!("The sessions plugin is enabled and ships the hook, so nothing was changed.")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "/home/u/.local/bin/sessions hook";

    fn tmp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sessions-setup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn commands(settings: &Value) -> Vec<&str> {
        settings["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|e| e["hooks"].as_array().unwrap())
            .filter_map(|h| h["command"].as_str())
            .collect()
    }

    #[test]
    fn adds_the_hook_to_an_empty_object() {
        let mut s = json!({});
        assert_eq!(add_hook(&mut s, CMD).unwrap(), Outcome::Added);
        assert_eq!(commands(&s), [CMD]);
        assert_eq!(s["hooks"]["SessionStart"][0]["matcher"], MATCHER);
    }

    #[test]
    fn keeps_other_settings_hooks_and_key_order() {
        let mut s: Value = serde_json::from_str(
            r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[]}],
                "SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"other.sh"}]}]},
                "theme":"dark"}"#,
        )
        .unwrap();
        assert_eq!(add_hook(&mut s, CMD).unwrap(), Outcome::Added);
        assert_eq!(commands(&s), ["other.sh", CMD]);
        assert!(s["hooks"]["PreToolUse"].is_array());
        let keys: Vec<_> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "theme"]);
    }

    #[test]
    fn is_idempotent_and_recognises_a_hand_written_hook() {
        let mut s = json!({});
        add_hook(&mut s, CMD).unwrap();
        let after_first = s.clone();
        assert_eq!(add_hook(&mut s, CMD).unwrap(), Outcome::AlreadyPresent);
        assert_eq!(s, after_first);

        let mut manual = json!({"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"$HOME/.cargo/bin/sessions hook","timeout":10}]}]}});
        assert_eq!(add_hook(&mut manual, CMD).unwrap(), Outcome::AlreadyPresent);
    }

    #[test]
    fn does_not_double_up_with_the_plugin() {
        let mut s = json!({"enabledPlugins":{"sessions@session-tui":true}});
        let before = s.clone();
        assert_eq!(add_hook(&mut s, CMD).unwrap(), Outcome::PluginInstalled);
        assert_eq!(s, before);
        // A disabled plugin doesn't run the hook, so it still needs adding.
        let mut off = json!({"enabledPlugins":{"sessions@session-tui":false}});
        assert_eq!(add_hook(&mut off, CMD).unwrap(), Outcome::Added);
    }

    #[test]
    fn rejects_settings_of_the_wrong_shape() {
        assert!(add_hook(&mut json!([]), CMD).is_err());
        assert!(add_hook(&mut json!({"hooks": []}), CMD).is_err());
        assert!(add_hook(&mut json!({"hooks": {"SessionStart": {}}}), CMD).is_err());
    }

    #[test]
    fn hook_command_quotes_paths_with_spaces() {
        assert_eq!(hook_command(Path::new("/a/sessions")), "/a/sessions hook");
        assert_eq!(
            hook_command(Path::new("/My Apps/sessions")),
            "\"/My Apps/sessions\" hook"
        );
    }

    #[test]
    fn creates_a_missing_file_without_a_backup() {
        let path = tmp("create");
        assert_eq!(setup_at(&path, CMD).unwrap(), Outcome::Added);
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(commands(&written), [CMD]);
        let entries = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(entries, 1, "only settings.json, no backup");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn backs_up_an_existing_file_before_changing_it() {
        let path = tmp("backup");
        let original = "{\n  \"model\": \"opus\"\n}\n";
        fs::write(&path, original).unwrap();
        assert_eq!(setup_at(&path, CMD).unwrap(), Outcome::Added);
        let backups: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), original);
        // A second run changes nothing and makes no further backup.
        assert_eq!(setup_at(&path, CMD).unwrap(), Outcome::AlreadyPresent);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn leaves_a_file_that_is_not_json_untouched() {
        let path = tmp("invalid");
        fs::write(&path, "{ not json").unwrap();
        assert!(setup_at(&path, CMD).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
