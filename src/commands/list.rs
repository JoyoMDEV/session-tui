//! `list`: print sessions, one line each or as JSON, optionally filtered.

use crate::query::{self, Filters};
use crate::store;
use anyhow::Result;
use chrono::Utc;
use std::io::Write;

/// The arguments of `list`, as typed.
pub struct Options {
    pub tickets: Vec<String>,
    pub tags: Vec<String>,
    pub branch: Option<String>,
    pub cwd: Option<String>,
    pub since: Option<String>,
    pub json: bool,
}

pub fn run(opts: Options) -> Result<()> {
    let filters = Filters {
        tickets: opts.tickets,
        tags: opts.tags,
        branch: opts.branch,
        cwd: opts.cwd.as_deref().map(query::absolute_dir).transpose()?,
        since: opts
            .since
            .as_deref()
            .map(|s| query::parse_since(s, Utc::now()))
            .transpose()?,
    };
    let sessions = store::load()?;
    let chosen: Vec<_> = sessions.iter().filter(|s| filters.matches(s)).collect();

    let mut out = std::io::stdout().lock();
    if opts.json {
        let views: Vec<_> = chosen.iter().map(|s| query::view(s)).collect();
        // A closed pipe is not an error worth reporting, as for the text output below.
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&views)?);
        return Ok(());
    }
    for s in chosen {
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
