//! Topic tags (`observability`, `repair`): one spelling per tag, removal, and the vocabulary the
//! hook shows the agent so it reuses tags instead of inventing new ones. Ticket keys live in
//! `tickets.rs`.

use crate::store::Session;
use crate::tickets;
use std::collections::HashMap;

/// Most tags the hook lists, and the most characters it spends on them. The hook's context is
/// limited to 10,000 characters in total, and the tag list must not crowd out the instructions.
const VOCABULARY_MAX_TAGS: usize = 40;
const VOCABULARY_MAX_CHARS: usize = 1200;

/// Lower case, trimmed, no leading `#`, whitespace runs turned into `-`.
pub fn normalize(tag: &str) -> Option<String> {
    let tag = tag.trim().trim_start_matches('#');
    let tag = tag
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

fn same(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

/// Adds the normalised tag unless one that normalises the same is already there.
pub fn add(list: &mut Vec<String>, tag: &str) -> bool {
    let Some(tag) = normalize(tag) else {
        return false;
    };
    if list.iter().any(|t| same(t, &tag)) {
        return false;
    }
    list.push(tag);
    true
}

/// Removes the tag however it was spelled when it was added. Returns whether anything was removed.
pub fn remove(list: &mut Vec<String>, tag: &str) -> bool {
    let before = list.len();
    list.retain(|t| !same(t, tag));
    list.len() != before
}

/// The tags in `text` (space separated), for the editor. A tag that matches an existing one keeps
/// the existing spelling, so saving the editor doesn't rewrite tags nobody touched; only new tags
/// are normalised. Duplicates are dropped.
pub fn reconcile(existing: &[String], text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let Some(normalized) = normalize(word) else {
            continue;
        };
        let spelling = existing
            .iter()
            .find(|t| same(t, &normalized))
            .cloned()
            .unwrap_or(normalized);
        if !out.iter().any(|t| same(t, &spelling)) {
            out.push(spelling);
        }
    }
    out
}

/// Whether the session has every given tag, however each is spelled. An empty filter matches all.
pub fn has_all(session: &Session, filters: &[String]) -> bool {
    filters
        .iter()
        .all(|f| session.tags.iter().any(|t| same(t, f)))
}

/// The words in `text` that look like ticket keys (`ABC-123`) and aren't among the `existing`
/// tags. Old files may hold such a tag, and saving the editor untouched must not trip over it.
pub fn new_ticket_keys<'a>(existing: &[String], text: &'a str) -> Vec<&'a str> {
    text.split_whitespace()
        .filter(|w| tickets::looks_like_key(w) && !existing.iter().any(|t| t == w))
        .collect()
}

/// Every tag of the given sessions once, spelled in its normal form, with the number of sessions
/// that have it. Most used first, ties by name.
pub fn counts<'a>(sessions: impl IntoIterator<Item = &'a Session>) -> Vec<(String, usize)> {
    tally(sessions, |_| true)
}

/// The tags in use, most used first (ties by name), without ticket-like keys, which belong in
/// the tickets field and shouldn't be suggested as topics.
pub fn vocabulary(sessions: &[Session]) -> Vec<(String, usize)> {
    tally(sessions, |tag| !tickets::looks_like_key(tag))
}

/// Counts the sessions per tag, looking only at tags for which `keep` is true. `keep` sees the
/// tag as stored, because a ticket key is recognised by its upper case spelling.
fn tally<'a>(
    sessions: impl IntoIterator<Item = &'a Session>,
    keep: impl Fn(&str) -> bool,
) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for s in sessions {
        // A session that lists a tag twice, in two spellings, still counts once.
        let mut own: Vec<String> = s
            .tags
            .iter()
            .filter(|t| keep(t))
            .filter_map(|t| normalize(t))
            .collect();
        own.sort();
        own.dedup();
        for tag in own {
            *counts.entry(tag).or_default() += 1;
        }
    }
    let mut out: Vec<_> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Existing tags that could finish the word being typed at the end of `text`, most used first.
