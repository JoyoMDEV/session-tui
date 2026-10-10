//! `list`: print sessions, one line each or as JSON, optionally filtered. It only reads.

use crate::cli::FilterArgs;
use crate::output::say;
use crate::query::TitleSource;
use crate::store::Session;
use crate::{commands, query, store};
use anyhow::Result;

pub fn run(args: FilterArgs, all: bool, json: bool) -> Result<()> {
    let filters = commands::filters(args, all)?;
    let sessions = store::load()?;
    let chosen: Vec<_> = sessions.iter().filter(|s| filters.matches(s)).collect();

    if json {
        let views: Vec<_> = chosen.iter().map(|s| query::view(s)).collect();
        say!("{}", serde_json::to_string_pretty(&views)?);
        return Ok(());
    }
    for s in chosen {
        // A closed pipe ends the listing, as for `sessions list | head`.
        if !say!("{}", text_line(s)) {
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
        // The same title as `log` and `--json`, but a mere snippet of the first prompt is marked.
        match query::display_title(s) {
            Some((TitleSource::Prompt, _)) => "(suggested)",
            Some((_, title)) => title,
            None => "-",
        },
        s.cwd,
        tickets,
        tags
    )
}
