//! Runs `scripts/check.sh`, which the git hooks and CI call, on commit messages and on a scratch
//! git repository with staged files.

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/check.sh")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sessions-hooks-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn commit_msg(name: &str, message: &str) -> Output {
    let dir = scratch(name);
    let file = dir.join("COMMIT_EDITMSG");
    fs::write(&file, message).unwrap();
    Command::new("sh")
        .arg(script())
        .arg("commit-msg")
        .arg(&file)
        .output()
        .unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn good_commit_messages_pass() {
    for (i, message) in [
        "feat(tui): add paging, mouse wheel and scrollbar\n",
        "fix: stop a panic\n\nA body that explains why.\n",
        "refactor!: drop the old format\n\nBREAKING CHANGE: sessions.json changes.\n",
        "docs(adr): record the layout\n# Please enter the commit message\n# lines starting with # are ignored\n",
        "Merge branch 'x' into main\n",
        "Revert \"feat: add y\"\n",
        "fixup! feat: add y\n",
    ]
    .iter()
    .enumerate()
    {
        let out = commit_msg(&format!("good{i}"), message);
        assert!(out.status.success(), "{message:?}: {}", stderr(&out));
    }
}

#[test]
fn bad_commit_messages_fail_with_a_reason() {
    let long = format!("feat: {}", "x".repeat(70));
    for (message, reason) in [
        ("Add paging\n", "type(scope): subject"),
        ("feat add paging\n", "type(scope): subject"),
        ("feature: add paging\n", "type(scope): subject"),
        ("Feat: add paging\n", "type(scope): subject"),
        ("feat:\n", "type(scope): subject"),
        ("feat: add paging.\n", "must not end with a period"),
        (long.as_str(), "72 is the hard limit"),
        (
            "feat: add y\n\nCo-Authored-By: Someone <a@b.c>\n",
            "attribution trailers",
        ),
        (
            "feat: add y\n\n🤖 Generated with Some Tool\n",
            "attribution trailers",
        ),
        ("\n# only a comment\n", "empty commit message"),
    ] {
        let out = commit_msg("bad", message);
        assert!(!out.status.success(), "{message:?} passed");
        assert!(
            stderr(&out).contains(reason),
            "{message:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_subject_over_50_characters_passes_with_a_note() {
    let message = format!("feat: {}\n", "x".repeat(55));
    let out = commit_msg("note", &message);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("50 is the aim"), "{}", stderr(&out));
}

/// A scratch repository with `files` staged, and the result of the pre-commit checks on it.
fn pre_commit(name: &str, files: &[(&str, &str)]) -> Output {
    let repo = scratch(name);
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
    };
    git(&["init", "-q"]);
    for (path, text) in files {
        let full: PathBuf = repo.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, text).unwrap();
        git(&["add", path]);
    }
    Command::new("sh")
        .arg(script())
        .arg("pre-commit")
        .current_dir(&repo)
        .env("CHECK_SKIP_MISSING", "1")
        .output()
        .unwrap()
}

#[test]
fn pre_commit_lets_ordinary_files_through() {
    let out = pre_commit("ok", &[("notes.txt", "fine\n"), ("dir/data.json", "{}\n")]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn pre_commit_refuses_files_that_must_never_be_committed() {
    for path in [
        "sessions.json",
        "backup/sessions.json",
        "transcript.jsonl",
        ".env",
        ".env.local",
        "deploy.pem",
        "signing.key",
        "id_ed25519",
    ] {
        let out = pre_commit("forbidden", &[(path, "x\n")]);
        assert!(!out.status.success(), "{path} was accepted");
        assert!(stderr(&out).contains(path), "{path}: {}", stderr(&out));
    }
}

#[test]
fn pre_commit_refuses_whitespace_errors_and_conflict_markers() {
    let spaces = pre_commit("spaces", &[("a.txt", "trailing   \n")]);
    assert!(!spaces.status.success(), "trailing whitespace passed");
    let conflict = pre_commit(
        "conflict",
        &[("b.txt", "<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> topic\n")],
    );
    assert!(!conflict.status.success(), "conflict markers passed");
}

#[test]
fn pre_commit_with_nothing_staged_succeeds() {
    let out = pre_commit("empty", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn an_unknown_mode_prints_the_usage() {
    let out = Command::new("sh")
        .arg(script())
        .arg("nope")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("usage"), "{}", stderr(&out));
}
