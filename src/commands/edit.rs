//! `title`, `ticket`, `tag` and `note`: change one field of one session.

use crate::{store, tags, tickets};
use anyhow::{Context, Result, bail};
use chrono::Utc;

pub fn title(id: Option<String>, title: String) -> Result<()> {
    let title = non_empty(title)?;
    modify(id, |s| {
        s.title = Some(title);
        s.updated_at = Utc::now();
    })
}

pub fn ticket(id: Option<String>, remove: bool, keys: Vec<String>) -> Result<()> {
    // Removing stays lenient, so that a key stored by an earlier version can still be removed.
    if !remove {
        tickets::check(&keys).map_err(anyhow::Error::msg)?;
    }
    modify(id, |s| {
        for key in &keys {
            if remove {
                tickets::remove(&mut s.tickets, key);
            } else {
                tickets::add(&mut s.tickets, key);
            }
        }
    })
}

pub fn tag(id: Option<String>, remove: bool, names: Vec<String>) -> Result<()> {
    // Tags are topics. Refuse a ticket key such as ABC-123 instead of guessing, and say
    // where it belongs. Nothing is written, so the call can be repeated corrected.
    if !remove {
        let keys: Vec<&str> = names
            .iter()
            .map(|n| n.trim())
            .filter(|n| tickets::looks_like_key(n))
            .collect();
        if !keys.is_empty() {
            let keys = keys.join(" ");
            let id = id
                .as_ref()
                .map(|id| format!("--id {id} "))
                .unwrap_or_default();
            bail!(
                "{keys} looks like a ticket key, so no tags were changed. Record it as a \
                 ticket: sessions ticket {id}{keys}. Tags are topics and are written in \
                 lower case, so if it is a topic, use e.g. {}",
                keys.to_lowercase()
            );
        }
    }
    modify(id, |s| {
        for name in &names {
            if remove {
                tags::remove(&mut s.tags, name);
            } else {
                tags::add(&mut s.tags, name);
            }
        }
    })
}

pub fn note(id: Option<String>, text: String) -> Result<()> {
    let text = text.trim().to_string();
    modify(id, |s| s.note = Some(text).filter(|t| !t.is_empty()))
}

fn non_empty(s: String) -> Result<String> {
    let s = s.trim().to_string();
    if s.is_empty() {
        bail!("empty title");
    }
    Ok(s)
}

/// Applies `f` to the session with the given id (default `$CLAUDE_SESSION_ID`). An unknown id is
/// an error, so a typo doesn't create an empty entry; `sessions import` registers older sessions.
pub fn modify(id: Option<String>, f: impl FnOnce(&mut store::Session)) -> Result<()> {
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
