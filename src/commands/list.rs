//! `list`: print sessions, one line each or as JSON, optionally filtered.

use crate::cli::FilterArgs;
use crate::{commands, query, store};
use anyhow::Result;
use std::io::Write;

pub fn run(args: FilterArgs, json: bool) -> Result<()> {
    let filters = commands::filters(args)?;
    let sessions = store::load()?;
    let chosen: Vec<_> = sessions.iter().filter(|s| filters.matches(s)).collect();

    let mut out = std::io::stdout().lock();
    if json {
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