/// Words typed before it are not offered again, and with nothing typed yet the most used tags
/// are offered.
pub fn suggestions(vocabulary: &[(String, usize)], text: &str) -> Vec<String> {
    let (done, prefix) = split_last_word(text);
    let prefix = normalize(prefix).unwrap_or_default();
    vocabulary
        .iter()
        .map(|(tag, _)| tag)
        .filter(|tag| tag.starts_with(&prefix))
        .filter(|tag| !done.iter().any(|w| same(w, tag)))
        .cloned()
        .collect()
}

/// Completes the last word of `text` like a shell: a single suggestion is completed and followed
/// by a space, several are completed as far as they agree, and without a match nothing changes.
pub fn complete(text: &str, suggestions: &[String]) -> String {
    let (_, prefix) = split_last_word(text);
    let head = &text[..text.len() - prefix.len()];
    let common = match suggestions {
        [] => return text.to_string(),
        [only] => return format!("{head}{only} "),
        [first, rest @ ..] => rest.iter().fold(first.clone(), |acc, s| {
            let n = acc
                .chars()
                .zip(s.chars())
                .take_while(|(a, b)| a == b)
                .count();
            acc.chars().take(n).collect()
        }),
    };
    // Never shorten what was typed: a typed `Obs` would otherwise be cut to nothing.
    if common.chars().count() > prefix.trim_start_matches('#').chars().count() {
        format!("{head}{common}")
    } else {
        text.to_string()
    }
}

/// The words of `text` before the last one, and the last one (empty after a trailing space).
fn split_last_word(text: &str) -> (Vec<&str>, &str) {
    let start = text.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    (text[..start].split_whitespace().collect(), &text[start..])
}

