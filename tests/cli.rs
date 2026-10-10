//! Runs the real binary against temporary directories and checks what a user or Claude Code sees:
//! output, exit status and the files it writes. Nothing here touches the real `~/.claude`.

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// A scratch `$HOME`, Claude config dir and sessions file, removed on drop.
struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("sessions-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("claude")).unwrap();
        Env { dir }
    }

    fn claude_dir(&self) -> PathBuf {
        self.dir.join("claude")
    }

    fn sessions_file(&self) -> PathBuf {
        self.dir.join("sessions.json")
    }

    fn settings_file(&self) -> PathBuf {
        self.claude_dir().join("settings.json")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sessions"));
        cmd.args(args)
            .env("HOME", &self.dir)
            .env("CLAUDE_CONFIG_DIR", self.claude_dir())
            .env("SESSIONS_FILE", self.sessions_file())
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_ENV_FILE")
            .env_remove("SESSIONS_RESUME_REMINDER")
            .env_remove("SESSIONS_PRESET")
            .env_remove("SESSIONS_CLAUDE_ARGS");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs the hook with `payload` on stdin.
    fn hook(&self, payload: Value) -> Output {
        self.hook_with_preset(payload, None)
    }

    /// Runs the hook with `payload` on stdin and `preset` as `$SESSIONS_PRESET`.
    fn hook_with_preset(&self, payload: Value, preset: Option<&str>) -> Output {
        let mut cmd = self.command(&["hook"]);
        if let Some(preset) = preset {
            cmd.env("SESSIONS_PRESET", preset);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn sessions(&self) -> Vec<Value> {
        let text = fs::read_to_string(self.sessions_file()).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn session(&self, id: &str) -> Value {
        self.sessions()
            .into_iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("no session {id}"))
    }

    /// Writes a sessions file by hand, for states the commands can no longer produce.
    fn write_sessions(&self, sessions: Value) {
        fs::write(self.sessions_file(), sessions.to_string()).unwrap();
    }

    /// Registers a session the way the hook does.
    fn register(&self, id: &str, cwd: &str) {
        let out = self.hook(json!({"session_id": id, "cwd": cwd, "source": "startup"}));
        assert!(out.status.success(), "{}", stderr(&out));
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `hookSpecificOutput` object of a successful hook run; stdout must be exactly one JSON value.
fn hook_output(out: &Output) -> Value {
    assert!(out.status.success(), "{}", stderr(out));
    let value: Value = serde_json::from_str(&stdout(out)).expect("stdout is one JSON object");
    value["hookSpecificOutput"].clone()
}

#[test]
fn hook_registers_the_session_and_tells_the_agent_its_id() {
    let env = Env::new("hook-register");
    let out = env.hook(json!({"session_id": "s1", "cwd": "/work/a", "source": "startup"}));
    let hook = hook_output(&out);
    assert_eq!(hook["hookEventName"], "SessionStart");
    let context = hook["additionalContext"].as_str().unwrap();
    assert!(context.contains("Session ID: s1"));
    assert!(context.contains("title --id s1"));
    assert!(context.contains("ticket --id s1"));
    assert!(context.contains("tag --id s1"));
    assert!(!context.contains("being resumed"));
    assert_eq!(env.session("s1")["cwd"], "/work/a");
}

#[test]
fn hook_exports_the_session_id_and_binary_dir_to_the_env_file() {
    let env = Env::new("hook-env-file");
    let env_file = env.dir.join("env");
    let mut cmd = env.command(&["hook"]);
    let mut child = cmd
        .env("CLAUDE_ENV_FILE", &env_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"s1","cwd":"/w","source":"startup"}"#)
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    let written = fs::read_to_string(env_file).unwrap();
    assert!(written.contains("export CLAUDE_SESSION_ID=s1"));
    assert!(written.contains("export PATH="));
}

#[test]
fn hook_passes_our_title_on_unless_the_session_already_has_a_name() {
    let env = Env::new("hook-title");
    env.register("s1", "/w");
    assert!(
        env.run(&["title", "--id", "s1", "Fix login"])
            .status
            .success()
    );

    let resumed =
        hook_output(&env.hook(json!({"session_id": "s1", "cwd": "/w", "source": "resume"})));
    assert_eq!(resumed["sessionTitle"], "Fix login");
    assert!(
        resumed["additionalContext"]
            .as_str()
            .unwrap()
            .contains("being resumed")
    );

    let named = hook_output(&env.hook(json!({
        "session_id": "s1", "cwd": "/w", "source": "resume", "session_title": "Mine"
    })));
    assert!(named.get("sessionTitle").is_none());

    // Claude Code ignores the title on clear and compact, so none is sent.
    let cleared =
        hook_output(&env.hook(json!({"session_id": "s1", "cwd": "/w", "source": "clear"})));
    assert!(cleared.get("sessionTitle").is_none());
}

#[test]
fn hook_reminder_on_resume_can_be_turned_off() {
    let env = Env::new("hook-reminder");
    env.register("s1", "/w");
    let mut cmd = env.command(&["hook"]);
    let mut child = cmd
        .env("SESSIONS_RESUME_REMINDER", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"s1","cwd":"/w","source":"resume"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let hook = hook_output(&out);
    assert!(
        !hook["additionalContext"]
            .as_str()
            .unwrap()
            .contains("being resumed")
    );
}

#[test]
fn hook_lists_tags_already_in_use() {
    let env = Env::new("hook-vocabulary");
    env.register("s1", "/w");
    assert!(
        env.run(&["tag", "--id", "s1", "observability"])
            .status
            .success()
    );
    let hook =
        hook_output(&env.hook(json!({"session_id": "s2", "cwd": "/w", "source": "startup"})));
    let context = hook["additionalContext"].as_str().unwrap();
    assert!(context.contains("Tags already in use (sessions): observability (1)"));
}

#[test]
fn hook_rejects_input_that_is_not_json_without_writing_anything() {
    let env = Env::new("hook-bad-input");
    let mut child = env
        .command(&["hook"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("parsing hook input"));
    assert!(!env.sessions_file().exists());
}

#[test]
fn title_tag_ticket_and_note_edit_the_session() {
    let env = Env::new("edit");
    env.register("s1", "/w");
    for args in [
        &["title", "--id", "s1", "  Fix login  "][..],
        &["tag", "--id", "s1", "Observability", "repair"],
        &["ticket", "--id", "s1", "ABC-123"],
        &["note", "--id", "s1", "ask Sam"],
    ] {
        let out = env.run(args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    }
    let s = env.session("s1");
    assert_eq!(s["title"], "Fix login");
    assert_eq!(s["tags"], json!(["observability", "repair"]));
    assert_eq!(s["tickets"], json!(["ABC-123"]));
    assert_eq!(s["note"], "ask Sam");

    assert!(
        env.run(&["tag", "--id", "s1", "--remove", "repair"])
            .status
            .success()
    );
    assert!(
        env.run(&["ticket", "--id", "s1", "--remove", "ABC-123"])
            .status
            .success()
    );
    assert!(env.run(&["note", "--id", "s1", ""]).status.success());
    let s = env.session("s1");
    assert_eq!(s["tags"], json!(["observability"]));
    assert!(s.get("tickets").is_none());
    assert!(s.get("note").is_none());
}

#[test]
fn tag_refuses_a_ticket_key_and_says_where_it_belongs() {
    let env = Env::new("tag-key");
    env.register("s1", "/w");
    let out = env.run(&["tag", "--id", "s1", "auth", "ABC-123"]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("ABC-123 looks like a ticket key"),
        "{message}"
    );
    assert!(
        message.contains("sessions ticket --id s1 ABC-123"),
        "{message}"
    );
    assert!(message.contains("lower case"), "{message}");
    // Nothing is written, not even the valid tag in the same call.
    let s = env.session("s1");
    assert!(s.get("tags").is_none());
    assert!(s.get("tickets").is_none());

    // Only the exact shape is refused, so `utf-8` and lower case spellings are tags.
    assert!(
        env.run(&["tag", "--id", "s1", "utf-8", "abc-123"])
            .status
            .success()
    );
    assert_eq!(env.session("s1")["tags"], json!(["utf-8", "abc-123"]));
}

#[test]
fn edits_default_to_claude_session_id() {
    let env = Env::new("edit-env-id");
    env.register("s1", "/w");
    let out = env
        .command(&["title", "From env"])
        .env("CLAUDE_SESSION_ID", "s1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(env.session("s1")["title"], "From env");
}

#[test]
fn editing_an_unknown_session_is_an_error_and_creates_nothing() {
    let env = Env::new("edit-unknown");
    env.register("s1", "/w");
    for args in [
        &["title", "--id", "typo", "x"][..],
        &["tag", "--id", "typo", "x"],
        &["ticket", "--id", "typo", "ABC-1"],
        &["note", "--id", "typo", "x"],
    ] {
        let out = env.run(args);
        assert!(!out.status.success(), "{args:?} succeeded");
        assert!(stderr(&out).contains("no session typo"), "{}", stderr(&out));
    }
    let ids: Vec<_> = env.sessions().iter().map(|s| s["id"].clone()).collect();
    assert_eq!(ids, [json!("s1")]);
}

#[test]
fn an_empty_title_is_an_error() {
    let env = Env::new("empty-title");
    env.register("s1", "/w");
    let out = env.run(&["title", "--id", "s1", "   "]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("empty title"));
    assert!(env.session("s1").get("title").is_none());
}

#[test]
fn list_prints_one_line_per_session_and_filters_by_ticket() {
    let env = Env::new("list");
    env.register("s1", "/work/a");
    env.register("s2", "/work/b");
    env.run(&["title", "--id", "s1", "Fix login"]);
    env.run(&["ticket", "--id", "s1", "ABC-1", "ABC-2"]);
    env.run(&["tag", "--id", "s1", "auth"]);
    env.run(&["ticket", "--id", "s2", "ABC-1"]);

    let all = stdout(&env.run(&["list"]));
    assert_eq!(all.lines().count(), 2);
    let line = all.lines().find(|l| l.starts_with("s1")).unwrap();
    assert!(line.contains("Fix login"));
    assert!(line.contains("/work/a"));
    assert!(line.contains("[ABC-1 ABC-2]"));
    assert!(line.contains("#auth"));

    let one = stdout(&env.run(&["list", "--ticket", "ABC-1"]));
    assert_eq!(one.lines().count(), 2);
    let both = stdout(&env.run(&["list", "--ticket", "ABC-1", "--ticket", "ABC-2"]));
    assert_eq!(both.lines().count(), 1);
    assert!(both.starts_with("s1"));
}

/// Three sessions with different tags, tickets, branches, directories and ages.
fn filtered_env(name: &str) -> Env {
    let env = Env::new(name);
    let now = chrono::Utc::now();
    let ago = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339();
    env.write_sessions(json!([
        {"id": "new", "cwd": "/work/app/api", "title": "Fix login", "branch": "feat/login",
         "tags": ["auth"], "tickets": ["ABC-1"], "created_at": ago(1), "updated_at": ago(1)},
        {"id": "mid", "cwd": "/work/app", "tags": ["auth", "repair"], "tickets": ["ABC-12"],
         "created_at": ago(10), "updated_at": ago(10)},
        {"id": "old", "cwd": "/work/other", "branch": "main",
         "created_at": ago(60), "updated_at": ago(60)},
    ]));
    env
}

fn listed_ids(env: &Env, args: &[&str]) -> Vec<String> {
    let mut all = vec!["list"];
    all.extend_from_slice(args);
    let out = env.run(&all);
    assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    stdout(&out)
        .lines()
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

#[test]
fn list_filters_combine_and_match_exactly() {
    let env = filtered_env("list-filters");
    assert_eq!(listed_ids(&env, &["--tag", "auth"]), ["new", "mid"]);
    assert_eq!(
        listed_ids(&env, &["--tag", "AUTH", "--tag", "repair"]),
        ["mid"]
    );
    assert!(
        listed_ids(&env, &["--tag", "aut"]).is_empty(),
        "tags are not fuzzy"
    );
    assert_eq!(
        listed_ids(&env, &["--ticket", "ABC-1"]),
        ["new"],
        "not ABC-12"
    );
    assert_eq!(listed_ids(&env, &["--branch", "main"]), ["old"]);
    assert_eq!(listed_ids(&env, &["--cwd", "/work/app"]), ["new", "mid"]);
    assert_eq!(
        listed_ids(&env, &["--cwd", "/work/ap"]),
        Vec::<String>::new()
    );
    assert_eq!(listed_ids(&env, &["--since", "7d"]), ["new"]);
    assert_eq!(listed_ids(&env, &["--since", "2w"]), ["new", "mid"]);
    assert_eq!(
        listed_ids(&env, &["--tag", "auth", "--since", "7d"]),
        ["new"]
    );
}

#[test]
fn list_cwd_takes_a_relative_path_from_the_current_directory() {
    let env = filtered_env("list-cwd-relative");
    let here = env.dir.join("proj");
    fs::create_dir_all(&here).unwrap();
    // The process sees the real path (on macOS /var is a link to /private/var).
    let here = here.canonicalize().unwrap();
    env.write_sessions(json!([
        {"id": "in", "cwd": here.join("sub").display().to_string(),
         "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
        {"id": "out", "cwd": "/elsewhere",
         "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
    ]));
    let out = env
        .command(&["list", "--cwd", "."])
        .current_dir(&here)
        .output()
        .unwrap();
    assert!(stdout(&out).starts_with("in "), "{}", stdout(&out));
    assert_eq!(stdout(&out).lines().count(), 1);
}

#[test]
fn list_cwd_resolves_dot_dot_from_the_current_directory() {
    let env = Env::new("list-cwd-dotdot");
    let proj = env.dir.join("proj");
    let sub = proj.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let (proj, sub) = (proj.canonicalize().unwrap(), sub.canonicalize().unwrap());
    env.write_sessions(json!([
        {"id": "in-proj", "cwd": proj.display().to_string(),
         "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
        {"id": "elsewhere", "cwd": "/elsewhere",
         "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
    ]));
    for arg in ["..", "../sub/..", "../../proj"] {
        let out = env
            .command(&["list", "--cwd", arg])
            .current_dir(&sub)
            .output()
            .unwrap();
        assert!(
            stdout(&out).starts_with("in-proj "),
            "--cwd {arg}: {}",
            stdout(&out)
        );
        assert_eq!(stdout(&out).lines().count(), 1, "--cwd {arg}");
    }
}

#[test]
fn list_rejects_an_age_with_a_multi_byte_character_instead_of_panicking() {
    let env = filtered_env("list-since-utf8");
    for age in ["7é", "é", "7😀"] {
        let out = env.run(&["list", "--since", age]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "--since {age}: {}",
            stderr(&out)
        );
        assert!(stderr(&out).contains("--since"), "{}", stderr(&out));
        assert!(!stderr(&out).contains("panicked"), "{}", stderr(&out));
    }
}

#[test]
fn list_rejects_an_age_it_cannot_read() {
    let env = filtered_env("list-bad-since");
    let out = env.run(&["list", "--since", "soon"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--since"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
}

#[test]
fn list_json_prints_the_public_view_of_the_matching_sessions() {
    let env = filtered_env("list-json");
    let out = env.run(&["list", "--json", "--ticket", "ABC-1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let list: Value = serde_json::from_str(&stdout(&out)).expect("stdout is one JSON value");
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    let s = &list[0];
    assert_eq!(s["id"], "new");
    assert_eq!(s["title"], "Fix login");
    assert_eq!(s["title_source"], "own");
    assert_eq!(s["branch"], "feat/login");
    assert_eq!(s["tickets"], json!(["ABC-1"]));
    assert_eq!(s["tags"], json!(["auth"]));
    assert!(s["note"].is_null() && s["pr_url"].is_null());
    assert_eq!(s["agent"], "claude-code");
    assert!(s["updated_at"].as_str().unwrap().contains('T'));

    // Nothing matching is an empty array, not an error and not empty output.
    let none = env.run(&["list", "--json", "--tag", "nope"]);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout(&none)).unwrap(),
        json!([])
    );
}

#[test]
fn log_prints_a_timeline_newest_first_and_takes_the_list_filters() {
    let env = Env::new("log");
    env.write_sessions(json!([
        {"id": "b", "cwd": "/work/app", "title": "Add rate limit", "tickets": ["ABC-1"],
         "branch": "feat/limit", "created_at": "2026-10-08T09:00:00Z", "updated_at": "2026-10-08T10:00:00Z"},
        {"id": "a", "cwd": "/work/app", "title": "Fix login", "tickets": ["ABC-1"],
         "branch": "feat/login", "pr_url": "https://github.com/o/r/pull/558",
         "created_at": "2026-10-06T09:00:00Z", "updated_at": "2026-10-06T10:00:00Z"},
        {"id": "c", "cwd": "/work/other",
         "created_at": "2026-10-07T09:00:00Z", "updated_at": "2026-10-07T10:00:00Z"},
    ]));
    let out = env.run(&["log"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "2026-10-08  Add rate limit  feat/limit  /work/app\n\
         2026-10-07  (untitled)  /work/other\n\
         2026-10-06  Fix login  feat/login  PR #558  /work/app\n"
    );

    let ticket = stdout(&env.run(&["log", "--ticket", "ABC-1"]));
    assert_eq!(ticket.lines().count(), 2);
    assert!(ticket.starts_with("2026-10-08"));
    assert!(stdout(&env.run(&["log", "--tag", "nope"])).is_empty());
}

#[test]
fn log_markdown_links_the_pull_request_and_escapes_the_title() {
    let env = Env::new("log-markdown");
    env.write_sessions(json!([
        {"id": "a", "cwd": "/work/app", "title": "Fix *login*", "branch": "feat/login",
         "pr_url": "https://github.com/o/r/pull/558",
         "created_at": "2026-10-06T09:00:00Z", "updated_at": "2026-10-06T10:00:00Z"},
    ]));
    let out = env.run(&["log", "--markdown"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "- **2026-10-06** Fix \\*login\\* (`feat/login`, [PR #558](https://github.com/o/r/pull/558), `/work/app`)\n"
    );
}

#[test]
fn log_rejects_an_age_it_cannot_read() {
    let env = Env::new("log-bad-since");
    let out = env.run(&["log", "--since", "soon"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--since"), "{}", stderr(&out));
}

/// Sessions whose directories exist, a transcript for the first two, and a fake `claude` on the
/// PATH that records its working directory and arguments in `<dir>/launched`.
fn resumable_env(name: &str) -> (Env, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new(name);
    let (app, other) = (env.dir.join("app"), env.dir.join("other"));
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&other).unwrap();
    let (app, other) = (app.canonicalize().unwrap(), other.canonicalize().unwrap());
    env.write_sessions(json!([
        {"id": "login", "cwd": app.display().to_string(), "title": "Fix login", "tickets": ["ABC-1"], "tags": ["auth"],
         "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z"},
        {"id": "logout", "cwd": app.display().to_string(), "title": "Fix logout", "tickets": ["ABC-2"], "tags": ["auth"],
         "created_at": "2026-10-02T00:00:00Z", "updated_at": "2026-10-02T00:00:00Z"},
        {"id": "gone", "cwd": other.display().to_string(), "title": "Old work", "tickets": ["ABC-3"],
         "created_at": "2026-10-03T00:00:00Z", "updated_at": "2026-10-03T00:00:00Z"},
    ]));
    let projects = env.claude_dir().join("projects").join("-app");
    fs::create_dir_all(&projects).unwrap();
    for id in ["login", "logout"] {
        fs::write(projects.join(format!("{id}.jsonl")), "").unwrap();
    }
    let bin = env.dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let script = bin.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n{{ pwd; for a in \"$@\"; do echo \"$a\"; done; }} > '{}'\nprintf '%s' \"${{SESSIONS_PRESET-UNSET}}\" > '{}'\n",
            env.dir.join("launched").display(),
            env.dir.join("launched-preset").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    (env, bin)
}

fn resume(env: &Env, bin: &Path, args: &[&str]) -> Output {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut all = vec!["resume"];
    all.extend_from_slice(args);
    env.command(&all).env("PATH", path).output().unwrap()
}

fn launched(env: &Env) -> Option<Vec<String>> {
    fs::read_to_string(env.dir.join("launched"))
        .ok()
        .map(|t| t.lines().map(String::from).collect())
}

#[test]
fn resume_starts_claude_in_the_directory_of_the_one_matching_session() {
    let (env, bin) = resumable_env("resume-one");
    let out = resume(&env, &bin, &["ticket:ABC-2"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let run = launched(&env).expect("claude was started");
    assert!(run[0].ends_with("/app"), "{run:?}");
    assert_eq!(run[1..], ["--resume", "logout"]);
}

#[test]
fn resume_takes_the_filters_of_list_and_passes_the_saved_flags_on() {
    let (env, bin) = resumable_env("resume-filters");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = env
        .command(&["resume", "--tag", "auth", "--ticket", "ABC-1"])
        .env("PATH", path)
        .env("SESSIONS_CLAUDE_ARGS", "--model sonnet")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        launched(&env).unwrap()[1..],
        ["--resume", "login", "--model", "sonnet"]
    );
}

#[test]
fn resume_refuses_to_choose_between_several_matches() {
    let (env, bin) = resumable_env("resume-many");
    let out = resume(&env, &bin, &["--tag", "auth"]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("2 sessions match, so none was resumed"),
        "{message}"
    );
    assert!(
        message.contains("login") && message.contains("logout"),
        "{message}"
    );
    assert!(launched(&env).is_none(), "nothing may be started");
}

#[test]
fn resume_reports_no_match_and_a_missing_transcript() {
    let (env, bin) = resumable_env("resume-none");
    let none = resume(&env, &bin, &["ticket:NOPE-1"]);
    assert!(!none.status.success());
    assert!(stderr(&none).contains("no session matches"));

    let gone = resume(&env, &bin, &["--ticket", "ABC-3"]);
    assert!(!gone.status.success());
    assert!(
        stderr(&gone).contains("transcript of gone is gone"),
        "{}",
        stderr(&gone)
    );
    assert!(launched(&env).is_none());
}

#[test]
fn resume_query_words_with_a_hash_match_a_tag_exactly() {
    let (env, bin) = resumable_env("resume-hash-tag");
    let out = resume(&env, &bin, &["--list", "#auth"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 2);
    let none = resume(&env, &bin, &["--list", "#aut"]);
    assert!(stdout(&none).is_empty(), "a tag word is not fuzzy");
}

#[test]
fn resume_list_shows_the_matches_without_starting_anything() {
    let (env, bin) = resumable_env("resume-list");
    let out = resume(&env, &bin, &["--list", "--tag", "auth"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let ids: Vec<_> = stdout(&out)
        .lines()
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["logout", "login"], "most recent first");
    assert!(launched(&env).is_none());
}

#[test]
fn archive_hides_a_session_from_list_and_resume_but_not_from_log_or_list_all() {
    let (env, bin) = resumable_env("archive");
    assert!(env.run(&["archive", "login"]).status.success());
    assert_eq!(env.session("login")["archived"], true);
    // Archiving is not activity: the timeline and the sort order keep their place.
    assert_eq!(env.session("login")["updated_at"], "2026-10-01T00:00:00Z");

    assert_eq!(listed_ids(&env, &[]), ["logout", "gone"]);
    assert_eq!(listed_ids(&env, &["--all"]), ["login", "logout", "gone"]);
    assert_eq!(
        listed_ids(&env, &["--tag", "auth", "--all"]),
        ["login", "logout"]
    );

    let json = env.run(&["list", "--json", "--all", "--tag", "auth"]);
    let list: Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(list[0]["id"], "login");
    assert_eq!(list[0]["archived"], true);
    assert_eq!(list[1]["archived"], false);

    assert_eq!(
        stdout(&env.run(&["log"])).lines().count(),
        3,
        "log keeps archived sessions"
    );

    let hidden = resume(&env, &bin, &["--ticket", "ABC-1"]);
    assert!(
        stderr(&hidden).contains("no session matches"),
        "{}",
        stderr(&hidden)
    );
    assert!(launched(&env).is_none());
    let shown = resume(&env, &bin, &["--all", "--ticket", "ABC-1"]);
    assert!(shown.status.success(), "{}", stderr(&shown));
    assert_eq!(launched(&env).unwrap()[1..], ["--resume", "login"]);

    assert!(env.run(&["unarchive", "login"]).status.success());
    assert!(env.session("login").get("archived").is_none());
    assert_eq!(listed_ids(&env, &[]), ["login", "logout", "gone"]);
}

#[test]
fn archive_survives_the_hook_and_fails_for_an_unknown_session() {
    let env = Env::new("archive-hook");
    env.register("s1", "/w");
    assert!(env.run(&["archive", "s1"]).status.success());
    env.hook(json!({"session_id": "s1", "cwd": "/w2", "source": "resume"}));
    assert_eq!(env.session("s1")["archived"], true);

    for command in ["archive", "unarchive"] {
        let out = env.run(&[command, "typo"]);
        assert!(!out.status.success(), "{command}");
        assert!(stderr(&out).contains("no session typo"), "{}", stderr(&out));
    }
    assert_eq!(env.sessions().len(), 1);
}

#[test]
fn list_survives_a_closed_pipe() {
    let env = Env::new("list-pipe");
    for i in 0..200 {
        env.register(&format!("s{i}"), "/w");
    }
    let out = run_with_closed_stdout(env.command(&["list"]));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains("panicked"), "{}", stderr(&out));
}

/// Runs `cmd` with stdout connected to a pipe whose reader is already gone, so that every write
/// fails the way it does for `sessions … | head` once `head` has what it wanted. The pipe is closed
/// before the program starts, so the result does not depend on timing.
fn run_with_closed_stdout(mut cmd: Command) -> Output {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    cmd.stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap()
}

fn assert_no_panic(what: &str, out: &Output) {
    assert!(!stderr(out).contains("panicked"), "{what}: {}", stderr(out));
    assert_ne!(out.status.code(), Some(101), "{what}: {}", stderr(out));
}

#[test]
fn prune_and_migrate_tickets_end_quietly_on_a_closed_pipe_and_still_do_their_work() {
    let env = Env::new("closed-pipe-work");
    // Fresh enough not to count as an orphan: prune spares what was touched in the last hour.
    let now = chrono::Utc::now().to_rfc3339();
    env.write_sessions(json!([
        {"id": "o1", "cwd": "/w", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"},
        {"id": "o2", "cwd": "/w", "created_at": "2026-01-02T00:00:00Z", "updated_at": "2026-01-02T00:00:00Z"},
        {"id": "k1", "cwd": "/w", "tags": ["ABC-1"], "created_at": now, "updated_at": now},
    ]));
    // The dry runs only print, so a closed pipe ends them without a panic.
    for args in [&["prune"][..], &["migrate-tickets"]] {
        let out = run_with_closed_stdout(env.command(args));
        assert_no_panic(&args.join(" "), &out);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    }
    assert_eq!(env.sessions().len(), 3, "a dry run changes nothing");

    // What was asked for is done even though nobody reads the output.
    let out = run_with_closed_stdout(env.command(&["migrate-tickets", "--yes"]));
    assert_no_panic("migrate-tickets --yes", &out);
    assert_eq!(env.session("k1")["tickets"], json!(["ABC-1"]));
    let out = run_with_closed_stdout(env.command(&["prune", "--yes"]));
    assert_no_panic("prune --yes", &out);
    assert_eq!(
        env.sessions().len(),
        1,
        "the orphans are gone, k1 is fresh enough to stay"
    );
}

#[test]
fn doctor_setup_import_and_the_hook_do_not_panic_on_a_closed_pipe() {
    let env = Env::new("closed-pipe-others");
    assert_no_panic("doctor", &run_with_closed_stdout(env.command(&["doctor"])));
    assert_no_panic("import", &run_with_closed_stdout(env.command(&["import"])));

    let setup = run_with_closed_stdout(env.command(&["setup"]));
    assert_no_panic("setup", &setup);
    assert!(setup.status.success(), "{}", stderr(&setup));
    assert!(
        env.settings_file().exists(),
        "setup still wrote the settings"
    );

    let mut cmd = env.command(&["hook"]);
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"s1","cwd":"/w","source":"startup"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_no_panic("hook", &out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        env.session("s1")["cwd"],
        "/w",
        "the session was registered anyway"
    );
}

#[test]
fn nothing_panics_when_home_is_not_set() {
    let env = Env::new("no-home");
    let bare = |args: &[&str]| {
        let mut cmd = env.command(args);
        cmd.env_remove("HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("SESSIONS_FILE");
        cmd
    };
    for args in [
        &["list"][..],
        &["import"],
        &["prune"],
        &["setup"],
        &["title", "--id", "x", "t"],
    ] {
        let out = bare(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("HOME is not set"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert!(
            !stderr(&out).contains("panicked"),
            "{args:?}: {}",
            stderr(&out)
        );
    }

    // doctor still reports what it can, and says what is missing.
    let out = bare(&["doctor"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("HOME is not set"), "{}", stdout(&out));
    assert!(!stderr(&out).contains("panicked"), "{}", stderr(&out));

    // The hook must not get in the way: one JSON object, status 0, the problem on stderr.
    let mut cmd = bare(&["hook"]);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"s1","cwd":"/w","source":"startup"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let hook = hook_output(&out);
    assert!(
        hook["additionalContext"]
            .as_str()
            .unwrap()
            .contains("Session ID: s1")
    );
    assert!(
        stderr(&out).contains("could not register the session"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn rm_removes_a_session_and_fails_for_an_unknown_id() {
    let env = Env::new("rm");
    env.register("s1", "/w");
    env.register("s2", "/w");
    assert!(env.run(&["rm", "s1"]).status.success());
    let ids: Vec<_> = env.sessions().iter().map(|s| s["id"].clone()).collect();
    assert_eq!(ids, [json!("s2")]);

    let out = env.run(&["rm", "nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no session nope"));
}

#[test]
fn prune_is_a_dry_run_until_confirmed_and_spares_fresh_entries() {
    let env = Env::new("prune");
    env.register("fresh", "/w");
    env.write_sessions(json!([
        {"id": "old", "cwd": "/w", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"},
    ]));
    env.register("fresh", "/w");
    // Neither has a transcript, but an entry touched in the last hour is spared.
    let dry = stdout(&env.run(&["prune"]));
    assert!(dry.starts_with("old"), "{dry}");
    assert!(
        dry.contains("1 orphaned session(s); run `sessions prune --yes`"),
        "{dry}"
    );
    assert_eq!(env.sessions().len(), 2);

    assert!(env.run(&["prune", "--yes"]).status.success());
    let ids: Vec<_> = env.sessions().iter().map(|s| s["id"].clone()).collect();
    assert_eq!(ids, [json!("fresh")]);
}

#[test]
fn import_registers_transcripts_that_are_not_known_yet() {
    let env = Env::new("import");
    let project = env.claude_dir().join("projects").join("-work-a");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("t1.jsonl"),
        concat!(
            r#"{"type":"user","cwd":"/work/a","sessionId":"t1","message":{"role":"user","content":"Fix the login redirect"}}"#,
            "\n"
        ),
    )
    .unwrap();
    let out = env.run(&["import"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 added"), "{}", stdout(&out));
    assert_eq!(env.session("t1")["cwd"], "/work/a");
    // A second run finds nothing new.
    assert!(stdout(&env.run(&["import"])).contains("0 added"));
}

#[test]
fn migrate_tickets_shows_a_plan_and_applies_it_with_a_backup() {
    let env = Env::new("migrate");
    // Tags are lower-cased today, so a key-shaped tag only exists in files from before tickets.
    env.write_sessions(json!([
        {"id": "s1", "cwd": "/w", "tags": ["ABC-123", "auth"],
         "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"},
    ]));
    let dry = stdout(&env.run(&["migrate-tickets"]));
    assert!(dry.contains("tags -> tickets: ABC-123"), "{dry}");
    assert_eq!(env.session("s1")["tags"], json!(["ABC-123", "auth"]));

    let out = env.run(&["migrate-tickets", "--yes"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let s = env.session("s1");
    assert_eq!(s["tickets"], json!(["ABC-123"]));
    assert_eq!(s["tags"], json!(["auth"]));
    let backups = fs::read_dir(&env.dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("json.bak-")
        })
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn a_corrupt_sessions_file_is_not_silently_replaced() {
    let env = Env::new("corrupt");
    fs::write(env.sessions_file(), "{ not json").unwrap();
    let out = env.hook(json!({"session_id": "s1", "cwd": "/w", "source": "startup"}));
    // Whatever the recovery does, the broken file must survive somewhere the user can find it.
    let kept = fs::read_dir(&env.dir).unwrap().any(|e| {
        fs::read_to_string(e.unwrap().path()).is_ok_and(|text| text.contains("{ not json"))
    });
    assert!(kept, "corrupt file was lost; hook said: {}", stderr(&out));
}

#[test]
fn setup_adds_the_hook_once_and_keeps_other_settings() {
    let env = Env::new("setup");
    fs::write(
        env.settings_file(),
        r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
    )
    .unwrap();
    let out = env.run(&["setup"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Added the SessionStart hook"));

    let settings: Value =
        serde_json::from_str(&fs::read_to_string(env.settings_file()).unwrap()).unwrap();
    assert_eq!(settings["theme"], "dark");
    assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "true");
    let entry = &settings["hooks"]["SessionStart"][0];
    assert!(
        entry["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with("sessions hook")
    );
    let matcher = entry["matcher"].as_str().unwrap();
    for source in ["startup", "resume", "clear", "compact", "fork"] {
        assert!(matcher.split('|').any(|m| m == source), "missing {source}");
    }

    let backups = fs::read_dir(env.claude_dir())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.")
        })
        .count();
    assert!(backups >= 1, "no backup of settings.json");

    let again = env.run(&["setup"]);
    assert!(stdout(&again).contains("already configured"));
}

#[test]
fn setup_leaves_a_settings_file_that_is_not_json_alone() {
    let env = Env::new("setup-invalid");
    fs::write(env.settings_file(), "{ nope").unwrap();
    let out = env.run(&["setup"]);
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(env.settings_file()).unwrap(), "{ nope");
}

#[test]
fn doctor_reports_the_hook_and_fails_without_one_only_when_something_is_broken() {
    let env = Env::new("doctor");
    assert!(env.run(&["setup"]).status.success());
    env.register("s1", "/w");
    let out = env.run(&["doctor"]);
    let text = stdout(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("hook"), "{text}");
    assert!(text.contains("sessions"), "{text}");
    assert!(text.contains("(1 entry)"), "{text}");
}

#[test]
fn doctor_exits_non_zero_when_the_sessions_file_is_unreadable() {
    let env = Env::new("doctor-fail");
    fs::write(env.sessions_file(), "{ not json").unwrap();
    let out = env.run(&["doctor"]);
    assert!(!out.status.success(), "{}", stdout(&out));
}

#[test]
fn version_flag_prints_the_crate_version() {
    let env = Env::new("version");
    let out = env.run(&["--version"]);
    assert!(stdout(&out).contains(env!("CARGO_PKG_VERSION")));
}

fn start(env: &Env, bin: &Path, args: &[&str]) -> Output {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut all = vec!["start"];
    all.extend_from_slice(args);
    env.command(&all).env("PATH", path).output().unwrap()
}

fn launched_preset(env: &Env) -> Option<String> {
    fs::read_to_string(env.dir.join("launched-preset")).ok()
}

#[test]
fn start_runs_claude_in_the_directory_with_the_message_and_the_preset() {
    let (env, bin) = resumable_env("start");
    let dir = env.dir.join("app");
    let out = start(
        &env,
        &bin,
        &[
            "--dir",
            dir.to_str().unwrap(),
            "--ticket",
            "ABC-1",
            "--ticket",
            "ABC-2",
            "--title",
            "Fix login",
            "fix",
            "the",
            "login",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let run = launched(&env).expect("claude was started");
    assert!(run[0].ends_with("/app"), "{run:?}");
    assert_eq!(run[1..], ["fix the login"], "the words are one message");
    let preset: Value = serde_json::from_str(&launched_preset(&env).unwrap()).unwrap();
    assert_eq!(
        preset,
        json!({"tickets": ["ABC-1", "ABC-2"], "title": "Fix login"})
    );
}

#[test]
fn start_passes_the_saved_flags_before_the_message_and_guards_a_dash() {
    let (env, bin) = resumable_env("start-flags");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = env
        .command(&[
            "start",
            "--dir",
            env.dir.join("app").to_str().unwrap(),
            // After `--`, so that `sessions start` itself does not read it as an option.
            "--",
            "--version please",
        ])
        .env("PATH", path)
        .env("SESSIONS_CLAUDE_ARGS", "--model sonnet")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        launched(&env).unwrap()[1..],
        ["--model", "sonnet", " --version please"]
    );
}

#[test]
fn start_without_a_dir_uses_the_current_directory_and_expands_home() {
    let (env, bin) = resumable_env("start-dirs");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let app = env.dir.join("app").canonicalize().unwrap();
    let out = env
        .command(&["start"])
        .current_dir(&app)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let run = launched(&env).unwrap();
    assert_eq!(run, [app.display().to_string()], "no message, no argument");
    assert_eq!(
        launched_preset(&env).as_deref(),
        Some("UNSET"),
        "no preset, no variable"
    );

    let _ = fs::remove_file(env.dir.join("launched"));
    // The scratch HOME is the environment's directory, so ~/app is the app directory.
    let out = env
        .command(&["start", "--dir", "~/app"])
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(launched(&env).unwrap()[0].ends_with("/app"));
}

#[test]
fn start_removes_a_preset_inherited_from_the_shell() {
    let (env, bin) = resumable_env("start-inherited");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = env
        .command(&["start", "--dir", env.dir.join("app").to_str().unwrap()])
        .env("PATH", path)
        .env("SESSIONS_PRESET", r#"{"tickets":["OLD-1"]}"#)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(launched_preset(&env).as_deref(), Some("UNSET"));
}

#[test]
fn start_refuses_a_directory_that_does_not_exist_or_is_a_file() {
    let (env, bin) = resumable_env("start-bad-dir");
    fs::write(env.dir.join("file.txt"), "x").unwrap();
    for (dir, reason) in [
        ("/no/such/dir", "No such directory: /no/such/dir"),
        ("~/file.txt", "Not a directory"),
    ] {
        let out = start(&env, &bin, &["--dir", dir, "hello"]);
        assert!(!out.status.success(), "{dir}");
        assert!(stderr(&out).contains(reason), "{dir}: {}", stderr(&out));
    }
    assert!(launched(&env).is_none(), "nothing may be started");
}

#[test]
fn a_started_session_gets_its_title_and_tickets_from_the_hook() {
    let (env, bin) = resumable_env("start-hook");
    let app = env.dir.join("app").canonicalize().unwrap();
    let out = start(
        &env,
        &bin,
        &[
            "--dir",
            app.to_str().unwrap(),
            "--ticket",
            "ABC-7",
            "--title",
            "Fix login",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    // What the fake claude saw is what the real one passes to the hook of the new session.
    let preset = launched_preset(&env).unwrap();
    let out = env.hook_with_preset(
        json!({"session_id": "new1", "cwd": app.display().to_string(), "source": "startup"}),
        Some(&preset),
    );
    let hook = hook_output(&out);
    let s = env.session("new1");
    assert_eq!(s["title"], "Fix login");
    assert_eq!(s["tickets"], json!(["ABC-7"]));
    assert_eq!(hook["sessionTitle"], "Fix login");
    assert!(
        hook["additionalContext"]
            .as_str()
            .unwrap()
            .contains("started for: ABC-7"),
        "{hook}"
    );
}

#[test]
fn a_preset_only_applies_to_a_new_session_at_startup() {
    let env = Env::new("preset-scope");
    let preset = r#"{"tickets":["ABC-1"],"title":"From preset"}"#;
    let payload = |id: &str, source: &str| json!({"session_id": id, "cwd": "/w", "source": source});

    // An existing session is left alone, even at startup.
    env.register("old", "/w");
    assert!(
        env.hook_with_preset(payload("old", "startup"), Some(preset))
            .status
            .success()
    );
    assert!(
        env.session("old").get("title").is_none() && env.session("old").get("tickets").is_none()
    );

    // The variable is inherited by /resume and /clear in the same process: not for them.
    for (id, source) in [
        ("r", "resume"),
        ("c", "clear"),
        ("f", "fork"),
        ("k", "compact"),
    ] {
        let out = env.hook_with_preset(payload(id, source), Some(preset));
        assert!(out.status.success(), "{source}: {}", stderr(&out));
        let s = env.session(id);
        assert!(
            s.get("title").is_none() && s.get("tickets").is_none(),
            "{source}: {s}"
        );
    }

    // A new session at startup gets it, and only the first time.
    assert!(
        env.hook_with_preset(payload("new", "startup"), Some(preset))
            .status
            .success()
    );
    assert_eq!(env.session("new")["title"], "From preset");
    env.run(&["title", "--id", "new", "Renamed"]);
    assert!(
        env.hook_with_preset(payload("new", "startup"), Some(preset))
            .status
            .success()
    );
    assert_eq!(env.session("new")["title"], "Renamed");
}

#[test]
fn a_broken_preset_never_stops_a_session_from_starting() {
    let env = Env::new("preset-broken");
    for (i, preset) in ["not json", "[1,2]", r#"{"tickets":"ABC-1"}"#, "", "{}"]
        .iter()
        .enumerate()
    {
        let id = format!("s{i}");
        let out = env.hook_with_preset(
            json!({"session_id": id, "cwd": "/w", "source": "startup"}),
            Some(preset),
        );
        assert!(out.status.success(), "{preset:?}: {}", stderr(&out));
        hook_output(&out);
        let s = env.session(&id);
        assert!(
            s.get("title").is_none() && s.get("tickets").is_none(),
            "{preset:?}"
        );
    }
}

/// The names of the environment variables the program reads: the literals passed to
/// `std::env::var` and `var_os` in `src/`, and every string that is exactly the name of one of
/// ours (`SESSIONS_…`, `CLAUDE_…`), which also finds one read through a constant. `PATH` is left
/// out, because the tests use the real one on purpose.
fn variables_read_by_the_program() -> Vec<String> {
    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let ours = |s: &str| {
        (s.starts_with("SESSIONS_") || s.starts_with("CLAUDE_"))
            && s.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    };
    let mut files = Vec::new();
    rust_files(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut names = Vec::new();
    let mut add = |name: &str| {
        if name != "PATH" && !names.contains(&name.to_string()) {
            names.push(name.to_string());
        }
    };
    for file in files {
        let text = fs::read_to_string(file).unwrap();
        for call in ["env::var(\"", "env::var_os(\""] {
            for part in text.split(call).skip(1) {
                add(part.split('"').next().unwrap_or(""));
            }
        }
        // Between two quotes: every second piece of a line split on `"` is a string literal.
        for line in text.lines() {
            for literal in line.split('"').skip(1).step_by(2) {
                if ours(literal) {
                    add(literal);
                }
            }
        }
    }
    names.sort();
    names
}

/// A variable the program reads that `Env::command` neither sets nor removes would let the shell
/// that runs `cargo test` change the result, as `SESSIONS_PRESET` once did.
#[test]
fn the_test_environment_sets_or_clears_every_variable_the_program_reads() {
    let source = include_str!("cli.rs");
    let start = source.find("fn command(&self").expect("Env::command");
    let end = source[start..].find("fn run(&self").expect("Env::run") + start;
    let command = &source[start..end];
    let names = variables_read_by_the_program();
    assert!(
        names.len() >= 8,
        "found too few variables, the scan is broken: {names:?}"
    );
    for name in names {
        assert!(
            command.contains(&format!("\"{name}\"")),
            "Env::command neither sets nor removes {name}"
        );
    }
}
