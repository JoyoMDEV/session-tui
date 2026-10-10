//! Drawing: turns an `App` into ratatui widgets. Nothing here changes what is stored.

use super::app::{App, Field, Mode, NewField, NewForm, TitleKind, shown_title};
use crate::query::Searcher;
use crate::query::pr_label;
use crate::transcript;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use std::path::Path;

/// Below this terminal height the details pane is dropped to leave room for the list.
const MIN_HEIGHT_FOR_DETAILS: u16 = 25;
/// Tag suggestions shown next to the tag editor.
const MAX_SUGGESTIONS: usize = 5;
/// Directory suggestions shown under the directory field of the new session dialog.
const MAX_NEW_SUGGESTIONS: usize = 5;
/// Border plus seven lines of details.
const DETAILS_HEIGHT: u16 = 9;

/// The tag view: a centered box over the list with every tag and its number of sessions. A tag
/// that is already a `#tag` word in the search box carries a check mark.
fn draw_tags(f: &mut Frame, rows: &[(String, usize)], state: &mut ListState, active: &[String]) {
    let area = f.area();
    let width = area.width.min(54);
    let height = (rows.len().max(1) as u16 + 2).clamp(5, area.height.saturating_sub(2).max(5));
    let box_area = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height: height.min(area.height),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Tags ({}) ", rows.len()));
    f.render_widget(Clear, box_area);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("No tags yet. ^T adds some to a session.").block(block),
            box_area,
        );
        return;
    }
    let items: Vec<ListItem> = rows
        .iter()
        .map(|(tag, n)| {
            let mark = if active.contains(tag) { "✓ " } else { "  " };
            Line::from(vec![
                Span::raw(format!("{mark}{tag}")),
                Span::raw(format!("  ({n})")).dim(),
            ])
            .into()
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, box_area, state);
}

/// `text`, or if it is longer than `room` characters its end behind an ellipsis: the end of a
/// path or of something being typed is the part that matters.
fn tail_fit(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let tail: String = text.chars().rev().take(room.saturating_sub(1)).collect();
    format!("…{}", tail.chars().rev().collect::<String>())
}

/// The new session dialog: a centered box with the four fields, the directories that could finish
/// the one being typed, and the reason a start was refused.
fn draw_new(f: &mut Frame, form: &NewForm) {
    let area = f.area();
    let width = area.width.min(90);
    let label = 11;
    let room = (width as usize).saturating_sub(label + 4).max(1);
    let mut lines: Vec<Line> = vec![Line::from("")];
    for field in NewField::ALL {
        let value = form.value(field);
        let cursor = if field == form.focus { "█" } else { "" };
        let shown = tail_fit(value, room.saturating_sub(cursor.chars().count()));
        let name = format!("{:<label$}", field.label());
        let name = if field == form.focus {
            name.bold()
        } else {
            name.dim()
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            name,
            Span::raw(format!("{shown}{cursor}")),
        ]));
        if field == NewField::Dir {
            for c in form.suggestions.iter().take(MAX_NEW_SUGGESTIONS) {
                let mark = match (c.git, c.used) {
                    (true, _) => "⎇ ",
                    (false, true) => "· ",
                    (false, false) => "  ",
                };
                let text = tail_fit(&c.text, room.saturating_sub(2));
                lines.push(Line::from(format!("   {mark}{text}")).dim());
            }
            if form.suggestions.len() > MAX_NEW_SUGGESTIONS {
                let more = form.suggestions.len() - MAX_NEW_SUGGESTIONS;
                lines.push(Line::from(format!("     … {more} more")).dim());
            }
        }
    }
    lines.push(Line::from(""));
    if let Some(why) = &form.error {
        lines.push(Line::from(format!(" {}", tail_fit(why, room + label))).red());
    } else {
        lines.push(Line::from(" ⎇ git repository   · used before").dim());
    }
    let height = (lines.len() as u16 + 2).min(area.height);
    let box_area = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, box_area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" New session "),
        ),
        box_area,
    );
}

