//! Directories typed by a person: `~` and `$HOME` are expanded, the directory must exist, and a
//! half-typed path can be completed like in a shell. It only looks at the file system and does not
//! change it.

use crate::query;
use std::path::{Path, PathBuf};

/// `text` with a leading `~`, `$HOME` or `${HOME}` replaced by `home`. Anything else is kept.
pub fn expand(text: &str, home: Option<&Path>) -> Result<String, String> {
    let rest = ["${HOME}", "$HOME", "~"]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .filter(|rest| rest.is_empty() || rest.starts_with('/'));
    let Some(rest) = rest else {
        return Ok(text.to_string());
    };
    let home = home.ok_or("HOME is not set, so ~ can't be expanded")?;
    Ok(format!("{}{rest}", home.display()))
}

/// The absolute path of an existing directory named by `text`, or the reason there is none.
/// Relative paths are taken from the current directory.
pub fn check_dir(text: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Choose a directory".to_string());
    }
    let expanded = expand(text, home)?;
    let path = PathBuf::from(query::absolute_dir(&expanded).map_err(|e| e.to_string())?);
    match path.metadata() {
        Ok(meta) if meta.is_dir() => Ok(path),
        Ok(_) => Err(format!("Not a directory: {}", path.display())),
        Err(_) => Err(format!("No such directory: {}", path.display())),
    }
}

/// The most suggestions offered for one half-typed path.
const MAX_CANDIDATES: usize = 50;

/// A directory that could finish what was typed.
#[derive(Debug, PartialEq, Clone)]
pub struct Candidate {
    /// The path as the person would type it (`~/Code/app/`), ending with a slash.
    pub text: String,
    /// It is a git repository.
    pub git: bool,
    /// A session was started there before.
    pub used: bool,
}

/// Directories that could finish `typed`: first the `used` ones, in the order given (most recent
/// first), whose whole path starts with it, then the subdirectories of what is typed up to the last
/// slash whose names start with the rest, alphabetically. Hidden directories are offered only if a
/// dot was typed. Only absolute paths, `~` and `$HOME` are completed.
pub fn candidates(typed: &str, used: &[String], home: Option<&Path>) -> Vec<Candidate> {
    let Ok(expanded) = expand(typed, home) else {
        return Vec::new();
    };
    let Some(slash) = expanded.rfind('/').filter(|_| expanded.starts_with('/')) else {
        return Vec::new();
    };
    let (parent, prefix) = expanded.split_at(slash + 1);

    let mut found: Vec<String> = Vec::new();
    let mut add = |dir: String| {
        if !found.contains(&dir) {
            found.push(dir);
        }
    };
    let used_dirs: Vec<&String> = used
        .iter()
        .filter(|u| u.starts_with(&expanded) && Path::new(u.as_str()).is_dir())
        .collect();
    for dir in &used_dirs {
        add(format!("{}/", dir.trim_end_matches('/')));
    }
    let mut names: Vec<String> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    for name in names {
        add(format!("{parent}{name}/"));
    }

    found
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|dir| Candidate {
            git: Path::new(&dir).join(".git").exists(),
            used: used_dirs
                .iter()
                .any(|u| dir.trim_end_matches('/') == u.trim_end_matches('/')),
            text: restyle(&dir, typed, home),
        })
        .collect()
}

/// `dir` written the way `typed` started: `~/…` or `$HOME/…` if it did and `dir` is under home.
fn restyle(dir: &str, typed: &str, home: Option<&Path>) -> String {
    let Some(home) = home.map(|h| h.display().to_string()) else {
        return dir.to_string();
    };
    for marker in ["${HOME}", "$HOME", "~"] {
        let marked = typed
            .strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        if let (true, Some(rest)) = (marked, dir.strip_prefix(&home))
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return format!("{marker}{rest}");
        }
    }
    dir.to_string()
}

