//! `list`: print every session, or only those with all the given tickets.

use crate::{store, tickets};
use anyhow::Result;
use std::io::Write;

pub fn run(wanted: Vec<String>) -> Result<()> {
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
