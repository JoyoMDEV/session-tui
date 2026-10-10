//! `sessions doctor`: checks that everything the tool relies on is in place.

use crate::output::text;
use crate::setup::{self, canonical, find_in_path};
use crate::{store, transcript};
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

/// How many of the newest transcripts are read to see whether the format still parses.
const SAMPLE_SIZE: usize = 20;
/// Smaller files are sessions that were opened and closed again; they have no prompt to find.
const MIN_SAMPLE_BYTES: u64 = 4096;
/// What Claude Code does when `cleanupPeriodDays` is not set (per its documentation).
const DEFAULT_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug)]
pub struct Check {
    pub level: Level,
    pub name: &'static str,
    pub detail: String,
    pub hint: Option<String>,
}

fn check(level: Level, name: &'static str, detail: impl Into<String>) -> Check {
    Check {
        level,
        name,
        detail: detail.into(),
        hint: None,
    }
}

fn ok(name: &'static str, detail: impl Into<String>) -> Check {
    check(Level::Ok, name, detail)
}

fn warn(name: &'static str, detail: impl Into<String>) -> Check {
    check(Level::Warn, name, detail)
}

fn fail(name: &'static str, detail: impl Into<String>) -> Check {
    check(Level::Fail, name, detail)
}

impl Check {
    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Claude Code's `settings.json` as far as `doctor` can tell.
pub enum Settings {
    Missing,
    Invalid(String),
    Loaded(Value),
}

fn load_settings(path: &Path) -> Settings {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(value) => Settings::Loaded(value),
            Err(e) => Settings::Invalid(e.to_string()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::Missing,
        Err(e) => Settings::Invalid(e.to_string()),
    }
}

fn dir_on_path(dir: &Path, path_var: &OsStr) -> bool {
    let want = canonical(dir);
    std::env::split_paths(path_var).any(|d| canonical(&d) == want)
}

/// Days Claude Code keeps transcripts, and whether that is just its default.
pub fn retention_days(settings: &Settings) -> (u64, bool) {
    match settings {
        Settings::Loaded(v) => match v["cleanupPeriodDays"].as_u64() {
            Some(days) => (days, false),
            None => (DEFAULT_RETENTION_DAYS, true),
        },
        _ => (DEFAULT_RETENTION_DAYS, true),
    }
}

/// `1 entry`, `2 entries`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn binary_checks(exe: &Path, path_var: &OsStr) -> Vec<Check> {
    let mut out = vec![ok(
        "binary",
        format!("{} ({})", exe.display(), env!("CARGO_PKG_VERSION")),
    )];
    if let Some(dir) = exe.parent() {
        out.push(if dir_on_path(dir, path_var) {
            ok("path", format!("{} is on your PATH", dir.display()))
        } else {
            warn("path", format!("{} is not on your PATH", dir.display())).hint(format!(
                "add to your shell profile: export PATH=\"{}:$PATH\"",
                dir.display()
            ))
        });
    }
    out.push(match find_in_path("claude", path_var) {
        Some(p) => ok("claude", p.display().to_string()),
        None => warn(
            "claude",
            "not found on your PATH, so Enter can't resume a session",
        )
        .hint("install Claude Code, or fix PATH for this shell"),
    });
    out
}

/// The fixed system directories the plugin's hook looks in after the ones under the home
/// directory, as written in `hooks/hooks.json`.
const SYSTEM_BIN_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// The directories the plugin's hook looks in: under `home`, then `system_dirs`, then `path_var`.
/// The caller passes the system directories so that a test does not find a real installation.
fn plugin_search_path(home: &Path, system_dirs: &[&str], path_var: &OsStr) -> std::ffi::OsString {
    let mut dirs = vec![home.join(".local/bin"), home.join(".cargo/bin")];
    dirs.extend(system_dirs.iter().map(PathBuf::from));
    dirs.extend(std::env::split_paths(path_var));
    std::env::join_paths(dirs).unwrap_or_default()
}

