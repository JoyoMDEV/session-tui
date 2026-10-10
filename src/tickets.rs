//! Ticket keys (`ABC-123`) on sessions: editing and the search syntax. Tickets are references to an issue tracker; tags are topics, and the two stay apart.

use crate::store::Session;

/// A key as the user typed it, trimmed. A key is not empty and contains neither whitespace nor a
/// comma, the two ways the browser's ticket editor separates keys.
pub fn normalize(key: &str) -> Option<String> {
    let key = key.trim();
    (!key.is_empty() && !key.contains(|c: char| c.is_whitespace() || c == ','))
        .then(|| key.to_string())
}

/// Whether every one of `keys` can be a ticket key, or why not. Several keys on the command line
/// are separate arguments, so a space or a comma inside one is a mistake to report, not something
/// to split or drop.
pub fn check(keys: &[String]) -> Result<(), String> {
    match keys.iter().find(|k| normalize(k).is_none()) {
        None => Ok(()),
        Some(bad) => Err(format!(
            "{bad:?} is not a ticket key: a key has no spaces or commas. Give each ticket as its own argument"
        )),
    }
}

pub fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Adds `key` unless an equal one (ignoring case) is already there. Returns whether it was added.
pub fn add(list: &mut Vec<String>, key: &str) -> bool {
    let Some(key) = normalize(key) else {
        return false;
    };
    if list.iter().any(|k| same(k, &key)) {
        return false;
    }
    list.push(key);
    true
}

/// Removes `key` (ignoring case). Returns whether something was removed.
pub fn remove(list: &mut Vec<String>, key: &str) -> bool {
    let before = list.len();
    list.retain(|k| !same(k, key.trim()));
    list.len() != before
}

/// Replaces the list with the keys in `text`, separated by spaces or commas, without duplicates.
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in text.split(|c: char| c.is_whitespace() || c == ',') {
        add(&mut out, key);
    }
    out
}

