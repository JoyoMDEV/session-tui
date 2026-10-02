use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Title proposal derived from the first prompt; only used while there is no real title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Title Claude Code generated itself (`ai-title` in the transcript). Kept after the
    /// transcript is deleted so the entry stays recognisable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_title: Option<String>,
    /// Last git branch seen in the transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// URL of the last pull request linked to the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Claude Code's config dir: `$CLAUDE_CONFIG_DIR`, otherwise `~/.claude`.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME not set")).join(".claude")
        })
}

/// `$SESSIONS_FILE`, otherwise `sessions.json` in the Claude config dir.
pub fn path() -> PathBuf {
    std::env::var_os("SESSIONS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| claude_dir().join("sessions.json"))
}

/// The outer `Result` is I/O, the inner one is a parse error of the file content.
fn read_raw(path: &Path) -> Result<Result<Vec<Session>, serde_json::Error>> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(Ok(Vec::new())),
        Ok(s) => Ok(serde_json::from_str(&s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ok(Vec::new())),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Reads without locking; writers replace the file atomically, so this never sees a partial write.
pub fn load() -> Result<Vec<Session>> {
    let path = path();
    read_raw(&path)?.map_err(|e| {
        anyhow::anyhow!(
            "{} is not valid JSON ({e}). Fix it by hand, or run any write command \
             (e.g. `sessions import`) to move it aside and start fresh.",
            path.display()
        )
    })
}

/// Locks, loads, applies `f`, and atomically writes the result back.
pub fn update<T>(f: impl FnOnce(&mut Vec<Session>) -> T) -> Result<T> {
    update_at(&path(), f)
}

fn update_at<T>(path: &Path, f: impl FnOnce(&mut Vec<Session>) -> T) -> Result<T> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let lock = File::create(path.with_extension("json.lock"))?;
    lock.lock_exclusive()?;

    let mut sessions = match read_raw(path)? {
        Ok(s) => s,
        Err(e) => {
            // Never overwrite a file we can't parse: keep it as a backup and start fresh.
            let backup = path.with_extension(format!("json.corrupt-{}", Utc::now().timestamp()));
            fs::rename(path, &backup)?;
            eprintln!(
                "warning: {} was not valid JSON ({e}); moved to {}",
                path.display(),
                backup.display()
            );
            Vec::new()
        }
    };
    let out = f(&mut sessions);

    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(&sessions)? + "\n")?;
    fs::rename(&tmp, path)?;
    Ok(out)
}

pub fn new_session(
    id: &str,
    cwd: &str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Session {
    Session {
        id: id.to_string(),
        title: None,
        suggestion: None,
        native_title: None,
        branch: None,
        pr_url: None,
        cwd: cwd.to_string(),
        tags: Vec::new(),
        note: None,
        created_at,
        updated_at,
    }
}

/// Insert a session or refresh `cwd`/`updated_at` of an existing one; everything else is kept.
pub fn upsert(sessions: &mut Vec<Session>, id: &str, cwd: &str) {
    let now = Utc::now();
    match sessions.iter_mut().find(|s| s.id == id) {
        Some(s) => {
            s.updated_at = now;
            if !cwd.is_empty() {
                s.cwd = cwd.to_string();
            }
        }
        None => sessions.push(new_session(id, cwd, now, now)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sessions-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("sessions.json")
    }

    #[test]
    fn upsert_keeps_title_tags_and_note_on_resume() {
        let mut all = Vec::new();
        upsert(&mut all, "a", "/x");
        all[0].title = Some("T".into());
        all[0].tags = vec!["jira".into()];
        all[0].note = Some("n".into());
        let created = all[0].created_at;

        upsert(&mut all, "a", "/y");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title.as_deref(), Some("T"));
        assert_eq!(all[0].tags, vec!["jira"]);
        assert_eq!(all[0].note.as_deref(), Some("n"));
        assert_eq!(all[0].cwd, "/y");
        assert_eq!(all[0].created_at, created);
        assert!(all[0].updated_at >= created);
    }

    #[test]
    fn upsert_with_empty_cwd_keeps_old_cwd() {
        let mut all = Vec::new();
        upsert(&mut all, "a", "/x");
        upsert(&mut all, "a", "");
        assert_eq!(all[0].cwd, "/x");
    }

    #[test]
    fn update_roundtrip_creates_missing_dirs() {
        let p = tmp_file("roundtrip");
        update_at(&p, |s| upsert(s, "a", "/x")).unwrap();
        update_at(&p, |s| upsert(s, "b", "/y")).unwrap();
        let loaded = read_raw(&p).unwrap().unwrap();
        assert_eq!(loaded.len(), 2);
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn corrupt_file_is_moved_aside_not_overwritten() {
        let p = tmp_file("corrupt");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "{ not json").unwrap();

        update_at(&p, |s| upsert(s, "a", "/x")).unwrap();

        assert_eq!(read_raw(&p).unwrap().unwrap().len(), 1);
        let backups: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), "{ not json");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn old_files_without_new_fields_still_load() {
        let json = r#"[{"id":"a","title":"T","cwd":"/x",
            "created_at":"2026-10-02T12:57:16Z","updated_at":"2026-10-02T12:57:16Z"}]"#;
        let all: Vec<Session> = serde_json::from_str(json).unwrap();
        assert!(all[0].tags.is_empty() && all[0].note.is_none() && all[0].suggestion.is_none());
        assert!(
            all[0].native_title.is_none() && all[0].branch.is_none() && all[0].pr_url.is_none()
        );
    }
}
