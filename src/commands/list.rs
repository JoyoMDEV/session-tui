//! `list`: print sessions, one line each or as JSON, optionally filtered.

use crate::cli::FilterArgs;
use crate::store::Session;
use crate::{commands, query, store};
use anyhow::Result;
use std::io::Write;

pub fn run(args: FilterArgs, all: bool, json: bool) -> Result<()> {
    let filters = commands::filters(args, all)?;
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
        // `println!` panics on a closed pipe, e.g. `sessions list | head`.
        if writeln!(out, "{}", text_line(s)).is_err() {
            break;
        }
    }
    Ok(())
}

/// `<id>  <updated>  <title>  <directory>  [tickets]  #tags`, the line `list` prints.
pub fn text_line(s: &Session) -> String {
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
    format!(
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
    )
}
