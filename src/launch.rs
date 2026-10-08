//! Starting `claude --resume`. The browser and `sessions resume` both end here: the process is
//! replaced by `claude`, which is why the tool is Unix only.

use crate::store::Session;
use anyhow::{Error, anyhow};
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Extra arguments for `claude` from `$SESSIONS_CLAUDE_ARGS`, as one string.
pub fn default_args() -> String {
    std::env::var("SESSIONS_CLAUDE_ARGS").unwrap_or_default()
}

/// Replaces this process with `claude --resume <id>` in the session's directory. It only returns,
/// with the reason, if `claude` could not be started.
pub fn resume(session: &Session, args: &[String]) -> Error {
    let err = Command::new("claude")
        .arg("--resume")
        .arg(&session.id)
        .args(args)
        .current_dir(&session.cwd)
        .exec();
    anyhow!("could not start claude in {}: {err}", session.cwd)
}