/// `1.5 MB`, `300 KB`, `12 B`.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let (mut value, mut unit) = (bytes as f64, 0);
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(super) fn draw(f: &mut Frame, app: &mut App) {
    let detail_height = if f.area().height >= MIN_HEIGHT_FOR_DETAILS {
        DETAILS_HEIGHT
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
        .filter(|s| shown_title(s).0 == TitleKind::Missing)
        .count();
    let mut search_title = if app.show_untitled {
        " Sessions (all)".to_string()
    } else {
        format!(" Sessions ({hidden} empty hidden)")
    };
    let archived = app.sessions.iter().filter(|s| s.archived).count();
    if app.show_archived {
        search_title += " · archived shown";
    } else if archived > 0 {
        search_title += &format!(" · {archived} archived hidden");
    }
    if app.only_here {
        search_title += " · this directory only";
    }
    if let Some(branch) = &app.only_branch {
        search_title += &format!(" · branch {branch}");
    }
    let words = Searcher::new(&app.query);
    if !words.tickets().is_empty() {
        search_title += &format!(" · ticket {}", words.tickets().join(", "));
    }
    if !words.tags().is_empty() {
        search_title += &format!(" · tag {}", words.tags().join(", "));
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
            let (kind, title) = shown_title(s);
            spans.push(match kind {
                TitleKind::Own => Span::raw(title.to_string()),
                TitleKind::Native => Span::raw(title.to_string()).italic(),
                TitleKind::Suggestion => Span::raw(format!("~ {title}")).dim().italic(),
                TitleKind::Missing => Span::raw("(untitled)").dim().italic(),
            });
            if !s.tickets.is_empty() {
                spans.push(Span::raw(format!("  [{}]", s.tickets.join(" "))).magenta());
            }
            if !s.tags.is_empty() {
                spans.push(Span::raw(format!("  #{}", s.tags.join(" #"))).yellow());
            }
            if let Some(branch) = &s.branch {
                spans.push(Span::raw(format!("  ⎇ {branch}")).dim());
            }
            if s.archived {
                spans.push(Span::raw("  archived").dim().italic());
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
        Some(s) => {
            let git = match (&s.branch, &s.pr_url) {
                (None, None) => "-".to_string(),
                (b, pr) => [
                    b.clone(),
                    pr.as_deref().map(|u| format!("PR {} ({u})", pr_label(u))),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("  ·  "),
            };
            let size = transcript::find(&s.id)
                .and_then(|p| std::fs::metadata(p).ok())
                .map_or("transcript gone".to_string(), |m| human_size(m.len()));
            let title_from = match shown_title(s).0 {
                TitleKind::Own => "title set by you or the agent",
                TitleKind::Native => "title generated by Claude Code",
                TitleKind::Suggestion => "title cut from the first prompt",
                TitleKind::Missing => "no title",
            };
            let archived = if s.archived { "  ·  archived" } else { "" };
            format!(
                "cwd:     {}\nid:      {}\ngit:     {git}\ntickets: {}\ntags:    {}\nnote:    {}\nfile:    {size}  ·  {title_from}{archived}",
                s.cwd,
                s.id,
                if s.tickets.is_empty() {
                    "-".to_string()
                } else {
                    s.tickets.join(" ")
                },
                if s.tags.is_empty() {
                    "-".to_string()
                } else {
                    s.tags.join(" ")
                },
                s.note.as_deref().unwrap_or("-")
            )
        }
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
        Mode::Edit(field, buf) => {
            let mut text = format!("{}: {buf}█   (Enter to confirm, Esc to cancel)", field.label());
            if matches!(field, Field::Tags) {
                let found = app.tag_suggestions(buf);
                if !found.is_empty() {
                    let shown: Vec<_> = found.iter().take(MAX_SUGGESTIONS).cloned().collect();
                    text += &format!("  Tab: {}", shown.join(" "));
                }
            }
            Line::from(text)
        }
        Mode::New(_) => Line::from(
            "Tab completes the directory  ↑↓ Enter field  ^U clears  Enter in Title starts  Esc cancels".dim(),
        ),
        Mode::Tags(..) => Line::from(
            "↑↓ select  Enter filters by the tag (again: removes the filter)  Esc closes".dim(),
        ),
        Mode::ConfirmDelete => Line::from("Really delete this session? (y = yes, anything else cancels)".red()),
        Mode::Preview(..) => Line::from("".dim()),
        Mode::Browse if !app.status.is_empty() => Line::from(app.status.clone().red()),
        Mode::Browse => Line::from(
            "Enter resume  ^N new  ^V preview  ^O flags  Tab empty  ^G tags  ^A archived  ^D archive  ^L dir  ^B branch  ^R title  ^T tags  ^K tickets  ^E note  ^X delete  Esc quit".dim(),
        ),
    };
    f.render_widget(Paragraph::new(help_line), help);

    // The preview covers everything, so it is drawn last. It pages by its own height.
    let preview_page = f.area().height.saturating_sub(2).max(1) as usize;
    let mut previewing = false;
    if let Mode::Preview(lines, scroll) = &app.mode {
        previewing = true;
        let area = f.area();
        let end = (*scroll + preview_page).min(lines.len());
        let text: Vec<Line> = lines[*scroll..end]
            .iter()
            .map(|l| match l.as_str() {
                "── you ──" => Line::from(l.as_str()).yellow().bold(),
                "── claude ──" => Line::from(l.as_str()).green().bold(),
                _ => Line::from(l.as_str()),
            })
            .collect();
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(format!(
                " Preview · {}/{} · ↑↓ PgUp PgDn Home End scroll · Esc closes ",
                (*scroll + 1).min(lines.len()),
                lines.len()
            ))),
            area,
        );
        if lines.len() > preview_page {
            let mut bar = ScrollbarState::new(lines.len())
                .viewport_content_length(preview_page)
                .position(*scroll);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area,
                &mut bar,
            );
        }
    }
    if previewing {
        app.page = preview_page;
    }
    if let Mode::Tags(rows, state) = &mut app.mode {
        draw_tags(f, rows, state, words.tags());
    }
    if let Mode::New(form) = &app.mode {
        draw_new(f, form);
    }
}

