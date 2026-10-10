//! `migrate-tickets`: move tags that look like ticket keys into the tickets field.

use crate::output::say;
use crate::{store, tickets};
use anyhow::{Context, Result};
use chrono::Utc;

pub fn run(yes: bool) -> Result<()> {
    let plan = tickets::migration_plan(&store::load()?);
    // A closed pipe only ends the listing; what was asked for is still done.
    for m in &plan {
        let title = if m.title.is_empty() { "-" } else { &m.title };
        if !say!("{}  {}  tags -> tickets: {}", m.id, title, m.keys.join(" ")) {
            break;
        }
    }
    if plan.is_empty() {
        say!("No tags look like ticket keys.");
        return Ok(());
    }
    if !yes {
        say!(
            "{} session(s) would change; run `sessions migrate-tickets --yes` to apply",
            plan.len()
        );
        return Ok(());
    }
    let path = store::path()?;
    let backup = path.with_extension(format!("json.bak-{}", Utc::now().timestamp()));
    std::fs::copy(&path, &backup).with_context(|| format!("backing up to {}", backup.display()))?;
    let changed = store::update(|all| tickets::migrate(all))?;
    say!(
        "moved the keys of {changed} session(s); the old file is {}",
        backup.display()
    );
    Ok(())
}