/// `ABC-123`: 2 to 10 capitals or digits starting with a capital, a dash, and digits. Only used to
/// recognise keys that nobody typed as such (tags), so it is deliberately strict:
/// `utf-8` and `gpt-4` must not count.
pub fn looks_like_key(s: &str) -> bool {
    let Some((prefix, number)) = s.split_once('-') else {
        return false;
    };
    (2..=10).contains(&prefix.len())
        && prefix.starts_with(|c: char| c.is_ascii_uppercase())
        && prefix
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Splits a search query into the free text and the exact `ticket:KEY` filters in it.
pub fn parse_query(query: &str) -> (String, Vec<String>) {
    let mut text = Vec::new();
    let mut filters = Vec::new();
    for word in query.split_whitespace() {
        match word.get(..7).filter(|p| p.eq_ignore_ascii_case("ticket:")) {
            Some(_) => {
                let key = &word[7..];
                if !key.is_empty() {
                    filters.push(key.to_string());
                }
            }
            None => text.push(word),
        }
    }
    (text.join(" "), filters)
}

/// Whether the session has every filtered ticket, ignoring case. An empty filter matches all.
pub fn has_all(session: &Session, filters: &[String]) -> bool {
    filters
        .iter()
        .all(|f| session.tickets.iter().any(|t| same(t, f)))
}

/// What `migrate` would move: tags that look like ticket keys.
pub struct Move {
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
}

pub fn migration_plan(sessions: &[Session]) -> Vec<Move> {
    sessions
        .iter()
        .filter_map(|s| {
            let keys: Vec<String> = s
                .tags
                .iter()
                .filter(|t| looks_like_key(t))
                .cloned()
                .collect();
            (!keys.is_empty()).then(|| Move {
                id: s.id.clone(),
                title: s.title.clone().unwrap_or_default(),
                keys,
            })
        })
        .collect()
}

/// Moves the tags that look like ticket keys into `tickets`. Returns how many sessions changed.
pub fn migrate(sessions: &mut [Session]) -> usize {
    let mut changed = 0;
    for s in sessions.iter_mut() {
        let (keys, rest): (Vec<String>, Vec<String>) =
            s.tags.drain(..).partition(|t| looks_like_key(t));
        s.tags = rest;
        if keys.is_empty() {
            continue;
        }
        for key in &keys {
            add(&mut s.tickets, key);
        }
        changed += 1;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_has_neither_whitespace_nor_a_comma() {
        for bad in ["A B", "A,B", "ABC-1,", " ", "", "A\tB", "ABC-1 DEF-2"] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
        assert_eq!(normalize("  ABC-1 "), Some("ABC-1".into()));
        assert_eq!(normalize("proj_x/42"), Some("proj_x/42".into()));
    }

    #[test]
    fn check_names_the_first_bad_key_and_what_to_do() {
        let keys = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(check(&keys(&["ABC-1", "DEF-2"])), Ok(()));
        assert_eq!(check(&[]), Ok(()));
        let why = check(&keys(&["ABC-1", "A B", "C,D"])).unwrap_err();
        assert!(why.starts_with("\"A B\" is not a ticket key"), "{why}");
        assert!(
            why.contains("no spaces or commas") && why.contains("its own argument"),
            "{why}"
        );
        assert!(
            check(&keys(&[""]))
                .unwrap_err()
                .starts_with("\"\" is not a ticket key")
        );
    }
    use crate::store::new_session;
    use chrono::Utc;

    fn session() -> Session {
        let now = Utc::now();
        new_session("a", "/x", now, now)
    }

    #[test]
    fn add_remove_and_parse_ignore_case_and_duplicates() {
        let mut list = Vec::new();
        assert!(add(&mut list, " ABC-1 "));
        assert!(!add(&mut list, "abc-1"), "same key in another case");
        assert!(!add(&mut list, "   "));
        assert!(!add(&mut list, "A B"), "whitespace inside a key");
        assert_eq!(list, ["ABC-1"], "stored as typed, trimmed");
        assert!(remove(&mut list, "abc-1"));
        assert!(!remove(&mut list, "abc-1"));
        assert_eq!(parse_list("A-1, b-2  a-1,,C-3"), ["A-1", "b-2", "C-3"]);
    }

    #[test]
    fn looks_like_key_is_strict() {
        for ok in ["ABC-1", "CLOUD-593", "AB-12", "A1-5", "ABCDEFGHIJ-1"] {
            assert!(looks_like_key(ok), "{ok}");
        }
        for bad in [
            "utf-8",
            "gpt-4",
            "X-1",
            "ABC-",
            "ABC",
            "ABC-1a",
            "1ABC-1",
            "ABCDEFGHIJK-1",
            "feat-1",
        ] {
            assert!(!looks_like_key(bad), "{bad}");
        }
    }

    #[test]
    fn parse_query_splits_free_text_from_ticket_filters() {
        assert_eq!(
            parse_query("grafana ticket:ABC-1 dash"),
            ("grafana dash".to_string(), vec!["ABC-1".to_string()])
        );
        assert_eq!(parse_query("TICKET:abc-1 TiCkEt:x-2").1, ["abc-1", "x-2"]);
        assert_eq!(
            parse_query("ticket:"),
            (String::new(), Vec::<String>::new()),
            "still typing"
        );
        assert_eq!(
            parse_query("tickets"),
            ("tickets".to_string(), Vec::<String>::new())
        );
        assert_eq!(
            parse_query("ticketé:x"),
            ("ticketé:x".to_string(), Vec::<String>::new()),
            "multibyte prefix is safe"
        );
    }

    #[test]
    fn has_all_needs_every_filter_and_ignores_case() {
        let mut s = session();
        s.tickets = vec!["ABC-1".into(), "DEF-2".into()];
        assert!(has_all(&s, &[]));
        assert!(has_all(&s, &["abc-1".to_string()]));
        assert!(has_all(&s, &["abc-1".to_string(), "def-2".to_string()]));
        assert!(!has_all(&s, &["ABC-1".to_string(), "ZZZ-9".to_string()]));
        assert!(!has_all(&s, &["ABC".to_string()]), "exact, not a prefix");
    }

    #[test]
    fn migration_moves_only_key_like_tags() {
        let mut a = session();
        a.tags = vec!["observability".into(), "CLOUD-593".into(), "utf-8".into()];
        let mut b = session();
        b.id = "b".into();
        b.tags = vec!["repair".into()];
        let mut all = vec![a, b];

        let plan = migration_plan(&all);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].keys, ["CLOUD-593"]);

        assert_eq!(migrate(&mut all), 1);
        assert_eq!(all[0].tags, ["observability", "utf-8"]);
        assert_eq!(all[0].tickets, ["CLOUD-593"]);
        assert_eq!(all[1].tags, ["repair"]);
        assert_eq!(migrate(&mut all), 0, "a second run changes nothing");
    }

    #[test]
    fn migration_does_not_duplicate_a_ticket_that_is_already_there() {
        let mut s = session();
        s.tickets = vec!["abc-1".into()];
        s.tags = vec!["ABC-1".into()];
        let mut all = vec![s];
        migrate(&mut all);
        assert_eq!(all[0].tickets, ["abc-1"]);
        assert!(all[0].tags.is_empty());
    }
}
