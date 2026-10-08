//! `log`: a timeline of the matching sessions, newest first, as text or as a Markdown list.

use crate::cli::FilterArgs;
use crate::query::{self, Filters};
use crate::store::Session;
use crate::{commands, store};
use anyhow::Result;
use std::io::Write;

pub fn run(args: FilterArgs, markdown: bool) -> Result<()> {
    let filters = commands::filters(args)?;
    let sessions = store::load()?;
    let mut out = std::io::stdout().lock();
    for s in timeline(&sessions, &filters) {
        let line = if markdown {
            markdown_line(s)
        } else {
            text_line(s)
        };
        // `println!` panics on a closed pipe, e.g. `sessions log | head`.
        if writeln!(out, "{line}").is_err() {
            break;
        }
    }
    Ok(())
}

/// The matching sessions, newest first by the day they were started.
fn timeline<'a>(sessions: &'a [Session], filters: &Filters) -> Vec<&'a Session> {
    let mut chosen: Vec<_> = sessions.iter().filter(|s| filters.matches(s)).collect();
    chosen.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    chosen
}

fn date(s: &Session) -> String {
    s.created_at.format("%Y-%m-%d").to_string()
}

fn title(s: &Session) -> &str {
    query::display_title(s).map_or("(untitled)", |(_, title)| title)
}

/// `2026-10-06  Fix login  feat/login  PR #558  /work/app`, leaving out what is not known.
fn text_line(s: &Session) -> String {
    let mut parts = vec![date(s), title(s).to_string()];
    parts.extend(s.branch.clone());
    parts.extend(
        s.pr_url
            .as_deref()
            .map(|u| format!("PR {}", query::pr_label(u))),
    );
    parts.push(s.cwd.clone());
    parts.join("  ")
}

/// `- **2026-10-06** Fix login (`feat/login`, [PR #558](url), `/work/app`)`
fn markdown_line(s: &Session) -> String {
    let mut details = Vec::new();
    details.extend(s.branch.as_deref().map(code));
    details.extend(s.pr_url.as_deref().map(|url| {
        let label = query::pr_label(url);
        let text = if label.starts_with('#') {
            format!("PR {label}")
        } else {
            "PR".to_string()
        };
        format!("[{text}]({})", url.replace(')', "%29").replace(' ', "%20"))
    }));
    details.push(code(&s.cwd));
    format!(
        "- **{}** {} ({})",
        date(s),
        escape(title(s)),
        details.join(", ")
    )
}

/// An inline code span. A backtick in the text would end it, so it becomes an apostrophe.
fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

/// Backslash-escapes the characters that would turn text into Markdown formatting.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\`*_[]<>|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::new_session;
    use chrono::{TimeZone, Utc};

    fn session(id: &str, day: u32) -> Session {
        let t = Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap();
        let mut s = new_session(id, "/work/app", t, t);
        s.title = Some("Fix login".into());
        s
    }

    #[test]
    fn the_timeline_is_newest_first_and_filtered() {
        let mut late = session("late", 9);
        late.tags = vec!["auth".into()];
        let sessions = vec![late, session("early", 2), session("mid", 5)];
        let ids = |filters: &Filters| -> Vec<_> {
            timeline(&sessions, filters)
                .iter()
                .map(|s| s.id.clone())
                .collect()
        };
        assert_eq!(ids(&Filters::default()), ["late", "mid", "early"]);
        let auth = Filters {
            tags: vec!["auth".into()],
            ..Default::default()
        };
        assert_eq!(ids(&auth), ["late"]);
    }

    #[test]
    fn a_text_line_leaves_out_what_is_not_known() {
        let mut s = session("a", 6);
        assert_eq!(text_line(&s), "2026-10-06  Fix login  /work/app");
        s.branch = Some("feat/login".into());
        s.pr_url = Some("https://github.com/o/r/pull/558".into());
        assert_eq!(
            text_line(&s),
            "2026-10-06  Fix login  feat/login  PR #558  /work/app"
        );
        s.title = None;
        assert!(text_line(&s).starts_with("2026-10-06  (untitled)  "));
    }

    #[test]
    fn a_markdown_line_links_the_pull_request_and_quotes_paths() {
        let mut s = session("a", 6);
        s.branch = Some("feat/login".into());
        s.pr_url = Some("https://github.com/o/r/pull/558".into());
        assert_eq!(
            markdown_line(&s),
            "- **2026-10-06** Fix login (`feat/login`, [PR #558](https://github.com/o/r/pull/558), `/work/app`)"
        );
        s.pr_url = Some("https://example.test/merge/a b".into());
        assert!(markdown_line(&s).contains("[PR](https://example.test/merge/a%20b)"));
    }

    #[test]
    fn markdown_formatting_characters_in_a_title_are_escaped() {
        let mut s = session("a", 6);
        s.title = Some("Fix *login* [wip] <b> `x` a_b".into());
        s.cwd = "/work/it`s".into();
        let line = markdown_line(&s);
        assert!(
            line.contains(r"Fix \*login\* \[wip\] \<b\> \`x\` a\_b"),
            "{line}"
        );
        assert!(line.contains("`/work/it's`"), "{line}");
    }
}
