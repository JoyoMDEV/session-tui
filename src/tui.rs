use crate::store::{self, Session};
use crate::transcript;
use anyhow::Result;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind,
    },
    layout::{Constraint, Layout},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};
use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy)]
enum Field {
    Title,
    Tags,
    Note,
    Flags,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Title => "Title",
            Field::Tags => "Tags (space-separated)",
            Field::Note => "Note",
            Field::Flags => "claude flags",
        }
    }
}

enum Mode {
    Browse,
    Edit(Field, String),
    ConfirmDelete,
}

struct App {
    sessions: Vec<Session>,
    /// Ids that still have a transcript, i.e. can actually be resumed.
    resumable: HashSet<String>,
    launch_dir: String,
    query: String,
    show_untitled: bool,
    only_here: bool,
    mode: Mode,
    list: ListState,
    /// Rows the list shows at once; set while drawing, used for PageUp/PageDown.
    page: usize,
    status: String,
}

/// Below this terminal height the details pane is dropped to leave room for the list.
const MIN_HEIGHT_FOR_DETAILS: u16 = 22;
/// Rows scrolled per mouse wheel notch.
const WHEEL_STEP: isize = 3;

/// What to run after the TUI has closed.
struct Launch {
    session: Session,
    args: Vec<String>,
}

fn default_args() -> String {
    std::env::var("SESSIONS_CLAUDE_ARGS").unwrap_or_default()
}

fn in_dir(session_cwd: &str, launch_dir: &str) -> bool {
    let below = |a: &str, b: &str| a == b || a.strip_prefix(b).is_some_and(|r| r.starts_with('/'));
    below(session_cwd, launch_dir) || below(launch_dir, session_cwd)
}

impl App {
    /// Indices into `sessions`, filtered and ordered for display.
    fn visible(&self) -> Vec<usize> {
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, usize)> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| self.show_untitled || s.title.is_some() || s.suggestion.is_some())
            .filter(|(_, s)| !self.only_here || in_dir(&s.cwd, &self.launch_dir))
            .filter_map(|(i, s)| {
                if self.query.is_empty() {
                    return Some((0, i));
                }
                let hay = format!(
                    "{} {} {} {}",
                    s.title.as_deref().or(s.suggestion.as_deref()).unwrap_or(""),
                    s.cwd,
                    s.tags.join(" "),
                    s.note.as_deref().unwrap_or("")
                );
                matcher
                    .fuzzy_match(&hay, &self.query)
                    .map(|score| (score, i))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| {
                self.sessions[b.1]
                    .updated_at
                    .cmp(&self.sessions[a.1].updated_at)
            })
        });
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn selected(&self) -> Option<usize> {
        self.visible().get(self.list.selected()?).copied()
    }

    /// Reloads from disk and keeps the cursor on the same session even if the order changed.
    fn reload(&mut self) {
        let selected_id = self.selected().map(|i| self.sessions[i].id.clone());
        match store::load() {
            Ok(s) => self.sessions = s,
            Err(e) => self.status = format!("{e:#}"),
        }
        self.resumable = transcript::existing_ids();
        let pos = selected_id.and_then(|id| {
            self.visible()
                .iter()
                .position(|&i| self.sessions[i].id == id)
        });
        if pos.is_some() {
            self.list.select(pos);
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        let n = self.visible().len();
        self.list.select(if n == 0 {
            None
        } else {
            Some(self.list.selected().unwrap_or(0).min(n - 1))
        });
    }

    fn mv(&mut self, delta: isize) {
        let n = self.visible().len() as isize;
        if n > 0 {
            let cur = self.list.selected().unwrap_or(0) as isize;
            self.list.select(Some((cur + delta).rem_euclid(n) as usize));
        }
    }

    /// Moves the cursor without wrapping around, for paging and the mouse wheel.
    fn move_by(&mut self, delta: isize) {
        let n = self.visible().len();
        if n > 0 {
            let cur = self.list.selected().unwrap_or(0) as isize;
            self.list
                .select(Some((cur + delta).clamp(0, n as isize - 1) as usize));
        }
    }

    fn jump_to_end(&mut self) {
        let n = self.visible().len();
        self.list.select(n.checked_sub(1));
    }

    /// Initial text of the edit prompt for the selected session.
    fn prefill(&self, field: Field) -> Option<String> {
        let s = &self.sessions[self.selected()?];
        Some(match field {
            Field::Title => s
                .title
                .clone()
                .or_else(|| s.suggestion.clone())
                .unwrap_or_default(),
            Field::Tags => s.tags.join(" "),
            Field::Note => s.note.clone().unwrap_or_default(),
            Field::Flags => default_args(),
        })
    }

    /// Persists an edit of the selected session. Empty titles are ignored; empty tags/notes clear them.
    fn apply_edit(&mut self, field: Field, text: String) {
        let Some(i) = self.selected() else { return };
        let id = self.sessions[i].id.clone();
        let res = store::update(|all| {
            let Some(s) = all.iter_mut().find(|s| s.id == id) else {
                return;
            };
            match field {
                Field::Title if !text.is_empty() => {
                    s.title = Some(text);
                    s.updated_at = chrono::Utc::now();
                }
                Field::Tags => {
                    let mut tags: Vec<String> = Vec::new();
                    for t in text
                        .split_whitespace()
                        .map(|t| t.trim_start_matches('#'))
                        .filter(|t| !t.is_empty())
                    {
                        if !tags.iter().any(|x| x == t) {
                            tags.push(t.to_string());
                        }
                    }
                    s.tags = tags;
                }
                Field::Note => s.note = Some(text).filter(|t| !t.is_empty()),
                _ => {}
            }
        });
        if let Err(e) = res {
            self.status = format!("{e:#}");
        }
        self.reload();
    }

    fn launch(&mut self, i: usize, args: Vec<String>) -> Option<Launch> {
        let session = self.sessions[i].clone();
        if !self.resumable.contains(&session.id) {
            self.status =
                "Transcript is gone, so this session can't be resumed (see `sessions prune`)"
                    .into();
            return None;
        }
        Some(Launch { session, args })
    }
}

