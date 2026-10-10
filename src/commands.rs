//! The small commands that read or change `sessions.json`: one file each, except the four that
//! edit a single session, which share `edit.rs`. Each takes already parsed arguments.

pub mod archive;
pub mod edit;
pub mod import;
pub mod list;
pub mod log;
pub mod migrate_tickets;
pub mod prune;
pub mod resume;
pub mod rm;
pub mod start;

use crate::cli::FilterArgs;
use crate::query::{self, Filters};
use anyhow::Result;
use chrono::Utc;

/// Turns the filter arguments as typed into filters, reading `--cwd` and `--since` on the way.
/// Archived sessions are kept only if `include_archived` is set.
pub fn filters(args: FilterArgs, include_archived: bool) -> Result<Filters> {
    Ok(Filters {
        tickets: args.tickets,
        tags: args.tags,
        branch: args.branch,
        cwd: args.cwd.as_deref().map(query::absolute_dir).transpose()?,
        since: args
            .since
            .as_deref()
            .map(|s| query::parse_since(s, Utc::now()))
            .transpose()?,
        include_archived,
    })
}
