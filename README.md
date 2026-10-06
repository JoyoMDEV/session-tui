# sessions

A logbook for your [Claude Code](https://claude.com/claude-code) sessions: titles, tickets, tags and notes that the agent keeps for you, searchable across all your projects.

Claude Code's own `claude --resume` picker is good for jumping back into recent work in the current repository. `sessions` is for finding work by what it was about, and keeping that around:

- **The agent keeps the log.** A hook tells the agent its session ID and asks it to give the session a title once the topic is clear, and to record the keys of any tickets it works on. Tags are for topics such as `observability`; tickets are a separate field. You don't have to name anything by hand.
- **Find work by ticket, branch or PR.** A session can have several tickets, and `ticket:ABC-123` in the search box shows exactly those sessions. Tags, notes, branch and pull request URL are searchable too, across every project. `^B` narrows the list to the branch you have checked out.
- **Resume where the session lived.** Enter changes into the session's original directory and resumes there, optionally with extra `claude` flags such as `--fork-session`.
- **The history outlives Claude's cleanup.** Claude Code deletes old transcripts after 30 days by default. `sessions` keeps the entry, with its title, branch, PR, tags and note, so you can still tell what you worked on back then. Such a session can no longer be resumed.
- **A plain JSON file you own.** Everything is in `sessions.json`, and `sessions list` prints it for scripts.

It also previews a conversation before you resume it, and imports the sessions you had before you installed it.

Unix only (macOS, Linux).

## Install

**Homebrew** (macOS and Linux):

```sh
brew install JoyoMDEV/tap/sessions
```

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

It adds the hook to `~/.claude/settings.json` and keeps the old file as `settings.json.bak-<timestamp>`. It is safe to repeat: a hook that is already there, even one you wrote by hand, is left alone, and so is the file if the plugin below is enabled.

If the hook points at a binary that no longer exists, for example after switching between Homebrew and `cargo install`, `setup` points it at the running binary instead. A hook that points at a different but working binary is also left alone; `sessions setup --force` switches it to this one.

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
| `^R` `^T` `^K` `^E` | Edit title, tags, tickets, note |
| `ticket:ABC-123` (in the search box) | Only sessions with exactly that ticket, ignoring case; repeat it to require several |
| `^X` | Delete the entry (asks first) |
| `Esc` | Quit |

The TUI captures the mouse for scrolling, so select text with Shift held (Option in some terminals). On terminals under 22 rows the details pane is hidden to make room for the list.

Each row shows the best title available: yours (plain), else the one Claude Code generated (italic), else a snippet of the first prompt (dimmed, marked with `~`). `^R` starts from whichever is shown.

### Commands

The agent calls these, but you can too.

| Command | What it does |
| --- | --- |
| `sessions setup [--force]` | Add or repair the SessionStart hook in Claude Code's `settings.json` |
| `sessions doctor` | Check the install and say how to fix what's wrong |
| `sessions title [--id ID] <title>` | Set the title |
| `sessions ticket [--id ID] [--remove] <KEY>...` | Add or remove ticket keys |
| `sessions migrate-tickets [--yes]` | Move tags that look like ticket keys (`ABC-123`) into tickets. Shows what it would change; `--yes` applies it after making a backup |
| `sessions tag [--id ID] [--remove] <tag>...` | Add or remove topic tags |
| `sessions note [--id ID] <text>` | Set the note (empty text clears it) |
| `sessions import` | Register transcripts that aren't known yet |
| `sessions prune [--yes]` | Remove entries whose transcript is gone (dry run without `--yes`) |
| `sessions rm <id>` | Remove an entry |
| `sessions list [--ticket KEY]...` | Print all entries, or only those with every given ticket |

`--id` defaults to `$CLAUDE_SESSION_ID`, which the hook sets for the agent's shell.

### Tags

Tags are for topics (`observability`, `repair`), not tickets. They are stored lower case with dashes for spaces, so `Cert Manager` and `cert-manager` are one tag. Tags that already exist keep their spelling, and saving the tag editor doesn't rewrite tags you didn't touch. `sessions tag ABC-123` is refused, because that looks like a ticket key: it says so, points to `sessions ticket`, and reminds you that tags are lower case. Only the exact shape is refused, so `utf-8` and `abc-123` are fine as tags. The tag editor (`^T`) refuses a new key the same way and points to `^K`; a key-shaped tag that is already there is left alone.

To keep the vocabulary small, the hook tells the agent which tags are already in use, with their counts, and asks it to reuse one and add a new tag only if none fits. The list is capped, so it stays small however many tags you have. When a session is resumed the hook also asks the agent to check that the title and tags still fit the work. It does this only at the start of a resumed session, never while the agent works; set `SESSIONS_RESUME_REMINDER=0` to turn it off. Remove a tag with `sessions tag --remove NAME`.

### Tickets

Tickets are the keys of issue tracker items, such as `ABC-123`. The agent records them with `sessions ticket`, and you can edit them with `^K`. They come from the agent, from you, or from `sessions migrate-tickets`; nothing is guessed from branch names, because a branch says little about what a session was for.

Keys are stored as you type them and compared ignoring case. Older versions asked the agent to add ticket keys as tags; `sessions migrate-tickets` moves those over.

## Troubleshooting

If the list is empty, titles stop appearing, or sessions aren't being registered, run:

```sh
sessions doctor
```

It checks that the binary is on your `PATH`, that `claude` can be found, that the hook is registered and points at a binary that still exists, that `sessions.json` is readable, and that Claude Code's transcripts still parse. Each problem comes with the command that fixes it. It exits with 1 if it finds a problem, and warnings don't change the exit code.

The transcript check matters after a Claude Code update. The format is internal to Claude Code, so if it changes, `doctor` says so instead of leaving you with an empty list.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SESSIONS_FILE` | `<claude dir>/sessions.json` | Where sessions are stored |
| `CLAUDE_CONFIG_DIR` | `~/.claude` | Claude Code's config dir, also used to find transcripts |
| `SESSIONS_CLAUDE_ARGS` | empty | Extra flags added to every `claude --resume` |
| `SESSIONS_RESUME_REMINDER` | on | Set to `0` to stop the hook asking the agent to re-check title and tags when a session is resumed |

## How it works

On every `SessionStart` Claude Code runs `sessions hook` with a JSON payload on stdin. The hook upserts the session in `sessions.json` and replies with `additionalContext`, which tells the agent the session ID and asks it to set a title once the topic is clear. It also appends `CLAUDE_SESSION_ID` and the binary's directory to `CLAUDE_ENV_FILE`, so the agent's shell can call `sessions` without further setup.

`sessions.json` is a plain JSON array. Writes are locked and atomic, and a file that doesn't parse is moved aside as `sessions.json.corrupt-<timestamp>` rather than overwritten.

The sort order uses the transcript's modification time, which changes on every message, so a long-running session stays near the top.

Claude Code's generated title, git branch and linked pull request are read from the transcript, whose format is internal to Claude Code and may change between versions. If a field disappears, `sessions` shows less rather than failing. The values are copied into `sessions.json`, so an entry stays recognisable after the transcript is gone.

When you resume a session that has a title from `sessions`, the hook also passes it to Claude Code as `sessionTitle`, so `claude --resume` shows it too. Claude Code only honours this at session start, and the hook leaves a name alone if the session already has one (for example from `/rename`). A title set mid-session therefore reaches Claude Code's own picker the next time you resume.

### Transcript retention

Claude Code deletes old transcripts (30 days by default, see `cleanupPeriodDays` in its settings). `sessions` keeps the entry, but a session without a transcript can't be resumed: it is marked `✗`, and `sessions prune` removes such entries. Raise `cleanupPeriodDays` if you want to find sessions for longer.

## Next to `claude --resume`

Use Claude Code's picker to jump back into recent work in the current repository. As documented at the time of writing, it has previews, renaming, a branch filter, worktree grouping and a lookup by pull request URL, and `sessions` does not try to replace those.

Use `sessions` when you want to find work by ticket, branch or note across all projects, add context of your own, or look back further than Claude Code keeps transcripts. Its documentation mentions no tags or notes, and it shows the current repository by default; `Ctrl+A` widens it to all projects.

The two share names where it matters: on resume the hook passes your title to Claude Code, so it shows up in its picker too.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
claude plugin validate --strict .
```

How the program is put together is explained in [Architecture](docs/explanation/architecture.md). How the code is organised, and when to reconsider that, is recorded in [ADR 0001](docs/adr/0001-code-organisation.md). Conventions for contributors are in [AGENTS.md](AGENTS.md).

`install.sh` and `scripts/formula.sh` are checked with `shellcheck`. Pushing a tag like `v0.1.0` builds release archives for macOS and Linux, publishes them as a GitHub release and updates the formula in the [Homebrew tap](https://github.com/JoyoMDEV/homebrew-tap). The tag must match the version in `Cargo.toml` and `.claude-plugin/plugin.json`.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
