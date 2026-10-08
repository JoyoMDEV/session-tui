//! Runs the real binary against temporary directories and checks what a user or Claude Code sees:
//! output, exit status and the files it writes. Nothing here touches the real `~/.claude`.

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
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
            .env_remove("SESSIONS_RESUME_REMINDER");
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs the hook with `payload` on stdin.
    fn hook(&self, payload: Value) -> Output {
        let mut child = self
            .command(&["hook"])
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
fn log_prints_a_timeline_oldest_first_and_takes_the_list_filters() {
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
        "2026-10-06  Fix login  feat/login  PR #558  /work/app\n\
         2026-10-07  (untitled)  /work/other\n\
         2026-10-08  Add rate limit  feat/limit  /work/app\n"
    );

    let ticket = stdout(&env.run(&["log", "--ticket", "ABC-1"]));
    assert_eq!(ticket.lines().count(), 2);
    assert!(ticket.starts_with("2026-10-06"));
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

#[test]
fn list_survives_a_closed_pipe() {
    let env = Env::new("list-pipe");
    for i in 0..200 {
        env.register(&format!("s{i}"), "/w");
    }
    let mut child = env
        .command(&["list"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(!stderr(&out).contains("panicked"), "{}", stderr(&out));
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
