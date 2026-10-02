# sessions

A terminal UI to find and resume your [Claude Code](https://claude.com/claude-code) sessions.

Claude Code prints a `claude --resume <session_id>` hint when a session ends, but nothing helps you find it again a week later. `sessions` registers every session, lets the agent give it a short title, and shows them in a searchable list. Pick one and it resumes in the right directory.

- Fuzzy search over titles, directories, tags and notes
- Shows the title, git branch and pull request Claude Code recorded itself, so old sessions are usable without naming them first
- Conversation preview before you resume
- Tags and notes, e.g. a ticket key
- Import of sessions that existed before you installed the tool
- Sessions whose transcript has been deleted are marked and can be pruned

Unix only (macOS, Linux).

## Install

**Install script** (macOS and Linux). It downloads the latest release for your machine, checks it against the published SHA-256 file and installs it to `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/JoyoMDEV/session-tui/main/install.sh | sh -s -- --setup --import
```

`--setup` also adds the Claude Code hook (see below) and `--import` registers your existing sessions. Without them the script prints the next steps. Use `--version v0.1.0` to pin a release and `INSTALL_DIR` to choose another directory. The script never edits your shell profile; if the directory isn't on your `PATH` it tells you which line to add. To read it before running it, download it first with `curl -fsSLO` and run `sh install.sh`.

**Manually:** download the archive for your platform from the releases page, check it with the `.sha256` file, and put `sessions` on your `PATH`. A browser download of an unsigned binary is quarantined by macOS; clear that with `xattr -d com.apple.quarantine sessions`. Downloads through the install script (curl) aren't affected.

**From source** (Rust 1.88 or newer):

```sh
git clone https://github.com/JoyoMDEV/session-tui
cd session-tui
cargo install --path .
```

Make sure `~/.cargo/bin` is on your `PATH`.

## Set up the hook

The hook registers each session and tells the agent its session ID and how to set a title.

**With the setup command** (simplest):

```sh
sessions setup
```

It adds the hook to `~/.claude/settings.json`, keeps the old file as `settings.json.bak-<timestamp>`, and is safe to repeat. It leaves the file alone if the hook is already there, even if you wrote it by hand, or if the plugin below is enabled.

**As a plugin** (install the binary first):

```text
/plugin marketplace add JoyoMDEV/session-tui
/plugin install sessions@session-tui
```

To try it from a local checkout: `claude --plugin-dir /path/to/session-tui`.

**By hand**, in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume|clear|compact|fork",
        "hooks": [{ "type": "command", "command": "$HOME/.cargo/bin/sessions hook", "timeout": 10 }]
      }
    ]
  }
}
```

Use only one of the three. New sessions are picked up from then on. To add your existing ones, run `sessions import`.

## Use

Run `sessions` to open the browser.

| Key | Action |
| --- | --- |
| type | Fuzzy filter |
| `↑` `↓`, `^P` `^N` | Move |
| `PgUp` `PgDn`, `Home` `End`, mouse wheel | Scroll by page, jump to start or end, scroll |
| `Enter` | Resume the session in its original directory |
| `^O` | Resume with extra `claude` flags |
| `^V` | Preview the conversation (`Esc` closes) |
| `Tab` | Show or hide empty sessions (no title and no prompt) |
| `^L` | Only sessions in or around the current directory |
| `^B` | Only sessions on the git branch checked out in the current directory |
| `^R` `^T` `^E` | Edit title, tags, note |
| `^X` | Delete the entry (asks first) |
| `Esc` | Quit |

The TUI captures the mouse for scrolling, so select text with Shift held (Option in some terminals). On terminals under 22 rows the details pane is hidden to make room for the list.

Each row shows the best title available: yours (plain), else the one Claude Code generated (italic), else a snippet of the first prompt (dimmed, marked with `~`). `^R` starts from whichever is shown.

### Commands

The agent calls these, but you can too.

| Command | What it does |
| --- | --- |
| `sessions setup` | Add the SessionStart hook to Claude Code's `settings.json` |
| `sessions title [--id ID] <title>` | Set the title |
| `sessions tag [--id ID] <tag>...` | Add tags |
| `sessions note [--id ID] <text>` | Set the note (empty text clears it) |
| `sessions import` | Register transcripts that aren't known yet |
| `sessions prune [--yes]` | Remove entries whose transcript is gone (dry run without `--yes`) |
| `sessions rm <id>` | Remove an entry |
| `sessions list` | Print all entries |

`--id` defaults to `$CLAUDE_SESSION_ID`, which the hook sets for the agent's shell.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SESSIONS_FILE` | `<claude dir>/sessions.json` | Where sessions are stored |
| `CLAUDE_CONFIG_DIR` | `~/.claude` | Claude Code's config dir, also used to find transcripts |
| `SESSIONS_CLAUDE_ARGS` | empty | Extra flags added to every `claude --resume` |

## How it works

On every `SessionStart` Claude Code runs `sessions hook` with a JSON payload on stdin. The hook upserts the session in `sessions.json` and replies with `additionalContext`, which tells the agent the session ID and asks it to set a title once the topic is clear. It also appends `CLAUDE_SESSION_ID` and the binary's directory to `CLAUDE_ENV_FILE`, so the agent's shell can call `sessions` without further setup.

`sessions.json` is a plain JSON array. Writes are locked and atomic, and a file that doesn't parse is moved aside as `sessions.json.corrupt-<timestamp>` rather than overwritten.

The sort order uses the transcript's modification time, which changes on every message, so a long-running session stays near the top.

Claude Code's generated title, git branch and linked pull request are read from the transcript, whose format is internal to Claude Code and may change between versions. If a field disappears, `sessions` shows less rather than failing. The values are copied into `sessions.json`, so an entry stays recognisable after the transcript is gone.

When you resume a session that has a title from `sessions`, the hook also passes it to Claude Code as `sessionTitle`, so `claude --resume` shows it too. Claude Code only honours this at session start, and the hook leaves a name alone if the session already has one (for example from `/rename`). A title set mid-session therefore reaches Claude Code's own picker the next time you resume.

### Transcript retention

Claude Code deletes old transcripts (30 days by default, see `cleanupPeriodDays` in its settings). `sessions` keeps the entry, but a session without a transcript can't be resumed: it is marked `✗`, and `sessions prune` removes such entries. Raise `cleanupPeriodDays` if you want to find sessions for longer.

## Compared with `claude --resume`

Claude Code has its own picker. As documented at the time of writing, it covers search, preview, rename, git branch, worktrees and PR lookup. `sessions` overlaps with it on purpose and adds:

- tags and notes (the built-in picker has none),
- a view across all projects by default, resuming straight in the session's own directory,
- extra `claude` flags per resume, such as `--fork-session`.

The built-in picker has things `sessions` doesn't: worktree grouping, grouped forks and PR URL search.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
claude plugin validate --strict .
```

`install.sh` and `scripts/formula.sh` are checked with `shellcheck`. Pushing a tag like `v0.1.0` builds release archives for macOS and Linux. The tag must match the version in `Cargo.toml` and `.claude-plugin/plugin.json`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