#[cfg(test)]
mod tests {
    use super::super::app::app_with;
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// Draws `app` on a terminal of the given size and returns the screen, one string per row.
    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn the_list_shows_every_title_and_the_details_pane_on_a_tall_terminal() {
        let mut app = app_with(3);
        app.sessions[0].tags = vec!["observability".into()];
        let screen = render(&mut app, 80, 30).join("\n");
        for title in ["t0", "t1", "t2"] {
            assert!(screen.contains(title), "{title} missing:\n{screen}");
        }
        assert!(
            screen.contains("observability"),
            "details missing:\n{screen}"
        );
    }

    #[test]
    fn a_short_terminal_drops_the_details_pane_and_keeps_the_list() {
        let mut app = app_with(3);
        app.sessions[0].tags = vec!["observability".into()];
        let screen = render(&mut app, 80, MIN_HEIGHT_FOR_DETAILS - 1).join("\n");
        assert!(screen.contains("t0"), "{screen}");
        assert!(!screen.contains("title set by you"), "{screen}");
    }

    #[test]
    fn the_tag_view_lists_tags_with_counts_and_marks_the_active_filter() {
        let mut app = app_with(3);
        app.sessions[0].tags = vec!["auth".into(), "repair".into()];
        app.sessions[1].tags = vec!["auth".into()];
        app.query = "#repair ".into();
        app.open_tags();
        let screen = render(&mut app, 80, 30).join("\n");
        assert!(screen.contains("auth  (2)"), "{screen}");
        assert!(screen.contains("✓ repair  (1)"), "{screen}");
        assert!(screen.contains("Tags (2)"), "{screen}");
        assert!(
            screen.contains("tag repair"),
            "the filter shows in the title bar:\n{screen}"
        );
    }

