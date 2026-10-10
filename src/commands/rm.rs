//! `rm`: remove a session. An ID that is not known is an error.

use crate::store;
use anyhow::{Result, bail};

pub fn run(id: String) -> Result<()> {
    if !store::update(|s| {
        let n = s.len();
        s.retain(|x| x.id != id);
        s.len() != n
    })? {
        bail!("no session {id}");
    }
    Ok(())
}
