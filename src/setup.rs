//! `sessions setup`: registers the SessionStart hook in Claude Code's `settings.json`.

use crate::output::say;
use crate::store;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// Every source on which Claude Code starts a session, so resumes and forks are registered too.
const MATCHER: &str = "startup|resume|clear|compact|fork";

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    /// The hook pointed at a binary that is gone (or `--force` was given); it now runs this one.
    Repaired {
        from: String,
    },
    AlreadyPresent,
    /// A working hook runs a different binary. That can be deliberate, so it is left alone.
    OtherInstall {
        command: String,
    },
    /// The plugin ships the same hook; adding it again would run it twice.
    PluginInstalled,
}

/// What `add_hook` needs to know about the machine.
pub struct Options<'a> {
    pub home: &'a Path,
    /// `$PATH`, for hook commands that name the program without a directory.
    pub path: &'a OsStr,
    /// Point the hook at this binary even if another working one is registered.
    pub force: bool,
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(settings_path_in(&store::claude_dir()?))
}

/// Claude Code's `settings.json` in the config directory `dir`.
pub fn settings_path_in(dir: &Path) -> PathBuf {
    dir.join("settings.json")
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

/// Whether a command is a `sessions hook` invocation, however it was installed.
fn is_sessions_hook(command: &str) -> bool {
    let command = command.trim();
    command.ends_with(" hook") && command.contains("sessions")
}

/// The SessionStart commands that run `sessions hook`.
pub fn hook_commands(settings: &Value) -> Vec<String> {
    settings["hooks"]["SessionStart"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str())
        .filter(|cmd| is_sessions_hook(cmd))
        .map(|cmd| cmd.trim().to_string())
        .collect()
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

/// The program a hook command runs, with a leading `~` or `$HOME` expanded.
pub fn hook_executable(command: &str, home: &Path) -> Option<PathBuf> {
    let command = command.trim();
    let first = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => command.split_whitespace().next()?,
    };
    let expand = |prefix: &str| first.strip_prefix(prefix).map(|rest| home.join(rest));
    Some(
        expand("~/")
            .or_else(|| expand("$HOME/"))
            .or_else(|| expand("${HOME}/"))
            .unwrap_or_else(|| PathBuf::from(first)),
    )
}

pub fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn find_in_path(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .map(|dir| dir.join(name))
        .find(|p| is_executable_file(p))
}

/// The executable a hook command would start, if it exists. A bare name is looked up on `PATH`
/// the way the shell would.
pub fn resolve_program(command: &str, home: &Path, path_var: &OsStr) -> Option<PathBuf> {
    let program = hook_executable(command, home)?;
    if program.components().count() == 1 {
        return find_in_path(&program.to_string_lossy(), path_var);
    }
    is_executable_file(&program).then_some(program)
}

fn push_hook(settings: &mut Value, command: &str) -> Result<()> {
    let root = settings.as_object_mut().expect("checked by the caller");
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
    Ok(())
}

/// Points the first sessions hook at `command` and drops the others, which would only run it
/// twice. The entry keeps its matcher and timeout, and so does everything that isn't ours.
fn repoint_hooks(settings: &mut Value, command: &str) {
    let Some(entries) = settings["hooks"]["SessionStart"].as_array_mut() else {
        return;
    };
    let mut seen_first = false;
    let mut emptied = Vec::new();
    for (i, entry) in entries.iter_mut().enumerate() {
        let Some(hooks) = entry["hooks"].as_array_mut() else {
            continue;
        };
        let before = hooks.len();
        hooks.retain_mut(|hook| {
            if !hook["command"].as_str().is_some_and(is_sessions_hook) {
                return true;
            }
            if seen_first {
                return false;
            }
            seen_first = true;
            hook["command"] = json!(command);
            true
        });
        if before > 0 && hooks.is_empty() {
            emptied.push(i);
        }
    }
    for i in emptied.into_iter().rev() {
        entries.remove(i);
    }
}

/// Makes sure `settings` has a working sessions hook that runs `command`. Only `Added` and
/// `Repaired` change it.
pub fn add_hook(settings: &mut Value, command: &str, opts: &Options) -> Result<Outcome> {
    if !settings.is_object() {
        bail!("settings.json is not a JSON object");
    }
    if plugin_enabled(settings) {
        return Ok(Outcome::PluginInstalled);
    }
    let existing = hook_commands(settings);
    if existing.is_empty() {
        push_hook(settings, command)?;
        return Ok(Outcome::Added);
    }

    let this = resolve_program(command, opts.home, opts.path).map(|p| canonical(&p));
    let working: Vec<&String> = existing
        .iter()
        .filter(|c| resolve_program(c, opts.home, opts.path).is_some())
        .collect();
    if !working.is_empty() && !opts.force {
        let runs_this = working.iter().any(|c| {
            resolve_program(c, opts.home, opts.path).map(|p| canonical(&p)) == this
                && this.is_some()
        });
        return Ok(if runs_this {
            Outcome::AlreadyPresent
        } else {
            Outcome::OtherInstall {
                command: working[0].clone(),
            }
        });
    }

    let from = existing[0].clone();
    repoint_hooks(settings, command);
    Ok(Outcome::Repaired { from })
}

/// Reads `path`, fixes the hook and writes it back. The old file is kept as `<name>.bak-<ts>`, and
/// a file that doesn't parse is never touched.
pub fn setup_at(path: &Path, command: &str, opts: &Options) -> Result<Outcome> {
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

    let outcome = add_hook(&mut settings, command, opts)?;
    if matches!(outcome, Outcome::Added | Outcome::Repaired { .. }) {
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

pub fn run(force: bool) -> Result<()> {
    let path = settings_path()?;
    let command = hook_command(&std::env::current_exe()?);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let opts = Options {
        home: &home,
        path: &path_var,
        force,
    };
    match setup_at(&path, &command, &opts)? {
        Outcome::Added => {
            say!("Added the SessionStart hook to {}", path.display());
            say!("It takes effect in new Claude Code sessions.");
            say!("Run `sessions import` to add sessions from before the hook existed.");
        }
        Outcome::Repaired { from } => {
            say!("Updated the hook in {}:", path.display());
            say!("  {from}");
            say!("  -> {command}");
            say!("It takes effect in new Claude Code sessions.");
        }
        Outcome::AlreadyPresent => {
            say!("The hook is already configured in {}", path.display());
        }
        Outcome::OtherInstall { command: other } => {
            say!("The hook runs another sessions binary, which works: {other}");
            say!("It was left alone. To use this binary instead, run `sessions setup --force`.");
        }
        Outcome::PluginInstalled => {
            say!("The sessions plugin is enabled and ships the hook, so nothing was changed.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const HOME: &str = "/home/u";

    fn tmpdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sessions-setup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file that counts as an installed binary.
    fn fake_binary(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::File::create(&path)
            .unwrap()
            .write_all(b"#!/bin/sh\n")
            .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn opts(force: bool) -> Options<'static> {
        Options {
            home: Path::new(HOME),
            path: OsStr::new(""),
            force,
        }
    }

    fn settings_with(command: &str) -> Value {
        json!({"hooks": {"SessionStart": [
            {"matcher": "startup", "hooks": [{"type": "command", "command": command, "timeout": 7}]}
        ]}})
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
        let out = add_hook(&mut s, "/a/sessions hook", &opts(false)).unwrap();
        assert_eq!(out, Outcome::Added);
        assert_eq!(commands(&s), ["/a/sessions hook"]);
        assert_eq!(s["hooks"]["SessionStart"][0]["matcher"], MATCHER);
    }

    #[test]
    fn keeps_other_settings_hooks_and_key_order_when_adding() {
        let mut s: Value = serde_json::from_str(
            r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[]}],
                "SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"other.sh"}]}]},
                "theme":"dark"}"#,
        )
        .unwrap();
        let out = add_hook(&mut s, "/a/sessions hook", &opts(false)).unwrap();
        assert_eq!(out, Outcome::Added);
        assert_eq!(commands(&s), ["other.sh", "/a/sessions hook"]);
        assert!(s["hooks"]["PreToolUse"].is_array());
        let keys: Vec<_> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "theme"]);
    }

    #[test]
    fn a_working_hook_for_this_binary_is_left_alone_and_a_second_run_changes_nothing() {
        let dir = tmpdir("same");
        let exe = fake_binary(&dir, "sessions");
        let command = hook_command(&exe);
        let mut s = json!({});
        assert_eq!(
            add_hook(&mut s, &command, &opts(false)).unwrap(),
            Outcome::Added
        );
        let after_first = s.clone();
        assert_eq!(
            add_hook(&mut s, &command, &opts(false)).unwrap(),
            Outcome::AlreadyPresent
        );
        assert_eq!(s, after_first);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recognises_a_hand_written_home_relative_hook() {
        let dir = tmpdir("home");
        fs::create_dir_all(dir.join(".cargo/bin")).unwrap();
        let exe = fake_binary(&dir.join(".cargo/bin"), "sessions");
        let mut s = settings_with("$HOME/.cargo/bin/sessions hook");
        let o = Options {
            home: &dir,
            path: OsStr::new(""),
            force: false,
        };
        assert_eq!(
            add_hook(&mut s, &hook_command(&exe), &o).unwrap(),
            Outcome::AlreadyPresent
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repairs_a_hook_whose_binary_is_gone_and_keeps_matcher_and_timeout() {
        let dir = tmpdir("stale");
        let command = hook_command(&fake_binary(&dir, "sessions"));
        let mut s = settings_with("/gone/bin/sessions hook");
        let out = add_hook(&mut s, &command, &opts(false)).unwrap();
        assert_eq!(
            out,
            Outcome::Repaired {
                from: "/gone/bin/sessions hook".into()
            }
        );
        assert_eq!(commands(&s), [command.as_str()]);
        let hook = &s["hooks"]["SessionStart"][0];
        assert_eq!(hook["matcher"], "startup", "matcher kept");
        assert_eq!(hook["hooks"][0]["timeout"], 7, "timeout kept");
        // And a second run is then a no-op.
        assert_eq!(
            add_hook(&mut s, &command, &opts(false)).unwrap(),
            Outcome::AlreadyPresent
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn several_stale_hooks_collapse_into_one_and_unrelated_hooks_stay() {
        let dir = tmpdir("many");
        let command = hook_command(&fake_binary(&dir, "sessions"));
        let mut s = json!({"hooks": {"SessionStart": [
            {"matcher": "a", "hooks": [{"command": "/gone/one/sessions hook"}, {"command": "keep.sh"}]},
            {"matcher": "b", "hooks": [{"command": "/gone/two/sessions hook"}]},
            {"matcher": "c", "hooks": []}
        ]}});
        let out = add_hook(&mut s, &command, &opts(false)).unwrap();
        assert!(matches!(out, Outcome::Repaired { .. }));
        assert_eq!(commands(&s), [command.as_str(), "keep.sh"]);
        let entries = s["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(
            entries.len(),
            2,
            "the entry we emptied is gone, the pre-existing empty one is not"
        );
        assert_eq!(entries[1]["matcher"], "c");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_working_hook_for_another_binary_is_left_alone_unless_forced() {
        let dir = tmpdir("other");
        let registered = fake_binary(&dir, "registered-sessions");
        let running = fake_binary(&dir, "running-sessions");
        let command = hook_command(&running);
        let mut s = settings_with(&hook_command(&registered));

        let out = add_hook(&mut s, &command, &opts(false)).unwrap();
        assert_eq!(
            out,
            Outcome::OtherInstall {
                command: hook_command(&registered)
            }
        );
        assert_eq!(
            commands(&s),
            [hook_command(&registered).as_str()],
            "unchanged"
        );

        let forced = add_hook(&mut s, &command, &opts(true)).unwrap();
        assert!(matches!(forced, Outcome::Repaired { .. }));
        assert_eq!(commands(&s), [command.as_str()]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bare_command_is_looked_up_on_path_before_calling_it_stale() {
        let dir = tmpdir("bare");
        let exe = fake_binary(&dir, "sessions");
        let command = hook_command(&exe);
        let with_path = Options {
            home: Path::new(HOME),
            path: dir.as_os_str(),
            force: false,
        };
        let mut s = settings_with("sessions hook");
        assert_eq!(
            add_hook(&mut s, &command, &with_path).unwrap(),
            Outcome::AlreadyPresent
        );
        // Without that directory on PATH the same command resolves to nothing, so it is stale.
        let mut s = settings_with("sessions hook");
        let out = add_hook(&mut s, &command, &opts(false)).unwrap();
        assert!(matches!(out, Outcome::Repaired { .. }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn does_not_double_up_with_the_plugin() {
        let mut s = json!({"enabledPlugins":{"sessions@session-tui":true}});
        let before = s.clone();
        assert_eq!(
            add_hook(&mut s, "/a/sessions hook", &opts(false)).unwrap(),
            Outcome::PluginInstalled
        );
        assert_eq!(s, before);
        // A disabled plugin doesn't run the hook, so it still needs adding.
        let mut off = json!({"enabledPlugins":{"sessions@session-tui":false}});
        assert_eq!(
            add_hook(&mut off, "/a/sessions hook", &opts(false)).unwrap(),
            Outcome::Added
        );
    }

    #[test]
    fn rejects_settings_of_the_wrong_shape() {
        let c = "/a/sessions hook";
        assert!(add_hook(&mut json!([]), c, &opts(false)).is_err());
        assert!(add_hook(&mut json!({"hooks": []}), c, &opts(false)).is_err());
        assert!(add_hook(&mut json!({"hooks": {"SessionStart": {}}}), c, &opts(false)).is_err());
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
    fn hook_executable_reads_quoted_bare_and_home_relative_commands() {
        let home = Path::new(HOME);
        let exe = |c: &str| hook_executable(c, home);
        assert_eq!(
            exe("/a/b/sessions hook"),
            Some(PathBuf::from("/a/b/sessions"))
        );
        assert_eq!(
            exe("\"/My Apps/sessions\" hook"),
            Some(PathBuf::from("/My Apps/sessions"))
        );
        assert_eq!(
            exe("$HOME/.cargo/bin/sessions hook"),
            Some(PathBuf::from("/home/u/.cargo/bin/sessions"))
        );
        assert_eq!(
            exe("${HOME}/bin/sessions hook"),
            Some(PathBuf::from("/home/u/bin/sessions"))
        );
        assert_eq!(
            exe("~/bin/sessions hook"),
            Some(PathBuf::from("/home/u/bin/sessions"))
        );
        assert_eq!(exe("sessions hook"), Some(PathBuf::from("sessions")));
        assert_eq!(exe("   "), None);
    }

    #[test]
    fn creates_a_missing_file_without_a_backup() {
        let dir = tmpdir("create");
        let path = dir.join("settings.json");
        assert_eq!(
            setup_at(&path, "/a/sessions hook", &opts(false)).unwrap(),
            Outcome::Added
        );
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(commands(&written), ["/a/sessions hook"]);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "only settings.json, no backup"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn backs_up_before_adding_or_repairing_but_not_when_nothing_changes() {
        let dir = tmpdir("backup");
        let path = dir.join("settings.json");
        let command = hook_command(&fake_binary(&dir, "sessions"));
        let backups = || {
            fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
                .count()
        };

        let original = "{\n  \"model\": \"opus\"\n}\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            setup_at(&path, &command, &opts(false)).unwrap(),
            Outcome::Added
        );
        assert_eq!(backups(), 1);
        let backup = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().contains(".bak-"))
            .unwrap()
            .path();
        assert_eq!(fs::read_to_string(backup).unwrap(), original);

        assert_eq!(
            setup_at(&path, &command, &opts(false)).unwrap(),
            Outcome::AlreadyPresent
        );
        assert_eq!(backups(), 1, "no further backup");

        // A stale hook is repaired on disk too, after another backup.
        fs::write(
            &path,
            serde_json::to_string(&settings_with("/gone/sessions hook")).unwrap(),
        )
        .unwrap();
        let out = setup_at(&path, &command, &opts(false)).unwrap();
        assert!(matches!(out, Outcome::Repaired { .. }));
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(commands(&written), [command.as_str()]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn leaves_a_file_that_is_not_json_untouched() {
        let dir = tmpdir("invalid");
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(setup_at(&path, "/a/sessions hook", &opts(false)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(dir);
    }
}