    #[test]
    fn the_new_session_dialog_shows_the_fields_the_suggestions_and_the_help() {
        let root = std::env::temp_dir().join(format!("sessions-draw-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Code/app/.git")).unwrap();
        std::fs::create_dir_all(root.join("Code/api")).unwrap();
        let root = root.canonicalize().unwrap();
        let mut app = app_with(1);
        app.launch_dir = format!("{}/Code/ap", root.display());
        app.open_new();
        let screen = render(&mut app, 100, 30).join("\n");
        for want in [
            "New session",
            "Directory",
            "Message",
            "Tickets",
            "Title",
            "Tab completes the directory",
        ] {
            assert!(screen.contains(want), "{want} missing:\n{screen}");
        }
        let has = |marker: &str, tail: &str| {
            screen
                .lines()
                .any(|l| l.contains(marker) && l.contains(tail))
        };
        assert!(
            has("⎇ ", "Code/app/"),
            "the repository is marked:\n{screen}"
        );
        assert!(
            has("   ", "Code/api/") && !has("⎇ ", "Code/api/"),
            "{screen}"
        );
    }

    #[test]
    fn the_new_session_dialog_shows_why_a_start_was_refused() {
        let mut app = app_with(1);
        app.launch_dir = "/no/such/place".into();
        app.open_new();
        if let Mode::New(form) = &mut app.mode {
            form.focus = NewField::Title;
        }
        app.new_key(ratatui::crossterm::event::KeyCode::Enter, false);
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(
            screen.contains("No such directory: /no/such/place"),
            "{screen}"
        );
    }

    #[test]
    fn a_long_error_in_the_dialog_keeps_the_end_of_the_path() {
        let mut app = app_with(1);
        app.launch_dir = format!("/{}end", "dir/".repeat(40));
        app.open_new();
        if let Mode::New(form) = &mut app.mode {
            form.focus = NewField::Title;
        }
        app.new_key(ratatui::crossterm::event::KeyCode::Enter, false);
        let screen = render(&mut app, 60, 30).join("\n");
        assert!(screen.contains("…"), "{screen}");
        assert!(
            screen.contains("dir/end"),
            "the end of the path is shown:\n{screen}"
        );
    }

    #[test]
    fn a_long_value_in_the_dialog_shows_its_end() {
        let mut app = app_with(1);
        app.launch_dir = format!("/{}", "a/".repeat(60));
        app.open_new();
        let screen = render(&mut app, 60, 30).join("\n");
        assert!(screen.contains("…"), "{screen}");
        assert!(
            screen.contains("a/█"),
            "the cursor stays visible:\n{screen}"
        );
    }

    #[test]
    fn the_tag_view_says_so_when_there_are_no_tags() {
        let mut app = app_with(2);
        app.open_tags();
        let screen = render(&mut app, 80, 30).join("\n");
        assert!(screen.contains("No tags yet"), "{screen}");
    }

    #[test]
    fn the_tag_editor_offers_existing_tags() {
        let mut app = app_with(2);
        app.sessions[0].tags = vec!["observability".into(), "observer".into()];
        app.mode = Mode::Edit(Field::Tags, "obs".into());
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("Tab: observability observer"), "{screen}");
    }

    #[test]
    fn drawing_records_how_many_rows_a_page_has() {
        let mut app = app_with(3);
        render(&mut app, 80, 30);
        // Terminal height minus search box (3), details (9), help line (1) and list border (2).
        assert_eq!(app.page, 30 - 3 - DETAILS_HEIGHT as usize - 1 - 2);
    }

    #[test]
    fn human_size_is_rounded_to_the_largest_unit() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
