//! `import`: register Claude Code transcripts that aren't known yet. It adds and fills in what is
//! missing, and never removes a session.

use crate::output::say;
use crate::transcript;
use anyhow::Result;

pub fn run() -> Result<()> {
    let st = transcript::import()?;
    say!(
        "{} added, {} suggestions filled, {} empty transcripts skipped",
        st.added,
        st.updated,
        st.skipped_empty
    );
    Ok(())
}
