//! Keyboard and mouse handling: reads events, changes the `App`, and decides when the browser
//! is done and which session to launch. It does not draw: the loop only asks `draw` for each frame.

use super::Launch;
use super::app::{App, Field, Mode};
use super::draw::draw;
use crate::launch::default_args;
use crate::store;
use crate::tags;
use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    widgets::ListState,
};

/// Rows scrolled per mouse wheel notch.
const WHEEL_STEP: isize = 3;

/// Moves the selection of a list by `delta` rows without leaving it.
fn select_step(state: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        return;
    }
    let cur = state.selected().unwrap_or(0) as isize;
    state.select(Some((cur + delta).clamp(0, len as isize - 1) as usize));
}

/// Moves a scroll offset by `delta`. The last page stays full instead of scrolling the final
/// line to the top.
fn scroll_by(cur: usize, delta: isize, len: usize, page: usize) -> usize {
    (cur as isize + delta).clamp(0, len.saturating_sub(page) as isize) as usize
}

pub(super) fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<Option<Launch>> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        let key = match event::read()? {
            Event::Key(key) => key,
            Event::Mouse(m) => {
                let delta = match m.kind {
                    MouseEventKind::ScrollUp => -WHEEL_STEP,
                    MouseEventKind::ScrollDown => WHEEL_STEP,
                    _ => 0,
                };
                match &mut app.mode {
                    Mode::Browse => app.move_by(delta),
                    Mode::Preview(lines, scroll) => {
                        *scroll = scroll_by(*scroll, delta, lines.len(), app.page)
                    }
                    Mode::Tags(rows, state) => select_step(state, rows.len(), delta),
                    _ => {}
                }
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        app.status.clear();

        match &mut app.mode {
            Mode::Preview(lines, scroll) => {
                let (len, page) = (lines.len(), app.page);
                let by = |cur: usize, delta: isize| scroll_by(cur, delta, len, page);
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Browse,
                    KeyCode::Char('v') if ctrl => app.mode = Mode::Browse,
                    KeyCode::Up => *scroll = by(*scroll, -1),
                    KeyCode::Down => *scroll = by(*scroll, 1),
                    KeyCode::PageUp => *scroll = by(*scroll, -(page as isize)),
                    KeyCode::PageDown => *scroll = by(*scroll, page as isize),
                    KeyCode::Home => *scroll = 0,
                    KeyCode::End => *scroll = len.saturating_sub(page),
                    _ => {}
                }
            }
            Mode::Tags(rows, state) => {
                let (len, page) = (rows.len(), app.page as isize);
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Browse,
                    KeyCode::Char('g') if ctrl => app.mode = Mode::Browse,
                    KeyCode::Up => select_step(state, len, -1),
                    KeyCode::Down => select_step(state, len, 1),
                    KeyCode::PageUp => select_step(state, len, -page),
                    KeyCode::PageDown => select_step(state, len, page),
                    KeyCode::Home => select_step(state, len, -(len as isize)),
                    KeyCode::End => select_step(state, len, len as isize),
                    KeyCode::Enter => {
                        let tag = state
                            .selected()
                            .and_then(|i| rows.get(i))
                            .map(|(tag, _)| tag.clone());
                        app.mode = Mode::Browse;
                        if let Some(tag) = tag {
                            app.toggle_tag_filter(&tag);
                        }
                    }
                    _ => {}
                }
            }
            Mode::New(_) => {
                if let Some(launch) = app.new_key(key.code, ctrl) {
                    return Ok(Some(launch));
                }
            }
            Mode::Edit(field, buf) => match key.code {
                KeyCode::Esc => app.mode = Mode::Browse,
                KeyCode::Tab if matches!(field, Field::Tags) => {
                    let found = tags::suggestions(&tags::vocabulary(&app.sessions), buf);
                    *buf = tags::complete(buf, &found);
                }
                KeyCode::Enter => {
                    let (field, text) = (*field, buf.trim().to_string());
                    app.mode = Mode::Browse;
                    if let Field::Flags = field {
                        if let Some(i) = app.selected() {
                            let args = text.split_whitespace().map(str::to_string).collect();
                            if let Some(l) = app.launch(i, args) {
                                return Ok(Some(l));
                            }
                        }
                    } else {
                        app.apply_edit(field, text);
                    }
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if !ctrl => buf.push(c),
                _ => {}
            },
            Mode::ConfirmDelete => {
                if matches!(key.code, KeyCode::Char('y'))
                    && let Some(i) = app.selected()
                {
                    let id = app.sessions[i].id.clone();
                    if let Err(e) = store::update(|all| all.retain(|s| s.id != id)) {
                        app.status = format!("{e:#}");
                    }
                    app.reload();
                }
                app.mode = Mode::Browse;
            }
            Mode::Browse => match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if ctrl => return Ok(None),
                KeyCode::Enter => {
                    if let Some(i) = app.selected() {
                        let args = default_args()
                            .split_whitespace()
                            .map(str::to_string)
                            .collect();
                        if let Some(l) = app.launch(i, args) {
                            return Ok(Some(l));
                        }
                    }
                }
                KeyCode::Up => app.mv(-1),
                KeyCode::Down => app.mv(1),
                KeyCode::PageUp => app.move_by(-(app.page as isize)),
                KeyCode::PageDown => app.move_by(app.page as isize),
                KeyCode::Home => app.list.select(Some(0)),
                KeyCode::End => app.jump_to_end(),
                KeyCode::Char('n') if ctrl => app.open_new(),
                KeyCode::Tab => {
                    app.show_untitled = !app.show_untitled;
                    app.clamp();
                }
                KeyCode::Char('g') if ctrl => app.open_tags(),
                KeyCode::Char('a') if ctrl => {
                    app.show_archived = !app.show_archived;
                    app.clamp();
                }
                KeyCode::Char('d') if ctrl => app.toggle_archived(),
                KeyCode::Char('l') if ctrl => {
                    app.only_here = !app.only_here;
                    app.clamp();
                }
                KeyCode::Char('b') if ctrl => app.toggle_branch_filter(),
                KeyCode::Char('v') if ctrl => app.open_preview(terminal.size()?.width),
                KeyCode::Char(c @ ('r' | 't' | 'k' | 'e' | 'o')) if ctrl => {
                    let field = match c {
                        'r' => Field::Title,
                        't' => Field::Tags,
                        'k' => Field::Tickets,
                        'e' => Field::Note,
                        _ => Field::Flags,
                    };
                    if let Some(text) = app.prefill(field) {
                        app.mode = Mode::Edit(field, text);
                    }
                }
                KeyCode::Char('x') if ctrl => {
                    if app.selected().is_some() {
                        app.mode = Mode::ConfirmDelete;
                    }
                }
                KeyCode::Backspace => {
                    app.query.pop();
                    app.clamp();
                }
                KeyCode::Char(c) if !ctrl => {
                    app.query.push(c);
                    app.list.select(Some(0));
                    app.clamp();
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_by_keeps_the_last_page_full() {
        assert_eq!(scroll_by(5, 100, 20, 8), 12);
        assert_eq!(scroll_by(5, -100, 20, 8), 0);
        assert_eq!(scroll_by(0, 3, 0, 8), 0);
        assert_eq!(scroll_by(0, 3, 5, 8), 0);
    }
}