/// `typed` completed like a shell does with Tab: a single candidate in full, several as far as
/// they agree, and never shorter than what was typed.
pub fn complete(typed: &str, candidates: &[Candidate]) -> String {
    let mut texts = candidates.iter().map(|c| c.text.as_str());
    let Some(first) = texts.next() else {
        return typed.to_string();
    };
    let common = texts.fold(first.to_string(), |acc, text| {
        let agree = acc
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();
        acc.chars().take(agree).collect()
    });
    if common.chars().count() > typed.chars().count() && common.starts_with(typed) {
        common
    } else {
        typed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sessions-paths-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn expand_replaces_a_leading_home_marker_only() {
        let home = Path::new("/home/u");
        for (given, want) in [
            ("~", "/home/u"),
            ("~/Code", "/home/u/Code"),
            ("$HOME", "/home/u"),
            ("$HOME/Code", "/home/u/Code"),
            ("${HOME}/Code", "/home/u/Code"),
            ("/abs/~", "/abs/~"),
            ("~other/x", "~other/x"),
            ("$HOMEDIR/x", "$HOMEDIR/x"),
            ("rel/dir", "rel/dir"),
            ("", ""),
        ] {
            assert_eq!(expand(given, Some(home)).unwrap(), want, "{given:?}");
        }
        assert!(expand("~/x", None).is_err());
        assert_eq!(expand("/plain", None).unwrap(), "/plain", "no HOME needed");
    }

    #[test]
    fn check_dir_accepts_an_existing_directory_with_spaces_and_home_markers() {
        let home = scratch("home").canonicalize().unwrap();
        fs::create_dir_all(home.join("My Code/proj")).unwrap();
        for text in [
            "~/My Code/proj",
            "$HOME/My Code/proj",
            "  ~/My Code/./proj/  ",
            "~/My Code/x/../proj",
        ] {
            assert_eq!(
                check_dir(text, Some(&home)).unwrap(),
                home.join("My Code/proj"),
                "{text:?}"
            );
        }
        assert_eq!(check_dir("~", Some(&home)).unwrap(), home);
    }

    #[test]
    fn check_dir_says_why_a_path_cannot_be_used() {
        let home = scratch("why").canonicalize().unwrap();
        fs::write(home.join("file.txt"), "x").unwrap();
        let missing = check_dir("~/nope", Some(&home)).unwrap_err();
        assert_eq!(
            missing,
            format!("No such directory: {}/nope", home.display())
        );
        let file = check_dir("~/file.txt", Some(&home)).unwrap_err();
        assert!(file.starts_with("Not a directory: "), "{file}");
        assert_eq!(
            check_dir("   ", Some(&home)).unwrap_err(),
            "Choose a directory"
        );
        assert!(
            check_dir("~/x", None)
                .unwrap_err()
                .contains("HOME is not set")
        );
    }

    #[test]
    fn a_relative_path_is_taken_from_the_current_directory() {
        let here = std::env::current_dir().unwrap();
        assert_eq!(check_dir(".", None).unwrap(), here);
    }

    fn tree(name: &str) -> PathBuf {
        let root = scratch(name).canonicalize().unwrap();
        for dir in [
            "Code/app/.git",
            "Code/api",
            "Code/Another Project",
            "Code/.hidden",
            "Docs",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("Code/file.txt"), "x").unwrap();
        root
    }

    fn texts(found: &[Candidate]) -> Vec<&str> {
        found.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn candidates_are_the_matching_directories_alphabetically_without_files_or_hidden_ones() {
        let home = tree("cand");
        let all = candidates("~/Code/", &[], Some(&home));
        assert_eq!(
            texts(&all),
            ["~/Code/Another Project/", "~/Code/api/", "~/Code/app/"]
        );
        let some = candidates("~/Code/ap", &[], Some(&home));
        assert_eq!(texts(&some), ["~/Code/api/", "~/Code/app/"]);
        let hidden = candidates("~/Code/.h", &[], Some(&home));
        assert_eq!(texts(&hidden), ["~/Code/.hidden/"]);
        assert!(candidates("~/Code/zz", &[], Some(&home)).is_empty());
        assert!(candidates("~/missing/x", &[], Some(&home)).is_empty());
    }

    #[test]
    fn candidates_mark_git_repositories_and_keep_the_way_the_path_was_typed() {
        let home = tree("style");
        let by_tilde = candidates("~/Code/ap", &[], Some(&home));
        assert!(by_tilde[1].git && !by_tilde[0].git, "{by_tilde:?}");
        let by_var = candidates("$HOME/Code/ap", &[], Some(&home));
        assert_eq!(texts(&by_var), ["$HOME/Code/api/", "$HOME/Code/app/"]);
        let absolute = format!("{}/Code/ap", home.display());
        let by_path = candidates(&absolute, &[], Some(&home));
        assert!(
            texts(&by_path)[0].starts_with(&home.display().to_string()),
            "{by_path:?}"
        );
    }

    #[test]
    fn used_directories_come_first_in_the_order_given_and_only_if_they_still_exist() {
        let home = tree("used");
        let used = vec![
            home.join("Docs").display().to_string(),
            home.join("Code/app").display().to_string(),
            home.join("Code/gone").display().to_string(),
        ];
        let found = candidates("~/", &used, Some(&home));
        assert_eq!(texts(&found)[..2], ["~/Docs/", "~/Code/app/"], "{found:?}");
        assert!(found[0].used && found[1].used && !found[2].used);
        assert!(!texts(&found).iter().any(|t| t.contains("gone")));
        // A used directory is matched by its whole path, so it is found from further up.
        let deep = candidates("~/Co", &used, Some(&home));
        assert_eq!(deep[0].text, "~/Code/app/", "{deep:?}");
        assert_eq!(
            texts(&deep).iter().filter(|t| **t == "~/Code/app/").count(),
            1,
            "no duplicates"
        );
    }

    #[test]
    fn only_absolute_paths_and_home_markers_are_completed() {
        let home = tree("rel");
        assert!(candidates("Code/", &[], Some(&home)).is_empty());
        assert!(candidates("", &[], Some(&home)).is_empty());
        assert!(candidates("~/Code/", &[], None).is_empty(), "~ needs HOME");
    }

    #[test]
    fn complete_works_like_a_shell() {
        let home = tree("complete");
        let one = candidates("~/Co", &[], Some(&home));
        assert_eq!(complete("~/Co", &one), "~/Code/");
        let several = candidates("~/Code/a", &[], Some(&home));
        assert_eq!(
            complete("~/Code/a", &several),
            "~/Code/ap",
            "as far as api and app agree"
        );
        let spaces = candidates("~/Code/An", &[], Some(&home));
        assert_eq!(complete("~/Code/An", &spaces), "~/Code/Another Project/");
        // Nothing to add: unchanged, and never shorter.
        assert_eq!(
            complete("~/Code/ap", &candidates("~/Code/ap", &[], Some(&home))),
            "~/Code/ap"
        );
        assert_eq!(complete("~/zz", &[]), "~/zz");
    }
}
