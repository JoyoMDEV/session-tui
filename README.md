# sessions

A terminal UI to find and resume your [Claude Code](https://claude.com/claude-code) sessions.

Claude Code prints a `claude --resume <session_id>` hint when a session ends, but nothing helps you find it again a week later. `sessions` registers every session, lets the agent give it a short title, and shows them in a searchable list. Pick one and it resumes in the right directory.

- Fuzzy search over titles, directories, tags and notes
- Title suggestions from the first prompt, so old sessions are usable without naming them first
- Tags and notes, e.g. a ticket key
- Import of sessions that existed before you installed the tool
- Sessions whose transcript has been deleted are marked and can be pruned

Unix only (macOS, Linux).

## Install

Download the archive for your platform from the releases page, verify it against the `.sha256` file, and put `sessions` on your `PATH`.

Or build from source (Rust 1.88 or newer):

```sh
git clone https://github.com/<owner>/session-tui
cd session-tui
cargo install --path .
```

Make sure `~/.cargo/bin` is on your `PATH`.

## Set up the hook

The hook registers each session and tells the agent its session ID and how to set a title.

**As a plugin** (install the binary first):

```text
/plugin marketplace add <owner>/session-tui
/plugin install sessions@session-tui
```

To try it from a local checkout: `claude --plugin-dir /path/to/session-tui`.

**Or by hand**, in `~/.claude/settings.json`:

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

Use one or the other, not both. New sessions are picked up from then on. To add your existing ones, run `sessions import`.

## Use

Run `sessions` to open the browser.

| Key | Action |
| --- | --- |
| type | Fuzzy filter |
| `↑` `↓`, `^P` `^N` | Move |
| `Enter` | Resume the session in its original directory |
| `^O` | Resume with extra `claude` flags |
| `Tab` | Show or hide empty sessions (no title and no prompt) |
| `^L` | Only sessions in or around the current directory |
| `^R` `^T` `^E` | Edit title, tags, note |
| `^X` | Delete the entry (asks first) |
| `Esc` | Quit |

Sessions without a title show a suggestion from their first prompt, marked with `~`. `^R` starts from that suggestion.

### Commands

The agent calls these, but you can too.

| Command | What it does |
| --- | --- |
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

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
claude plugin validate --strict .
```

Pushing a tag like `v0.1.0` builds release archives for macOS and Linux. The tag must match the version in `Cargo.toml` and `.claude-plugin/plugin.json`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
