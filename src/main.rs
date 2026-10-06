//! The `sessions` binary: command line parsing, the SessionStart hook and the small commands
//! (`title`, `tag`, `ticket`, `note`, `import`, `prune`, `rm`, `list`). Output that may be piped
//! goes through `writeln!`, never `println!`.

mod doctor;
mod setup;
mod store;
mod tags;
mod tickets;
mod transcript;
mod tui;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::io::{Read, Write};

#[derive(Parser)]
#[command(version, about = "Browse and resume saved Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add or repair the SessionStart hook in Claude Code's settings.json (keeps a backup)
    Setup {
        /// Point the hook at this binary even if another working one is registered
        #[arg(long)]
        force: bool,
    },
    /// Check the install: binary, PATH, hook, sessions file, transcripts and retention
    Doctor,
    /// SessionStart hook: reads hook JSON from stdin, registers the session, injects its id
    Hook,
    /// Set the title of a session (defaults to $CLAUDE_SESSION_ID)
    Title {
        #[arg(long)]
        id: Option<String>,
        title: String,
    },
    /// Add or remove ticket keys such as ABC-123 (session defaults to $CLAUDE_SESSION_ID)
    Ticket {
        #[arg(long)]
        id: Option<String>,
        /// Remove the keys instead of adding them
        #[arg(long)]
        remove: bool,
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Move tags that look like ticket keys into the tickets field (dry run unless --yes)
    MigrateTickets {
        #[arg(long)]
        yes: bool,
    },
    /// Add or remove topic tags (session defaults to $CLAUDE_SESSION_ID), e.g. observability
    Tag {
        #[arg(long)]
        id: Option<String>,
        /// Remove the tags instead of adding them
        #[arg(long)]
        remove: bool,
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Set the note of a session (defaults to $CLAUDE_SESSION_ID); empty text clears it
    Note {
        #[arg(long)]
        id: Option<String>,
        text: String,
    },
    /// Register existing Claude Code transcripts that aren't known yet
    Import,
    /// Remove sessions whose transcript no longer exists (dry run unless --yes)
    Prune {
        #[arg(long)]
        yes: bool,
    },
    /// Remove a session
    Rm { id: String },
    /// Print all sessions
    List {
        /// Only sessions with this ticket (repeatable: all of them must match)
        #[arg(long = "ticket", value_name = "KEY")]
        tickets: Vec<String>,
    },
}

#[derive(Deserialize)]
struct HookInput {
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    source: String,
    /// Present when the session already has a name, e.g. from `/rename` or `--name`.
    #[serde(default)]
    session_title: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        None => tui::run(),
        Some(Cmd::Setup { force }) => setup::run(force),
        Some(Cmd::Doctor) => doctor::run(),
        Some(Cmd::Hook) => hook(),
        Some(Cmd::Title { id, title }) => {
            let title = non_empty(title)?;
            modify(id, |s| {
                s.title = Some(title);
                s.updated_at = Utc::now();
            })
        }
        Some(Cmd::Ticket { id, remove, keys }) => modify(id, |s| {
            for key in &keys {
                if remove {
                    tickets::remove(&mut s.tickets, key);
                } else {
                    tickets::add(&mut s.tickets, key);
                }
            }
        }),
        Some(Cmd::MigrateTickets { yes }) => migrate_tickets(yes),
        Some(Cmd::Tag { id, remove, names }) => {
            // Tags are topics. A ticket key such as ABC-123 goes to the tickets field instead.
            let is_key = |n: &String| !remove && tickets::looks_like_key(n.trim());
            let (keys, names): (Vec<String>, Vec<String>) = names.into_iter().partition(is_key);
            if !keys.is_empty() {
                eprintln!(
                    "{} looks like a ticket key; recorded as a ticket, not a tag",
                    keys.join(" ")
                );
            }
            modify(id, |s| {
                for key in &keys {
                    tickets::add(&mut s.tickets, key);
                }
                for name in &names {
                    if remove {
                        tags::remove(&mut s.tags, name);
                    } else {
                        tags::add(&mut s.tags, name);
                    }
                }
            })
        }
        Some(Cmd::Note { id, text }) => {
            let text = text.trim().to_string();
            modify(id, |s| s.note = Some(text).filter(|t| !t.is_empty()))
        }
        Some(Cmd::Import) => {
            let st = transcript::import()?;
            println!(
                "{} added, {} suggestions filled, {} empty transcripts skipped",
                st.added, st.updated, st.skipped_empty
            );
            Ok(())
        }
        Some(Cmd::Prune { yes }) => prune(yes),
        Some(Cmd::Rm { id }) => {
            if !store::update(|s| {
                let n = s.len();
                s.retain(|x| x.id != id);
                s.len() != n
            })? {
                bail!("no session {id}");
            }
            Ok(())
        }
        Some(Cmd::List { tickets: wanted }) => {
            let mut out = std::io::stdout().lock();
            for s in store::load()?
                .into_iter()
                .filter(|s| tickets::has_all(s, &wanted))
            {
                let tickets = if s.tickets.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", s.tickets.join(" "))
                };
                let tags = if s.tags.is_empty() {
                    String::new()
                } else {
                    format!("  #{}", s.tags.join(" #"))
                };
                let written = writeln!(
                    out,
                    "{}  {}  {}  {}{}{}",
                    s.id,
                    s.updated_at.format("%Y-%m-%d %H:%M"),
                    s.title
                        .as_deref()
                        .or(s.suggestion.as_deref().map(|_| "(suggested)"))
                        .unwrap_or("-"),
                    s.cwd,
                    tickets,
                    tags
                );
                // `println!` panics on a closed pipe, e.g. `sessions list | head`.
                if written.is_err() {
                    break;
                }
            }
            Ok(())
        }
    }
}

