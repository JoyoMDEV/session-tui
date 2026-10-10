//! Starting `claude`, to resume a session or to start a new one. The browser and the commands end
//! here: the process is replaced by `claude`, which is why the tool is Unix only. It does not
//! decide which session to resume or what to start.

use crate::preset::{self, Preset};
use crate::store::Session;
use anyhow::{Error, anyhow};
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Extra arguments for `claude` from `$SESSIONS_CLAUDE_ARGS`, as one string.
pub fn default_args() -> String {
    std::env::var("SESSIONS_CLAUDE_ARGS").unwrap_or_default()
}

/// A session to start: where, with which first message, and which tickets and title the hook is to
/// record for it.
pub struct NewSession {
    pub dir: String,
    pub prompt: String,
    pub preset: Preset,
}

/// The command that starts `new`, apart from `start` so it can be looked at. The first message is
/// the last argument. The preset travels in the environment, and a preset that is not wanted is
/// taken out of it, so one inherited from the shell cannot reach the new session.
pub fn start_command(new: &NewSession, args: &[String]) -> Command {
    let mut cmd = Command::new("claude");
    cmd.args(args).current_dir(&new.dir);
    let prompt = new.prompt.trim();
    if !prompt.is_empty() {
        // A message that starts with a dash would be read as an option.
        cmd.arg(if prompt.starts_with('-') {
            format!(" {prompt}")
        } else {
            prompt.to_string()
        });
    }
    if new.preset.is_empty() {
        cmd.env_remove(preset::ENV);
    } else {
        cmd.env(preset::ENV, new.preset.to_json());
    }
    cmd
}

/// Replaces this process with `claude` started as described by `new`. It only returns, with the
/// reason, if `claude` could not be started.
pub fn start(new: &NewSession, args: &[String]) -> Error {
    let err = start_command(new, args).exec();
    anyhow!("could not start claude in {}: {err}", new.dir)
}

/// Replaces this process with `claude --resume <id>` in the session's directory. It only returns,
/// with the reason, if `claude` could not be started.
pub fn resume(session: &Session, args: &[String]) -> Error {
    let err = Command::new("claude")
        .arg("--resume")
        .arg(&session.id)
        .args(args)
        .current_dir(&session.cwd)
        .exec();
    anyhow!("could not start claude in {}: {err}", session.cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    fn new(prompt: &str, preset: Preset) -> NewSession {
        NewSession {
            dir: "/work/app".into(),
            prompt: prompt.into(),
            preset,
        }
    }

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn env<'a>(cmd: &'a Command, key: &str) -> Option<Option<&'a OsStr>> {
        cmd.get_envs()
            .find(|(k, _)| *k == OsStr::new(key))
            .map(|(_, v)| v)
    }

    #[test]
    fn the_first_message_is_the_last_argument_after_the_saved_flags() {
        let cmd = start_command(
            &new("  fix the login  ", Preset::default()),
            &["--model".into(), "sonnet".into()],
        );
        assert_eq!(args(&cmd), ["--model", "sonnet", "fix the login"]);
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/work/app")));
        assert_eq!(cmd.get_program(), "claude");
    }

    #[test]
    fn without_a_message_there_is_no_argument() {
        assert!(args(&start_command(&new("   ", Preset::default()), &[])).is_empty());
    }

    #[test]
    fn a_message_that_starts_with_a_dash_is_not_taken_for_an_option() {
        let cmd = start_command(&new("--version please", Preset::default()), &[]);
        assert_eq!(args(&cmd), [" --version please"]);
    }

    #[test]
    fn the_preset_goes_in_the_environment_and_an_inherited_one_is_removed() {
        let preset = Preset::new(vec!["ABC-1".into()], Some("Fix login".into()));
        let with = start_command(&new("", preset), &[]);
        assert_eq!(
            env(&with, preset::ENV),
            Some(Some(OsStr::new(
                r#"{"tickets":["ABC-1"],"title":"Fix login"}"#
            )))
        );
        let without = start_command(&new("", Preset::default()), &[]);
        assert_eq!(env(&without, preset::ENV), Some(None), "None means removed");
    }
}
