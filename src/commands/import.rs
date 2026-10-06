//! `import`: register Claude Code transcripts that aren't known yet.

use crate::transcript;
use anyhow::Result;

pub fn run() -> Result<()> {
    let st = transcript::import()?;
    println!(
        "{} added, {} suggestions filled, {} empty transcripts skipped",
        st.added, st.updated, st.skipped_empty
    );
    Ok(())
}