fn migrate_tickets(yes: bool) -> Result<()> {
    let plan = tickets::migration_plan(&store::load()?);
    for m in &plan {
        let title = if m.title.is_empty() { "-" } else { &m.title };
        println!("{}  {}  tags -> tickets: {}", m.id, title, m.keys.join(" "));
    }
    if plan.is_empty() {
        println!("No tags look like ticket keys.");
        return Ok(());
    }
    if !yes {
        println!(
            "{} session(s) would change; run `sessions migrate-tickets --yes` to apply",
            plan.len()
        );
        return Ok(());
    }
    let path = store::path();
    let backup = path.with_extension(format!("json.bak-{}", Utc::now().timestamp()));
    std::fs::copy(&path, &backup).with_context(|| format!("backing up to {}", backup.display()))?;
    let changed = store::update(|all| tickets::migrate(all))?;
    println!(
        "moved the keys of {changed} session(s); the old file is {}",
        backup.display()
    );
    Ok(())
}

fn non_empty(s: String) -> Result<String> {
    let s = s.trim().to_string();
    if s.is_empty() {
        bail!("empty title");
    }
    Ok(s)
}

fn hook() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let h: HookInput = serde_json::from_str(&input).context("parsing hook input")?;

    let view = store::update(|sessions| {
        store::upsert(sessions, &h.session_id, &h.cwd);
        if let Some(s) = sessions.iter_mut().find(|s| s.id == h.session_id) {
            transcript::fill_suggestion(s);
        }
        let s = sessions.iter().find(|s| s.id == h.session_id)?;
        Some(HookView {
            title: s.title.clone(),
            tags: s.tags.clone(),
            vocabulary: tags::vocabulary(sessions),
            // Set SESSIONS_RESUME_REMINDER=0 to turn the reminder off.
            remind_on_resume: std::env::var("SESSIONS_RESUME_REMINDER").as_deref() != Ok("0"),
        })
    })?
    .unwrap_or_default();

    // The agent's shell may not have our install dir on PATH, so use the absolute path.
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().context("binary has no parent dir")?;

    // Make the id (and the binary) available to the agent's later Bash calls.
    if let Some(env_file) = std::env::var_os("CLAUDE_ENV_FILE") {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(env_file)?;
        writeln!(f, "export CLAUDE_SESSION_ID={}", h.session_id)?;
        writeln!(f, "export PATH=\"{}:$PATH\"", exe_dir.display())?;
    }

    println!("{}", hook_output(&h, &view, &exe));
    Ok(())
}

