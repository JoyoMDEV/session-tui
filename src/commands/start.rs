//! `start`: start a new Claude Code session in a directory, for tickets and with a first message.

use crate::launch::{self, NewSession};
use crate::paths;
use crate::preset::Preset;
use crate::tickets;
use anyhow::{Result, bail};
use std::path::PathBuf;

/// Checks the directory, then replaces this process with `claude`. Only returns an error. The
/// tickets and the title are recorded by the hook of the new session, see `preset`.
pub fn run(
    dir: Option<String>,
    tickets: Vec<String>,
    title: Option<String>,
    prompt: Vec<String>,
) -> Result<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = match paths::check_dir(dir.as_deref().unwrap_or("."), home.as_deref()) {
        Ok(dir) => dir,
        Err(why) => bail!("{why}"),
    };
    let preset = Preset::new(tickets, title);
    tickets::check(&preset.tickets).map_err(anyhow::Error::msg)?;
    let new = NewSession {
        dir: dir.display().to_string(),
        prompt: prompt.join(" "),
        preset,
    };
    let flags: Vec<String> = launch::default_args()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    // Replaces this process; only returns on failure.
    Err(launch::start(&new, &flags))
}
