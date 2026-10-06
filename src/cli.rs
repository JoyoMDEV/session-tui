//! The command line, defined with clap. This file only describes the commands; what each one does
//! is in `commands/`, `hook.rs`, `setup.rs`, `doctor.rs` and `tui.rs`.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Browse and resume saved Claude Code sessions")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
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