/// `observability (5), repair (3)`, cut off at a fixed size.
pub fn vocabulary_text(vocabulary: &[(String, usize)]) -> String {
    let mut out = String::new();
    for (tag, n) in vocabulary.iter().take(VOCABULARY_MAX_TAGS) {
        let item = format!("{tag} ({n})");
        let extra = if out.is_empty() { 0 } else { 2 };
        if out.chars().count() + extra + item.chars().count() > VOCABULARY_MAX_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::new_session;
    use chrono::Utc;

    #[test]
    fn counts_every_tag_once_per_session_in_its_normal_form() {
        let a = session(&["Observability", "repair", "observability"]);
        let b = session(&["observability", "ABC-1"]);
        assert_eq!(
            counts([&a, &b]),
            [
                ("observability".to_string(), 2),
                ("abc-1".to_string(), 1),
                ("repair".to_string(), 1)
            ]
        );
        assert!(counts(std::iter::empty::<&Session>()).is_empty());
        // The hook's vocabulary leaves ticket-like tags out.
        assert_eq!(vocabulary(&[a, b]).len(), 2);
    }

    fn vocab() -> Vec<(String, usize)> {
        [("observability", 5), ("observer", 2), ("repair", 2)]
            .into_iter()
            .map(|(t, n)| (t.to_string(), n))
            .collect()
    }

    #[test]
    fn suggestions_finish_the_last_word_and_skip_tags_already_typed() {
        let v = vocab();
        assert_eq!(suggestions(&v, "obs"), ["observability", "observer"]);
        assert_eq!(suggestions(&v, "#OBS"), ["observability", "observer"]);
        assert_eq!(suggestions(&v, "repair obs"), ["observability", "observer"]);
        assert_eq!(suggestions(&v, "observability obs"), ["observer"]);
        assert_eq!(suggestions(&v, "zzz"), Vec::<String>::new());
        // Nothing typed yet: the most used tags, minus the ones already there.
        assert_eq!(suggestions(&v, ""), ["observability", "observer", "repair"]);
        assert_eq!(suggestions(&v, "observer "), ["observability", "repair"]);
    }

    #[test]
    fn complete_works_like_a_shell() {
        let v = vocab();
        // One match: finished, with a space.
        assert_eq!(complete("rep", &suggestions(&v, "rep")), "repair ");
        assert_eq!(
            complete("observer rep", &suggestions(&v, "observer rep")),
            "observer repair "
        );
        // Several matches: as far as they agree, never shorter than typed.
        assert_eq!(complete("ob", &suggestions(&v, "ob")), "observ");
        assert_eq!(complete("observ", &suggestions(&v, "observ")), "observ");
        assert_eq!(complete("OBS", &suggestions(&v, "OBS")), "observ");
        // No match, or nothing typed with several candidates: unchanged.
        assert_eq!(complete("zzz", &suggestions(&v, "zzz")), "zzz");
        assert_eq!(complete("", &suggestions(&v, "")), "");
    }

    #[test]
    fn new_ticket_keys_skips_tags_that_are_already_there() {
        let existing = vec!["ABC-1".to_string(), "auth".to_string()];
        assert_eq!(
            new_ticket_keys(&existing, "ABC-1 auth ABC-2 utf-8 abc-3"),
            ["ABC-2"]
        );
    }

    fn session(tags: &[&str]) -> Session {
        let now = Utc::now();
        let mut s = new_session("a", "/x", now, now);
        s.tags = tags.iter().map(|t| t.to_string()).collect();
        s
    }

    #[test]
    fn normalize_lowercases_trims_and_joins_words_with_dashes() {
        assert_eq!(normalize("  #Observability "), Some("observability".into()));
        assert_eq!(normalize("Cert Manager"), Some("cert-manager".into()));
        assert_eq!(normalize("a   b\tc"), Some("a-b-c".into()));
        assert_eq!(normalize("Ünïcode"), Some("ünïcode".into()));
        assert_eq!(normalize("  # "), None);
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn add_stores_the_normalised_form_and_skips_a_spelling_variant() {
        let mut list = vec!["Observability".to_string()];
        assert!(!add(&mut list, "observability"), "same tag, different case");
        assert!(!add(&mut list, "#OBSERVABILITY"));
        assert!(add(&mut list, "Repair Work"));
        assert_eq!(
            list,
            ["Observability", "repair-work"],
            "old spelling untouched"
        );
    }

    #[test]
    fn remove_matches_however_the_tag_was_spelled() {
        let mut list = vec!["Observability".to_string(), "repair".to_string()];
        assert!(remove(&mut list, "#observability"));
        assert!(
            !remove(&mut list, "observability"),
            "no-op when it isn't there"
        );
        assert_eq!(list, ["repair"]);
    }

    #[test]
    fn reconcile_keeps_existing_spellings_and_normalises_only_new_tags() {
        let existing = vec!["Observability".to_string(), "CLOUD-327".to_string()];
        let got = reconcile(
            &existing,
            "Observability  New Tag cloud-327 observability #Repair",
        );
        assert_eq!(got, ["Observability", "new", "tag", "CLOUD-327", "repair"]);
        assert_eq!(
            reconcile(&existing, "  "),
            Vec::<String>::new(),
            "clearing works"
        );
    }

    #[test]
    fn vocabulary_counts_across_sessions_most_used_first_and_skips_ticket_keys() {
        let all = vec![
            session(&["observability", "repair", "ABC-1"]),
            session(&["Observability", "docs"]),
            session(&["repair", "observability"]),
        ];
        let v = vocabulary(&all);
        assert_eq!(
            v,
            [
                ("observability".to_string(), 3),
                ("repair".to_string(), 2),
                ("docs".to_string(), 1)
            ]
        );
        assert_eq!(
            vocabulary_text(&v),
            "observability (3), repair (2), docs (1)"
        );
        assert_eq!(vocabulary_text(&[]), "");
    }

    #[test]
    fn vocabulary_text_stays_within_its_budget_however_many_tags_exist() {
        let many: Vec<(String, usize)> = (0..500)
            .map(|i| (format!("a-rather-long-tag-name-{i}"), 1))
            .collect();
        let text = vocabulary_text(&many);
        assert!(
            text.chars().count() <= VOCABULARY_MAX_CHARS,
            "{}",
            text.len()
        );
        assert!(text.matches(", ").count() < VOCABULARY_MAX_TAGS);
        assert!(!text.is_empty() && !text.ends_with(", "));
    }
}
