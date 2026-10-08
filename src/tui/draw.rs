//! Drawing: turns an `App` into ratatui widgets. Nothing here changes what is stored.

use super::app::{App, Mode, TitleKind, shown_title};
use crate::query::pr_label;
use crate::{tickets, transcript};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};
use std::path::Path;

/// Below this terminal height the details pane is dropped to leave room for the list.
const MIN_HEIGHT_FOR_DETAILS: u16 = 25;
/// Border plus seven lines of details.
const DETAILS_HEIGHT: u16 = 9;

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
    if app.only_here {
        search_title += " · this directory only";
    }
    if let Some(branch) = &app.only_branch {
        search_title += &format!(" · branch {branch}");
    }
    let wanted = tickets::parse_query(&app.query).1;
    if !wanted.is_empty() {
        search_title += &format!(" · ticket {}", wanted.join(", "));
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
            format!(
                "cwd:     {}\nid:      {}\ngit:     {git}\ntickets: {}\ntags:    {}\nnote:    {}\nfile:    {size}  ·  {title_from}",
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
        Mode::Edit(field, buf) => Line::from(format!("{}: {buf}█   (Enter to confirm, Esc to cancel)", field.label())),
        Mode::ConfirmDelete => Line::from("Really delete this session? (y = yes, anything else cancels)".red()),
        Mode::Preview(..) => Line::from("".dim()),
        Mode::Browse if !app.status.is_empty() => Line::from(app.status.clone().red()),
        Mode::Browse => Line::from(
            "Enter resume  ^V preview  ^O flags  Tab empty  ^L dir  ^B branch  ^R title  ^T tags  ^K tickets  ^E note  ^X delete  Esc quit".dim(),
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