pub fn run() -> Result<()> {
    // Untitled sessions get a title proposal from their first prompt, and the sort order
    // follows real activity (transcript mtime), not just session starts.
    store::update(|all| {
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
        only_here: false,
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
        let err = Command::new("claude")
            .arg("--resume")
            .arg(&session.id)
            .args(&args)
            .current_dir(&session.cwd)
            .exec();
        anyhow::bail!("could not start claude in {}: {err}", session.cwd);
    }
    Ok(())
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<Option<Launch>> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        let key = match event::read()? {
            Event::Key(key) => key,
            Event::Mouse(m) if matches!(app.mode, Mode::Browse) => {
                match m.kind {
                    MouseEventKind::ScrollUp => app.move_by(-WHEEL_STEP),
                    MouseEventKind::ScrollDown => app.move_by(WHEEL_STEP),
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
            Mode::Edit(field, buf) => match key.code {
                KeyCode::Esc => app.mode = Mode::Browse,
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
                KeyCode::Char('p') if ctrl => app.mv(-1),
                KeyCode::Char('n') if ctrl => app.mv(1),
                KeyCode::Tab => {
                    app.show_untitled = !app.show_untitled;
                    app.clamp();
                }
                KeyCode::Char('l') if ctrl => {
                    app.only_here = !app.only_here;
                    app.clamp();
                }
                KeyCode::Char(c @ ('r' | 't' | 'e' | 'o')) if ctrl => {
                    let field = match c {
                        'r' => Field::Title,
                        't' => Field::Tags,
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

fn draw(f: &mut Frame, app: &mut App) {
    let detail_height = if f.area().height >= MIN_HEIGHT_FOR_DETAILS {
        6
    } else {
        0
    };
    let [search, body, detail, help] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(detail_height),
        Constraint::Length(1),
    ])
    .areas(f.area());
    // Inner height of the bordered list.
    app.page = body.height.saturating_sub(2).max(1) as usize;

    let hidden = app
        .sessions
        .iter()
        .filter(|s| s.title.is_none() && s.suggestion.is_none())
        .count();
    let mut search_title = if app.show_untitled {
        " Sessions (all)".to_string()
    } else {
        format!(" Sessions ({hidden} empty hidden)")
    };
    if app.only_here {
        search_title += " · this directory only";
    }
    search_title.push(' ');
    f.render_widget(
        Paragraph::new(format!("> {}", app.query))
            .block(Block::default().borders(Borders::ALL).title(search_title)),
        search,
    );

    let visible = app.visible();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let s = &app.sessions[i];
            let mut spans = Vec::new();
            if !app.resumable.contains(&s.id) {
                spans.push(Span::raw("✗ ").red());
            }
            spans.push(
                Span::raw(
                    s.updated_at
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M  ")
                        .to_string(),
                )
                .dim(),
            );
            let dir = Path::new(&s.cwd)
                .file_name()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            spans.push(Span::raw(format!("{:<18.18}  ", dir)).cyan());
            spans.push(match (&s.title, &s.suggestion) {
                (Some(t), _) => Span::raw(t.clone()),
                (None, Some(sug)) => Span::raw(format!("~ {sug}")).dim().italic(),
                (None, None) => Span::raw("(untitled)").dim().italic(),
            });
            if !s.tags.is_empty() {
                spans.push(Span::raw(format!("  #{}", s.tags.join(" #"))).yellow());
            }
            Line::from(spans).into()
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, body, &mut app.list);
    if visible.len() > app.page {
        let mut scrollbar = ScrollbarState::new(visible.len())
            .viewport_content_length(app.page)
            .position(app.list.selected().unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            body,
            &mut scrollbar,
        );
    }

    let detail_text = match app.selected().map(|i| &app.sessions[i]) {
        Some(s) => format!(
            "cwd:   {}\nid:    {}\ntags:  {}\nnote:  {}",
            s.cwd,
            s.id,
            if s.tags.is_empty() {
                "-".to_string()
            } else {
                s.tags.join(" ")
            },
            s.note.as_deref().unwrap_or("-")
        ),
        None => "No session selected".to_string(),
    };
    if detail_height > 0 {
        f.render_widget(
            Paragraph::new(detail_text)
                .block(Block::default().borders(Borders::ALL).title(" Details ")),
            detail,
        );
    }

    let help_line = match &app.mode {
        Mode::Edit(field, buf) => Line::from(format!("{}: {buf}█   (Enter to confirm, Esc to cancel)", field.label())),
        Mode::ConfirmDelete => Line::from("Really delete this session? (y = yes, anything else cancels)".red()),
        Mode::Browse if !app.status.is_empty() => Line::from(app.status.clone().red()),
        Mode::Browse => Line::from(
            "Enter resume  ^O flags  Tab empty  ^L dir  ^R title  ^T tags  ^E note  ^X delete  PgUp/PgDn  Esc quit".dim(),
        ),
    };
    f.render_widget(Paragraph::new(help_line), help);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(n: usize) -> App {
        let now = chrono::Utc::now();
        let sessions = (0..n)
            .map(|i| {
                let mut s = store::new_session(&format!("id{i}"), "/x", now, now);
                s.title = Some(format!("t{i}"));
                s
            })
            .collect();
        let mut app = App {
            sessions,
            resumable: HashSet::new(),
            launch_dir: String::new(),
            query: String::new(),
            show_untitled: false,
            only_here: false,
            mode: Mode::Browse,
            list: ListState::default(),
            page: 10,
            status: String::new(),
        };
        app.clamp();
        app
    }

    #[test]
    fn paging_clamps_at_both_ends_instead_of_wrapping() {
        let mut app = app_with(25);
        app.move_by(10);
        assert_eq!(app.list.selected(), Some(10));
        app.move_by(100);
        assert_eq!(app.list.selected(), Some(24));
        app.move_by(-100);
        assert_eq!(app.list.selected(), Some(0));
        app.jump_to_end();
        assert_eq!(app.list.selected(), Some(24));
    }

    #[test]
    fn paging_an_empty_list_keeps_nothing_selected() {
        let mut app = app_with(0);
        app.move_by(10);
        app.jump_to_end();
        assert_eq!(app.list.selected(), None);
    }

    #[test]
    fn in_dir_matches_same_parent_and_child_but_not_siblings() {
        assert!(in_dir("/a/b", "/a/b"));
        assert!(in_dir("/a/b/c", "/a/b"));
        assert!(in_dir("/a/b", "/a/b/c"));
        assert!(!in_dir("/a/bc", "/a/b"));
        assert!(!in_dir("/a/x", "/a/b"));
    }
}
