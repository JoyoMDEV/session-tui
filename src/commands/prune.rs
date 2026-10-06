//! `prune`: remove sessions whose transcript is gone. A dry run unless confirmed.

use crate::{store, transcript};
use anyhow::Result;

pub fn run(yes: bool) -> Result<()> {
    let existing = transcript::existing_ids();
    let orphan = |s: &store::Session| transcript::is_orphan(s, &existing);

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