/// Claude Code ignores `sessionTitle` on `clear` and `compact`.
fn title_applies(source: &str) -> bool {
    matches!(source, "startup" | "resume" | "fork")
}

/// What the hook knows about the session when it builds its answer.
#[derive(Default)]
struct HookView {
    title: Option<String>,
    tags: Vec<String>,
    /// Tags in use across all sessions, most used first.
    vocabulary: Vec<(String, usize)>,
    remind_on_resume: bool,
}

fn hook_context(h: &HookInput, view: &HookView, exe: &std::path::Path) -> String {
    let (id, exe) = (&h.session_id, exe.display());
    let mut text = format!(
        "Session ID: {id} (also in $CLAUDE_SESSION_ID). Once the topic of this session is clear, \
         give it a short, descriptive title exactly once: {exe} title --id {id} \"<title>\". \
         Only update it if the topic changes fundamentally. If the work belongs to tickets or \
         issues, record their keys: {exe} ticket --id {id} <KEY>... \
         Tags are for topics, not tickets. Keep to a few (three at most) and prefer one that \
         already exists; add a new tag only if none fits: {exe} tag --id {id} <TAG>... \
         Remove one that no longer fits: {exe} tag --id {id} --remove <TAG>."
    );
    let vocabulary = tags::vocabulary_text(&view.vocabulary);
    if !vocabulary.is_empty() {
        text += &format!(" Tags already in use (sessions): {vocabulary}.");
    }
    // Only on resume: the topic may have moved on since the title and tags were set. Nothing is
    // added later in a session, because a hook that nags after every answer would be intrusive.
    if h.source == "resume" && view.remind_on_resume {
        let title = view.title.as_deref().unwrap_or("none yet");
        let tags = if view.tags.is_empty() {
            "none".to_string()
        } else {
            view.tags.join(", ")
        };
        text += &format!(
            " This session is being resumed. Check that its title ({title}) and tags ({tags}) \
             still fit the work, and correct them if they don't."
        );
    }
    text
}

fn hook_output(h: &HookInput, view: &HookView, exe: &std::path::Path) -> serde_json::Value {
    let context = hook_context(h, view, exe);
    let mut out =
        serde_json::json!({ "hookEventName": "SessionStart", "additionalContext": context });
    // Pass our title on as Claude Code's own session name so `claude --resume` shows it too,
    // but never replace a name that is already set.
    if let (true, None, Some(title)) = (title_applies(&h.source), &h.session_title, &view.title) {
        out["sessionTitle"] = serde_json::json!(title);
    }
    serde_json::json!({ "hookSpecificOutput": out })
}

/// Applies `f` to the session with the given id (default `$CLAUDE_SESSION_ID`). An unknown id is
/// an error, so a typo doesn't create an empty entry; `sessions import` registers older sessions.
fn modify(id: Option<String>, f: impl FnOnce(&mut store::Session)) -> Result<()> {
    let id = id
        .or_else(|| std::env::var("CLAUDE_SESSION_ID").ok())
        .context("no --id given and $CLAUDE_SESSION_ID is not set")?;
    store::update(|sessions| match sessions.iter_mut().find(|s| s.id == id) {
        Some(s) => {
            f(s);
            Ok(())
        }
        None => bail!("no session {id}; `sessions import` registers sessions from before the hook"),
    })?
}

