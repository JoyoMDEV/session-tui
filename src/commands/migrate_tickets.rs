//! `migrate-tickets`: move tags that look like ticket keys into the tickets field.

use crate::{store, tickets};
use anyhow::{Context, Result};
use chrono::Utc;

pub fn run(yes: bool) -> Result<()> {
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
