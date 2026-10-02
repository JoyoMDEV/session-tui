mod store;
mod transcript;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::io::{Read, Write};

#[derive(Parser)]
#[command(about = "Browse and resume saved Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// SessionStart hook: reads hook JSON from stdin, registers the session, injects its id
    Hook,
    /// Set the title of a session (defaults to $CLAUDE_SESSION_ID)
    Title {
        #[arg(long)]
        id: Option<String>,
        title: String,
    },
    /// Add tags to a session (defaults to $CLAUDE_SESSION_ID), e.g. a ticket key
    Tag {
        #[arg(long)]
        id: Option<String>,
        #[arg(required = true)]
        tags: Vec<String>,
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
    List,
}

#[derive(Deserialize)]
struct HookInput {
    session_id: String,
    #[serde(default)]
    cwd: String,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Some(Cmd::Hook) => hook(),
        Some(Cmd::Title { id, title }) => {
            let title = non_empty(title)?;
            modify(id, |s| {
                s.title = Some(title);
                s.updated_at = Utc::now();
            })
        }
        Some(Cmd::Tag { id, tags }) => modify(id, |s| {
            for t in tags
                .iter()
                .map(|t| t.trim_start_matches('#'))
                .filter(|t| !t.is_empty())
            {
                if !s.tags.iter().any(|x| x == t) {
                    s.tags.push(t.to_string());
                }
            }
        }),
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
        None | Some(Cmd::List) => {
            for s in store::load()? {
                let tags = if s.tags.is_empty() {
                    String::new()
                } else {
                    format!("  #{}", s.tags.join(" #"))
                };
                println!(
                    "{}  {}  {}  {}{}",
                    s.id,
                    s.updated_at.format("%Y-%m-%d %H:%M"),
                    s.title
                        .as_deref()
                        .or(s.suggestion.as_deref().map(|_| "(suggested)"))
                        .unwrap_or("-"),
                    s.cwd,
                    tags
                );
            }
            Ok(())
        }
    }
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

    store::update(|sessions| {
        store::upsert(sessions, &h.session_id, &h.cwd);
        if let Some(s) = sessions.iter_mut().find(|s| s.id == h.session_id) {
            transcript::fill_suggestion(s);
        }
    })?;

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

    let context = format!(
        "Session ID: {id} (also in $CLAUDE_SESSION_ID). Once the topic of this session is clear, \
         give it a short, descriptive title exactly once: {exe} title --id {id} \"<title>\". \
         Only update it if the topic changes fundamentally. If the work belongs to a ticket or \
         issue, add its key as a tag: {exe} tag --id {id} <KEY>.",
        id = h.session_id,
        exe = exe.display()
    );
    println!(
        "{}",
        serde_json::json!({
            "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": context }
        })
    );
    Ok(())
}

/// Applies `f` to the session with the given id (default `$CLAUDE_SESSION_ID`), creating it if needed.
fn modify(id: Option<String>, f: impl FnOnce(&mut store::Session)) -> Result<()> {
    let id = id
        .or_else(|| std::env::var("CLAUDE_SESSION_ID").ok())
        .context("no --id given and $CLAUDE_SESSION_ID is not set")?;
    store::update(|sessions| {
        if !sessions.iter().any(|s| s.id == id) {
            // Hook may not have run (e.g. session predates the hook); cwd is the best we have.
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            store::upsert(sessions, &id, &cwd);
        }
        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
            f(s);
        }
    })
}

fn prune(yes: bool) -> Result<()> {
    let existing = transcript::existing_ids();
    // A session that just started has no transcript until the first message is sent.
    let grace = Utc::now() - Duration::hours(1);
    let orphan = |s: &store::Session| !existing.contains(&s.id) && s.updated_at < grace;

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