fn prune(yes: bool) -> Result<()> {
    let existing = transcript::existing_ids();
    let orphan = |s: &store::Session| transcript::is_orphan(s, &existing);

    if !yes {
        let orphans: Vec<_> = store::load()?.into_iter().filter(|s| orphan(s)).collect();
        for s in &orphans {
            println!("{}  {}  {}", s.id, s.title.as_deref().unwrap_or("-"), s.cwd);
        }
        println!(
            "{} orphaned session(s); run `sessions prune --yes` to remove",
            orphans.len()
        );
        return Ok(());
    }
    let removed = store::update(|all| {
        let n = all.len();
        all.retain(|s| !orphan(s));
        n - all.len()
    })?;
    println!("removed {removed} orphaned session(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(source: &str, session_title: Option<&str>) -> HookInput {
        HookInput {
            session_id: "abc".into(),
            cwd: "/x".into(),
            source: source.into(),
            session_title: session_title.map(str::to_string),
        }
    }

    fn view(title: Option<&str>) -> HookView {
        HookView {
            title: title.map(str::to_string),
            remind_on_resume: true,
            ..Default::default()
        }
    }

    const EXE: &str = "/bin/sessions";

    fn title_of(out: &serde_json::Value) -> Option<&str> {
        out["hookSpecificOutput"]["sessionTitle"].as_str()
    }

    fn context(h: &HookInput, v: &HookView) -> String {
        hook_context(h, v, EXE.as_ref())
    }

    #[test]
    fn hook_hands_our_title_to_claude_on_resume() {
        let out = hook_output(
            &input("resume", None),
            &view(Some("My title")),
            EXE.as_ref(),
        );
        assert_eq!(title_of(&out), Some("My title"));
        assert!(
            out["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("abc")
        );
    }

    #[test]
    fn hook_never_replaces_an_existing_name() {
        let out = hook_output(
            &input("resume", Some("renamed")),
            &view(Some("Mine")),
            EXE.as_ref(),
        );
        assert_eq!(title_of(&out), None);
    }

    #[test]
    fn hook_skips_title_on_sources_where_claude_ignores_it_or_without_a_title() {
        let title = |source: &str, v: &HookView| {
            title_of(&hook_output(&input(source, None), v, EXE.as_ref())).map(str::to_string)
        };
        assert_eq!(title("clear", &view(Some("T"))), None);
        assert_eq!(title("compact", &view(Some("T"))), None);
        assert_eq!(title("startup", &view(None)), None);
        assert_eq!(title("", &view(Some("T"))), None);
    }

    #[test]
    fn the_context_asks_for_few_existing_tags_and_for_tickets_separately() {
        let text = context(&input("startup", None), &view(None));
        assert!(text.contains("ticket --id abc <KEY>"));
        assert!(text.contains("tag --id abc <TAG>"));
        assert!(text.contains("--remove <TAG>"));
        assert!(text.contains("prefer one that already exists"));
        assert!(
            !text.contains("already in use"),
            "no vocabulary yet, so no list"
        );
    }

    #[test]
    fn the_context_lists_the_tags_already_in_use() {
        let mut v = view(None);
        v.vocabulary = vec![("observability".into(), 5), ("repair".into(), 2)];
        let text = context(&input("startup", None), &v);
        assert!(text.contains("Tags already in use (sessions): observability (5), repair (2)."));
    }

    #[test]
    fn the_resume_reminder_appears_on_resume_only_and_can_be_switched_off() {
        let mut v = view(Some("Fix alerts"));
        v.tags = vec!["observability".into()];
        let text = context(&input("resume", None), &v);
        assert!(
            text.contains("being resumed")
                && text.contains("Fix alerts")
                && text.contains("observability")
        );

        for source in ["startup", "clear", "compact", "fork", ""] {
            assert!(
                !context(&input(source, None), &v).contains("being resumed"),
                "{source}"
            );
        }
        v.remind_on_resume = false;
        assert!(!context(&input("resume", None), &v).contains("being resumed"));

        let untitled = context(&input("resume", None), &view(None));
        assert!(untitled.contains("title (none yet) and tags (none)"));
    }

    #[test]
    fn the_context_stays_far_below_the_hook_limit_with_a_huge_vocabulary() {
        let mut v = view(Some(&"t".repeat(300)));
        v.vocabulary = (0..5000).map(|i| (format!("tag-number-{i}"), 1)).collect();
        v.tags = (0..50).map(|i| format!("tag-{i}")).collect();
        let text = context(&input("resume", None), &v);
        assert!(text.chars().count() < 5000, "{}", text.chars().count());
    }
}
