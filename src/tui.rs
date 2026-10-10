//! The ratatui browser. `app` holds the state and handles changes without a terminal, `input`
//! reads keys and the mouse, `draw` renders. This file starts and stops the terminal and, when the
//! user picks a session, replaces the process with `claude --resume`.

mod app;
mod draw;
mod input;

use crate::store::{self, Session};
use crate::{launch, transcript};
use anyhow::Result;
use app::{App, Mode};
use input::event_loop;
use ratatui::{
    crossterm::event::{DisableMouseCapture, EnableMouseCapture},
    widgets::ListState,
};
use std::collections::HashSet;

/// What to run after the TUI has closed.
pub(super) struct Launch {
    pub(super) session: Session,
    pub(super) args: Vec<String>,
}

pub fn run() -> Result<()> {
    // Pull in what Claude Code recorded itself (generated title, branch, PR), give untitled
    // sessions a fallback title from their first prompt, and sort by real activity (transcript
    // mtime) rather than just session starts.
    store::update(|all| {
        transcript::refresh_meta(all);
        for s in all.iter_mut() {
            transcript::fill_suggestion(s);
        }
        transcript::sync_activity(all);
    })?;

    let mut app = App {
        sessions: Vec::new(),
        resumable: HashSet::new(),
        launch_dir: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        query: String::new(),
        show_untitled: false,
        show_archived: false,
        only_here: false,
        only_branch: None,
        mode: Mode::Browse,
        list: ListState::default(),
        page: 10,
        status: String::new(),
    };
    app.sessions = store::load()?;
    app.resumable = transcript::existing_ids();
    app.clamp();

    let mut terminal = ratatui::init();
    // Without mouse capture the wheel only scrolls if the terminal maps it to arrow keys.
    // Capturing it makes plain text selection need Shift (Option in some terminals).
    let _ = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = event_loop(&mut terminal, &mut app);
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    if let Some(Launch { session, args }) = result? {
        // Replaces this process; only returns on failure.
        return Err(launch::resume(&session, &args));
    }
    Ok(())
}