fn hook_check(
    settings: &Settings,
    settings_path: &Path,
    current_exe: &Path,
    home: &Path,
    system_dirs: &[&str],
    path_var: &OsStr,
) -> Check {
    let shown = settings_path.display();
    let setup_hint = "run `sessions setup`, or install the plugin";
    let Settings::Loaded(value) = settings else {
        return match settings {
            Settings::Invalid(e) => fail("hook", format!("{shown} is not valid JSON: {e}"))
                .hint("fix the file by hand; `sessions setup` won't touch it"),
            _ => fail(
                "hook",
                format!("{shown} does not exist, so no hook is registered"),
            )
            .hint(setup_hint),
        };
    };

    if let Some(command) = setup::hook_commands(value).first() {
        let Some(declared) = setup::hook_executable(command, home) else {
            return fail("hook", format!("cannot read the hook command: {command}"))
                .hint(setup_hint);
        };
        let Some(program) = setup::resolve_program(command, home, path_var) else {
            return fail(
                "hook",
                format!("the hook runs {}, which does not exist", declared.display()),
            )
            .hint("run `sessions setup`; it points the hook at this binary");
        };
        if canonical(&program) != canonical(current_exe) {
            return warn(
                "hook",
                format!(
                    "the hook runs {}, but this is {}",
                    program.display(),
                    current_exe.display()
                ),
            )
            .hint("two installs can differ in version; keep one, or run `sessions setup --force` to point the hook at this one");
        }
        return ok("hook", format!("registered in {shown}"));
    }

    if setup::plugin_enabled(value) {
        return match find_in_path("sessions", &plugin_search_path(home, system_dirs, path_var)) {
            Some(p) => ok(
                "hook",
                format!(
                    "provided by the sessions plugin, which finds {}",
                    p.display()
                ),
            ),
            None => fail(
                "hook",
                "the plugin is enabled, but no sessions binary is where it looks",
            )
            .hint("install to ~/.local/bin (install script), ~/.cargo/bin or Homebrew"),
        };
    }

    fail(
        "hook",
        format!("{shown} has no SessionStart hook for sessions"),
    )
    .hint(setup_hint)
}

fn sessions_checks(existing: &HashSet<String>) -> Vec<Check> {
    let path = match store::path() {
        Ok(path) => path,
        Err(e) => return vec![fail("sessions", format!("{e:#}"))],
    };
    if !path.exists() {
        return vec![ok(
            "sessions",
            format!(
                "{} (not created yet; the first session creates it)",
                path.display()
            ),
        )];
    }
    let mut out = Vec::new();
    match store::load() {
        Err(e) => out.push(fail("sessions", format!("{e:#}"))),
        Ok(all) => {
            out.push(ok(
                "sessions",
                format!(
                    "{} ({})",
                    path.display(),
                    count(all.len(), "entry", "entries")
                ),
            ));
            let orphans = all
                .iter()
                .filter(|s| transcript::is_orphan(s, existing))
                .count();
            if orphans > 0 {
                out.push(
                    warn(
                        "orphans",
                        format!(
                            "{} no transcript and can't be resumed",
                            if orphans == 1 {
                                "1 entry has".to_string()
                            } else {
                                format!("{orphans} entries have")
                            }
                        ),
                    )
                    .hint("`sessions prune` removes them"),
                );
            }
        }
    }
    let backups = path
        .parent()
        .and_then(|dir| fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .count();
    if backups > 0 {
        out.push(
            warn(
                "backups",
                format!(
                    "{} of an unreadable sessions file",
                    count(backups, "moved-aside copy", "moved-aside copies")
                ),
            )
                .hint("sessions.json.corrupt-* next to it; delete them once you've checked nothing is missing"),
        );
    }
    out
}

/// What one transcript from the sample showed.
pub struct Sample {
    pub has_prompt: bool,
    pub has_title: bool,
}

fn transcript_checks(total: usize, samples: &[Sample], projects: &Path) -> Vec<Check> {
    if total == 0 {
        return vec![
            warn("transcripts", format!("none under {}", projects.display()))
                .hint("if Claude Code keeps them elsewhere, set CLAUDE_CONFIG_DIR"),
        ];
    }
    let mut out = vec![ok(
        "transcripts",
        format!("{total} under {}", projects.display()),
    )];
    if samples.is_empty() {
        return out;
    }
    let n = samples.len();
    let readable = samples.iter().filter(|s| s.has_prompt).count();
    let changed =
        "Claude Code's transcript format may have changed; update sessions, or open an issue";
    out.push(if readable == 0 {
        fail(
            "format",
            format!("could not read a prompt from any of the {n} newest transcripts"),
        )
        .hint(changed)
    } else if readable * 2 < n {
        warn(
            "format",
            format!("read a prompt from only {readable} of the {n} newest transcripts"),
        )
        .hint(changed)
    } else {
        ok(
            "format",
            format!("{readable} of the {n} newest transcripts parse"),
        )
    });
    if n >= 5 && samples.iter().all(|s| !s.has_title) {
        out.push(
            warn(
                "titles",
                "no Claude-generated titles in the newest transcripts",
            )
            .hint(
                "titles fall back to the first prompt; if this is new, the format may have changed",
            ),
        );
    }
    out
}

fn sample_transcripts() -> (usize, Vec<Sample>) {
    let all = transcript::all();
    let total = all.len();
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for path in all {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.len() >= MIN_SAMPLE_BYTES
            && let Ok(modified) = meta.modified()
        {
            files.push((modified, path));
        }
    }
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let samples = files
        .into_iter()
        .take(SAMPLE_SIZE)
        .map(|(_, path)| Sample {
            has_prompt: transcript::read_info(&path).is_ok_and(|i| i.first_prompt.is_some()),
            has_title: transcript::scan_meta(&path).is_ok_and(|m| m.native_title.is_some()),
        })
        .collect();
    (total, samples)
}

fn retention_checks(settings: &Settings) -> Vec<Check> {
    let (days, is_default) = retention_days(settings);
    let origin = if is_default { " (its default)" } else { "" };
    vec![ok(
        "retention",
        format!("Claude Code keeps transcripts for {days} days{origin}"),
    )]
}

pub fn gather() -> Vec<Check> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sessions"));
    let mut checks = binary_checks(&exe, &path_var);
    // Everything else lives in Claude Code's config directory, which may not be findable.
    let claude_dir = match store::claude_dir() {
        Ok(dir) => dir,
        Err(e) => {
            checks.push(
                fail("config", format!("{e:#}"))
                    .hint("set HOME, or CLAUDE_CONFIG_DIR to Claude Code's config directory"),
            );
            return checks;
        }
    };
    let settings_path = setup::settings_path_in(&claude_dir);
    let settings = load_settings(&settings_path);
    checks.push(hook_check(
        &settings,
        &settings_path,
        &exe,
        &home,
        &SYSTEM_BIN_DIRS,
        &path_var,
    ));
    checks.extend(sessions_checks(&transcript::existing_ids()));
    let (total, samples) = sample_transcripts();
    checks.extend(transcript_checks(
        total,
        &samples,
        &claude_dir.join("projects"),
    ));
    checks.extend(retention_checks(&settings));
    checks
}

