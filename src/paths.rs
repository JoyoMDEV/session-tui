//! Directories typed by a person: `~` and `$HOME` are expanded and the directory must exist. It
//! only looks at the file system and does not change it.

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
}
