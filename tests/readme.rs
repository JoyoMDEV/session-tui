//! Keeps the README in step with the command line.

use std::{fs, path::Path, process::Command};

/// Subcommand names from the "Commands:" block of `sessions --help`.
fn commands() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_sessions"))
        .arg("--help")
        .output()
        .expect("run sessions --help");
    let help = String::from_utf8(out.stdout).unwrap();
    help.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(String::from)
        .collect()
}

#[test]
fn readme_mentions_every_command() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("read README.md");
    let commands = commands();
    assert!(
        commands.len() > 5,
        "found no commands in --help: {commands:?}"
    );
    for name in commands {
        assert!(
            readme.contains(&format!("sessions {name}")),
            "README.md does not mention `sessions {name}`"
        );
    }
}
