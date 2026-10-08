//! `resume`: resume the session a query points to, without the browser.

use crate::cli::FilterArgs;
use crate::query::{Filters, Searcher};
use crate::store::Session;
use crate::{commands, launch, store, transcript};
use anyhow::{Result, bail};
use std::io::Write;

pub fn run(query: Vec<String>, args: FilterArgs, list: bool) -> Result<()> {
    let filters = commands::filters(args)?;
    let sessions = store::load()?;
    let matches = find(&sessions, &filters, &query.join(" "));

    if list {
        let mut out = std::io::stdout().lock();
        for s in matches {
            // A closed pipe ends the output, as for `list`.
            if writeln!(out, "{}", commands::list::text_line(s)).is_err() {
                break;
            }
        }
        return Ok(());
    }

    let session = pick(&matches)?;
    if !transcript::existing_ids().contains(&session.id) {
        bail!(
            "the transcript of {} is gone, so it can't be resumed (see `sessions prune`)",
            session.id
        );
    }
    let extra: Vec<String> = launch::default_args()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    // Replaces this process; only returns on failure.
    Err(launch::resume(session, &extra))
}

/// The sessions that match the filters and the search, best match first, then most recent.
fn find<'a>(sessions: &'a [Session], filters: &Filters, query: &str) -> Vec<&'a Session> {
    let search = Searcher::new(query);
    let mut scored: Vec<(i64, &Session)> = sessions
        .iter()
        .filter(|s| filters.matches(s))
        .filter_map(|s| search.score(s).map(|score| (score, s)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.updated_at.cmp(&a.1.updated_at)));
    scored.into_iter().map(|(_, s)| s).collect()
}

/// The one session to resume. Several matches are an error that lists them, because guessing
/// which one was meant would resume the wrong work.
fn pick<'a>(matches: &[&'a Session]) -> Result<&'a Session> {
    match matches {
        [] => bail!("no session matches"),
        [only] => Ok(only),
        many => {
            let lines: Vec<String> = many
                .iter()
                .map(|s| format!("  {}", commands::list::text_line(s)))
                .collect();
            bail!(
                "{} sessions match, so none was resumed. Narrow the query or add a filter, \
                 or list them with --list:\n{}",
                many.len(),
                lines.join("\n")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::new_session;
    use chrono::{TimeZone, Utc};

    fn session(id: &str, title: &str, day: u32) -> Session {
        let t = Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap();
        let mut s = new_session(id, "/work/app", t, t);
        s.title = Some(title.into());
        s
    }

    fn ids(found: &[&Session]) -> Vec<String> {
        found.iter().map(|s| s.id.clone()).collect()
    }

    #[test]
    fn find_applies_filters_and_the_search_and_puts_recent_first() {
        let mut a = session("a", "Fix login", 1);
        a.tickets = vec!["ABC-1".into()];
        let mut b = session("b", "Fix logout", 5);
        b.tickets = vec!["ABC-1".into()];
        let c = session("c", "Other work", 9);
        let all = [a, b, c];

        assert_eq!(ids(&find(&all, &Filters::default(), "")), ["c", "b", "a"]);
        assert_eq!(ids(&find(&all, &Filters::default(), "fix")), ["b", "a"]);
        assert_eq!(
            ids(&find(&all, &Filters::default(), "zebra")),
            Vec::<String>::new()
        );
        let ticket = Filters {
            tickets: vec!["ABC-1".into()],
            ..Default::default()
        };
        assert_eq!(ids(&find(&all, &ticket, "")), ["b", "a"]);
    }

    #[test]
    fn pick_takes_a_unique_match_and_refuses_to_choose_between_several() {
        let (a, b) = (session("a", "Fix login", 1), session("b", "Fix logout", 5));
        assert_eq!(pick(&[&a]).unwrap().id, "a");

        let none = pick(&[]).unwrap_err().to_string();
        assert_eq!(none, "no session matches");

        let many = pick(&[&b, &a]).unwrap_err().to_string();
        assert!(
            many.starts_with("2 sessions match, so none was resumed"),
            "{many}"
        );
        assert!(many.contains("  a  ") && many.contains("  b  "), "{many}");
        assert!(many.contains("--list"), "{many}");
    }
}
