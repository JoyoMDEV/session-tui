//! Read-only access to Claude Code's own session transcripts (`<config>/projects/*/<id>.jsonl`).

use crate::store::{self, Session};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Prompts can be long; the first line is plenty for a title proposal.
const SUGGESTION_CHARS: usize = 70;
/// The first real prompt sits near the top; don't scan huge files end to end.
const MAX_LINES: usize = 5000;

#[derive(Debug, Default)]
pub struct Info {
    pub cwd: Option<String>,
    pub started: Option<DateTime<Utc>>,
    pub first_prompt: Option<String>,
}

fn project_dirs() -> Vec<PathBuf> {
    fs::read_dir(store::claude_dir().join("projects"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

/// All top-level transcripts (subagent logs live deeper and are not sessions).
pub fn all() -> Vec<PathBuf> {
    project_dirs()
        .into_iter()
        .flat_map(|d| fs::read_dir(d).into_iter().flatten().filter_map(|e| e.ok()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect()
}

pub fn existing_ids() -> HashSet<String> {
    all()
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect()
}

pub fn find(id: &str) -> Option<PathBuf> {
    project_dirs()
        .into_iter()
        .map(|d| d.join(format!("{id}.jsonl")))
        .find(|p| p.is_file())
}

/// Text of a user record, or `None` for tool results, meta records, and slash-command noise.
fn prompt_text(v: &Value) -> Option<String> {
    if v["type"] != "user" || v["isMeta"].as_bool() == Some(true) {
        return None;
    }
    let content = &v["message"]["content"];
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    // `<command-name>`, `<local-command-caveat>`, `<system-reminder>` and friends.
    (!text.is_empty() && !text.starts_with('<')).then(|| text.to_string())
}

pub fn read_info(path: &Path) -> Result<Info> {
    let mut info = Info::default();
    for line in BufReader::new(File::open(path)?).lines().take(MAX_LINES) {
        let Ok(v) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        if info.cwd.is_none() {
            info.cwd = v["cwd"].as_str().map(str::to_string);
        }
        if info.started.is_none() {
            info.started = v["timestamp"]
                .as_str()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc));
        }
        if let Some(p) = prompt_text(&v) {
            info.first_prompt = Some(p);
            break;
        }
    }
    Ok(info)
}

/// First line of the prompt, whitespace-collapsed and cut to a title-sized snippet.
pub fn suggest(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= SUGGESTION_CHARS {
        line
    } else {
        line.chars()
            .take(SUGGESTION_CHARS)
            .collect::<String>()
            .trim_end()
            .to_string()
            + "…"
    }
}

/// Adds a suggestion to an untitled session that has none yet. Returns whether it changed.
pub fn fill_suggestion(s: &mut Session) -> bool {
    if s.title.is_some() || s.suggestion.is_some() {
        return false;
    }
    let prompt = find(&s.id)
        .and_then(|p| read_info(&p).ok())
        .and_then(|i| i.first_prompt);
    s.suggestion = prompt.map(|p| suggest(&p)).filter(|p| !p.is_empty());
    s.suggestion.is_some()
}

/// Raises `updated_at` to `activity` if that is later. Returns whether it changed.
fn bump_to(s: &mut Session, activity: DateTime<Utc>) -> bool {
    let later = activity > s.updated_at;
    if later {
        s.updated_at = activity;
    }
    later
}

/// Claude appends to the transcript on every message, so its mtime is the last real activity.
/// Lifts `updated_at` of known sessions to it; returns how many changed.
pub fn sync_activity(sessions: &mut [Session]) -> usize {
    let mtimes: std::collections::HashMap<String, DateTime<Utc>> = all()
        .into_iter()
        .filter_map(|p| {
            let id = p.file_stem()?.to_string_lossy().into_owned();
            let t = fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((id, DateTime::<Utc>::from(t)))
        })
        .collect();
    let mut changed = 0;
    for s in sessions.iter_mut() {
        if let Some(&t) = mtimes.get(&s.id) {
            changed += bump_to(s, t) as usize;
        }
    }
    changed
}

/// What Claude Code itself recorded about a session; the last value of each wins.
#[derive(Debug, Default, PartialEq)]
pub struct Meta {
    pub native_title: Option<String>,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
}

/// Value of `"key":"…"` for plain strings without escapes, found without parsing the line.
/// Branch names are the only caller, and parsing every record just for them would be slow.
fn quick_str<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = &line[line.find(key)? + key.len()..];
    let value = &rest[..rest.find('"')?];
    (!value.is_empty() && !value.contains('\\')).then_some(value)
}

/// Scans the whole transcript for the generated title, git branch and linked PR. The format is
/// internal to Claude Code, so unknown or missing records just leave the fields empty.
pub fn scan_meta(path: &Path) -> Result<Meta> {
    let mut meta = Meta::default();
    for line in BufReader::new(File::open(path)?).lines() {
        let Ok(line) = line else { continue };
        if line.contains("\"ai-title\"") || line.contains("\"pr-link\"") {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v["type"].as_str() {
                Some("ai-title") => {
                    meta.native_title = v["aiTitle"]
                        .as_str()
                        .map(str::to_string)
                        .or(meta.native_title)
                }
                Some("pr-link") => {
                    meta.pr_url = v["prUrl"].as_str().map(str::to_string).or(meta.pr_url)
                }
                _ => {}
            }
        } else if let Some(branch) = quick_str(&line, "\"gitBranch\":\"") {
            // A detached HEAD is recorded as "HEAD", which is not a branch name.
            if branch != "HEAD" {
                meta.branch = Some(branch.to_string());
            }
        }
    }
    Ok(meta)
}

/// Copies title, branch and PR from the transcripts into the sessions. Values that are gone
/// from the transcript (or whose transcript was deleted) are kept.
pub fn refresh_meta(sessions: &mut [Session]) {
    let paths: HashMap<String, PathBuf> = all()
        .into_iter()
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
        .collect();
    for s in sessions.iter_mut() {
        let Some(meta) = paths.get(&s.id).and_then(|p| scan_meta(p).ok()) else {
            continue;
        };
        s.native_title = meta.native_title.or(s.native_title.take());
        s.branch = meta.branch.or(s.branch.take());
        s.pr_url = meta.pr_url.or(s.pr_url.take());
    }
}

#[derive(Default)]
pub struct ImportStats {
    pub added: usize,
    pub updated: usize,
    pub skipped_empty: usize,
}

/// Registers every transcript that isn't known yet and fills missing suggestions. Idempotent.
pub fn import() -> Result<ImportStats> {
    store::update(|sessions| {
        let mut stats = ImportStats::default();
        for path in all() {
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                if fill_suggestion(s) {
                    stats.updated += 1;
                }
                continue;
            }
            let Ok(info) = read_info(&path) else { continue };
            // Sessions without any prompt (opened and closed again) are noise.
            let (Some(prompt), Some(cwd)) = (info.first_prompt, info.cwd) else {
                stats.skipped_empty += 1;
                continue;
            };
            let mtime = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(|_| Utc::now());
            let mut s = store::new_session(&id, &cwd, info.started.unwrap_or(mtime), mtime);
            s.suggestion = Some(suggest(&prompt)).filter(|p| !p.is_empty());
            sessions.push(s);
            stats.added += 1;
        }
        sync_activity(sessions);
        refresh_meta(sessions);
        stats
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_transcript(name: &str, lines: &[&str]) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("sessions-tr-{}-{name}.jsonl", std::process::id()));
        let mut f = File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn suggest_takes_first_line_and_truncates() {
        assert_eq!(suggest("  \n Hallo   Welt \nzweite zeile"), "Hallo Welt");
        let long = "x".repeat(200);
        let s = suggest(&long);
        assert_eq!(s.chars().count(), SUGGESTION_CHARS + 1);
        assert!(s.ends_with('…'));
        assert_eq!(suggest("   "), "");
    }

    #[test]
    fn read_info_skips_meta_commands_and_tool_results() {
        let p = write_transcript(
            "skip",
            &[
                r#"{"type":"permission-mode"}"#,
                r#"{"type":"user","isMeta":true,"cwd":"/w","timestamp":"2026-10-01T10:00:00Z","message":{"content":"<local-command-caveat>x"}}"#,
                r#"{"type":"user","message":{"content":"<command-name>/model</command-name>"}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"zzz"}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"text","text":"Eigentliche Frage"}]}}"#,
                r#"{"type":"user","message":{"content":"too late"}}"#,
            ],
        );
        let info = read_info(&p).unwrap();
        assert_eq!(info.first_prompt.as_deref(), Some("Eigentliche Frage"));
        assert_eq!(info.cwd.as_deref(), Some("/w"));
        assert!(info.started.is_some());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn bump_to_only_moves_updated_at_forward() {
        let t0 = "2026-10-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let t1 = "2026-10-02T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let mut s = store::new_session("a", "/x", t0, t0);
        assert!(bump_to(&mut s, t1));
        assert_eq!(s.updated_at, t1);
        assert!(!bump_to(&mut s, t0));
        assert!(!bump_to(&mut s, t1));
        assert_eq!(s.updated_at, t1);
        assert_eq!(s.created_at, t0);
    }

    #[test]
    fn scan_meta_takes_the_last_title_branch_and_pr() {
        let p = write_transcript(
            "meta",
            &[
                r#"{"type":"user","gitBranch":"main","message":{"content":"hi"}}"#,
                r#"{"type":"ai-title","aiTitle":"First title","sessionId":"x"}"#,
                r#"{"type":"pr-link","prNumber":1,"prUrl":"https://example.test/pull/1"}"#,
                r#"{"type":"user","gitBranch":"feat/x","message":{"content":"again"}}"#,
                r#"{"type":"user","gitBranch":"HEAD","message":{"content":"detached"}}"#,
                r#"{"type":"ai-title","aiTitle":"Better title","sessionId":"x"}"#,
                r#"{"type":"pr-link","prNumber":2,"prUrl":"https://example.test/pull/2"}"#,
                "garbage",
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"\"gitBranch\":\"fake\""}]}}"#,
            ],
        );
        let meta = scan_meta(&p).unwrap();
        assert_eq!(meta.native_title.as_deref(), Some("Better title"));
        assert_eq!(meta.branch.as_deref(), Some("feat/x"));
        assert_eq!(meta.pr_url.as_deref(), Some("https://example.test/pull/2"));
        let _ = fs::remove_file(p);
    }

    #[test]
    fn scan_meta_on_a_bare_transcript_is_empty() {
        let p = write_transcript("bare", &[r#"{"type":"mode"}"#]);
        assert_eq!(scan_meta(&p).unwrap(), Meta::default());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn read_info_tolerates_garbage_lines_and_missing_prompt() {
        let p = write_transcript("garbage", &["not json", r#"{"type":"mode"}"#]);
        let info = read_info(&p).unwrap();
        assert!(info.first_prompt.is_none() && info.cwd.is_none());
        let _ = fs::remove_file(p);
    }
}