pub fn render(checks: &[Check]) -> String {
    let mut out = String::new();
    for c in checks {
        let mark = match c.level {
            Level::Ok => "✓",
            Level::Warn => "!",
            Level::Fail => "✗",
        };
        out += &format!("{mark} {:<12} {}\n", c.name, c.detail);
        if let Some(hint) = &c.hint {
            out += &format!("  {:<12} → {hint}\n", "");
        }
    }
    let fails = checks.iter().filter(|c| c.level == Level::Fail).count();
    let warns = checks.iter().filter(|c| c.level == Level::Warn).count();
    out += &match (fails, warns) {
        (0, 0) => "\nEverything looks fine.\n".to_string(),
        (0, w) => format!(
            "\nNo problems, {w} warning{}.\n",
            if w == 1 { "" } else { "s" }
        ),
        (f, w) => format!(
            "\n{f} problem{}, {w} warning{}.\n",
            if f == 1 { "" } else { "s" },
            if w == 1 { "" } else { "s" }
        ),
    };
    out
}

/// Prints the report. `false` if a check failed, which `sessions doctor` reports as exit status 1.
pub fn run() -> Result<bool> {
    let checks = gather();
    text(&render(&checks));
    Ok(!checks.iter().any(|c| c.level == Level::Fail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn tmpdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sessions-doctor-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_binary(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::File::create(&path)
            .unwrap()
            .write_all(b"#!/bin/sh\n")
            .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn settings_with(command: &str) -> Settings {
        Settings::Loaded(
            json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":command}]}]}}),
        )
    }

    const HOME: &str = "/home/u";

    #[test]
    fn dir_on_path_matches_through_symlinks_and_ignores_others() {
        let dir = tmpdir("path");
        let real = dir.join("real");
        fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("link")).unwrap();
        let path_var = std::env::join_paths([dir.join("link"), PathBuf::from("/usr/bin")]).unwrap();
        assert!(dir_on_path(&real, &path_var));
        assert!(!dir_on_path(&dir.join("other"), &path_var));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_reads_the_setting_and_falls_back_to_the_default() {
        assert_eq!(
            retention_days(&Settings::Loaded(json!({"cleanupPeriodDays": 90}))),
            (90, false)
        );
        assert_eq!(retention_days(&Settings::Loaded(json!({}))), (30, true));
        assert_eq!(retention_days(&Settings::Missing), (30, true));
    }

    #[test]
    fn hook_check_passes_when_the_hook_runs_this_binary() {
        let dir = tmpdir("hook-ok");
        let exe = fake_binary(&dir, "sessions");
        let s = settings_with(&format!("{} hook", exe.display()));
        let c = hook_check(
            &s,
            Path::new("/s.json"),
            &exe,
            Path::new(HOME),
            &[],
            OsStr::new(""),
        );
        assert_eq!(c.level, Level::Ok, "{}", c.detail);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hook_check_fails_when_the_hook_points_at_a_binary_that_is_gone() {
        let dir = tmpdir("hook-stale");
        let exe = fake_binary(&dir, "sessions");
        let s = settings_with("/moved/away/sessions hook");
        let c = hook_check(
            &s,
            Path::new("/s.json"),
            &exe,
            Path::new(HOME),
            &[],
            OsStr::new(""),
        );
        assert_eq!(c.level, Level::Fail);
        assert!(c.detail.contains("/moved/away/sessions"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hook_check_warns_when_another_install_is_registered() {
        let dir = tmpdir("hook-other");
        let registered = fake_binary(&dir, "registered");
        let running = fake_binary(&dir, "running");
        let s = settings_with(&format!("{} hook", registered.display()));
        let c = hook_check(
            &s,
            Path::new("/s.json"),
            &running,
            Path::new(HOME),
            &[],
            OsStr::new(""),
        );
        assert_eq!(c.level, Level::Warn);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hook_check_fails_on_missing_invalid_or_hookless_settings() {
        let exe = Path::new("/x/sessions");
        let level = |s: &Settings| {
            hook_check(
                s,
                Path::new("/s.json"),
                exe,
                Path::new(HOME),
                &[],
                OsStr::new(""),
            )
            .level
        };
        assert_eq!(level(&Settings::Missing), Level::Fail);
        assert_eq!(level(&Settings::Invalid("eof".into())), Level::Fail);
        assert_eq!(level(&Settings::Loaded(json!({"model": "x"}))), Level::Fail);
    }

    #[test]
    fn hook_check_accepts_the_plugin_only_if_the_binary_is_where_it_looks() {
        let dir = tmpdir("plugin");
        let home = dir.join("home");
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        let settings = Settings::Loaded(json!({"enabledPlugins": {"sessions@session-tui": true}}));
        let run = |home: &Path| {
            hook_check(
                &settings,
                Path::new("/s.json"),
                Path::new("/x/sessions"),
                home,
                &[],
                OsStr::new(""),
            )
        };

        assert_eq!(run(&home).level, Level::Fail, "binary missing");
        fake_binary(&home.join(".local/bin"), "sessions");
        assert_eq!(run(&home).level, Level::Ok, "binary in ~/.local/bin");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_system_directories_are_the_ones_the_plugin_hook_searches() {
        let hooks = include_str!("../hooks/hooks.json");
        for dir in SYSTEM_BIN_DIRS {
            assert!(hooks.contains(dir), "{dir} is not in hooks/hooks.json");
        }
    }

    fn samples(prompts: usize, titled: usize, total: usize) -> Vec<Sample> {
        (0..total)
            .map(|i| Sample {
                has_prompt: i < prompts,
                has_title: i < titled,
            })
            .collect()
    }

    fn worst(checks: &[Check]) -> Level {
        checks
            .iter()
            .map(|c| c.level)
            .fold(Level::Ok, |a, b| match (a, b) {
                (Level::Fail, _) | (_, Level::Fail) => Level::Fail,
                (Level::Warn, _) | (_, Level::Warn) => Level::Warn,
                _ => Level::Ok,
            })
    }

    #[test]
    fn transcript_checks_tell_a_changed_format_from_a_healthy_one() {
        let dir = Path::new("/p");
        assert_eq!(
            worst(&transcript_checks(0, &[], dir)),
            Level::Warn,
            "no transcripts"
        );
        assert_eq!(
            worst(&transcript_checks(20, &samples(20, 20, 20), dir)),
            Level::Ok
        );
        assert_eq!(
            worst(&transcript_checks(20, &samples(4, 20, 20), dir)),
            Level::Warn,
            "mostly unreadable"
        );
        assert_eq!(
            worst(&transcript_checks(20, &samples(0, 0, 20), dir)),
            Level::Fail,
            "nothing readable"
        );
        // Only a handful of small files and no titles is too little to call a format change.
        assert_eq!(
            worst(&transcript_checks(3, &samples(3, 0, 3), dir)),
            Level::Ok
        );
        let untitled = transcript_checks(20, &samples(20, 0, 20), dir);
        assert!(
            untitled
                .iter()
                .any(|c| c.name == "titles" && c.level == Level::Warn)
        );
    }

    #[test]
    fn render_marks_each_level_and_summarises() {
        let checks = vec![
            ok("binary", "/x"),
            warn("path", "not on PATH").hint("fix it"),
            fail("hook", "missing"),
        ];
        let text = render(&checks);
        assert!(text.contains("✓ binary"));
        assert!(text.contains("! path"));
        assert!(text.contains("→ fix it"));
        assert!(text.contains("✗ hook"));
        assert!(text.contains("1 problem, 1 warning."));
        assert!(render(&[ok("a", "b")]).contains("Everything looks fine."));
    }
}
