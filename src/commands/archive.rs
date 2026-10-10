//! `archive` and `unarchive`: move a finished session out of the default list, or back. They change
//! the flag only, never the time of the last activity.

use super::edit::modify;
use anyhow::Result;

/// Sets whether the session is archived. Archiving is not activity, so `updated_at` is left alone
/// and the timeline keeps its place.
pub fn run(id: String, archived: bool) -> Result<()> {
    modify(Some(id), |s| s.archived = archived)
}
