# Architecture

How `sessions` is put together and why. For the commands, see the [README](../../README.md). For
the rules behind the layout and when to change them, see
[ADR 0001](../adr/0001-code-organisation.md).

## What the program is

One package, one binary. `src/main.rs` calls `sessions::run()` in the library crate, which parses
the command line and hands the command to the module that implements it. With no command it starts
the terminal browser.

There are three ways in, and they share the same data:

* **The hook** runs at every Claude Code session start and registers the session.
* **The commands** (`title`, `tag`, `ticket`, `note`, `list`, `log`, `resume`, `start`, `prune`, `rm`, `import`,
  `migrate-tickets`) are called by the agent, by you, or from scripts.
* **The browser** lists, searches and edits the same sessions and resumes one.

All of them read and write `sessions.json`. Claude Code's transcripts are only ever read.

## Modules

| Module | Owns | Does not |
| --- | --- | --- |
| `store` | The data model and every write to `sessions.json`: lock, atomic replace, recovery from a corrupt file | Read transcripts or draw anything |
| `transcript` | Reading Claude Code's transcripts: import, generated title, branch, pull request, preview text | Write transcripts, or fail on a record it doesn't understand |
| `tags`, `tickets` | The rules for topic tags and ticket keys: spelling, editing, search syntax, migration | Touch the file themselves |
| `query` | Choosing sessions (the filters, the search box's matching) and the JSON view scripts read | Print or change anything |
| `output` | Standard output that ends quietly on a closed pipe | Print anything itself |
| `launch` | Replacing the process with `claude`, to resume a session or start one, for the browser and the commands | Decide which session to resume |
| `preset` | The tickets and title handed from `start` to the hook of the new session in `$SESSIONS_PRESET` | Read or write the sessions file |
| `paths` | Directories a person types: `~` and `$HOME`, and a check that the directory exists | Change the file system |
| `hook` | The answer to `SessionStart` | Print anything but one JSON object on stdout |
| `commands/` | One small command each, working on parsed arguments | Parse the command line |
| `cli` | The clap definition of all commands | Decide what a command does |
| `setup`, `doctor` | `setup` merges the hook into Claude Code's `settings.json`; `doctor` checks the install | Touch a `settings.json` that isn't valid JSON (`setup`), or change anything (`doctor`) |
| `tui/` | The browser: `app` (state and edits, including the new session dialog), `input` (keys and mouse), `draw` (rendering) | Change state while drawing, except the page size that paging needs |

Dependencies only point one way: `commands`, `hook` and `tui` use `store`, `transcript`, `tags`
and `tickets`, and none of those four uses `commands`, `hook`, `tui` or `cli`.

## Decisions that shape the code

**One writer path.** Every change to `sessions.json` goes through `store::update`: it takes an
exclusive lock, reads the current file, applies the change and replaces the file atomically. The
hook, the agent's commands and the browser can run at the same time, and none of them overwrites
another's change. A file that does not parse is moved aside rather than overwritten, so a bad
edit by hand never costs the log.

**Transcripts are a foreign format.** Claude Code does not promise a transcript format, and it
changes between versions. `transcript` reads what it needs and treats a missing or renamed record
as less information, not as an error. Values that matter later (generated title, branch, pull
request) are copied into `sessions.json`, so an entry stays recognisable after Claude Code deletes
its transcript.

**The hook must not get in the way.** It runs at every session start. It prints exactly one JSON
object, sends diagnostics to stderr, does no network access and does not scan whole transcripts. A
hook that was slow or noisy would be noticed in every session, so it does less than the browser.

**Tags and tickets are separate fields.** Tags are topics and tickets are references to an issue
tracker, so they have their own fields and their own search (`ticket:ABC-123`). Nothing is guessed
from branch names, and `sessions tag` and the browser's tag editor refuse a ticket-shaped tag and
say where it belongs.

**The browser is testable without a terminal.** `App` holds all state and changes it through
methods, so tests drive it directly. Drawing reads the `App` and writes a buffer, and tests render
into ratatui's `TestBackend` and look at the text. Resuming replaces the process with
`claude --resume` through `exec`, which is why the tool is Unix only.

## How it is tested

* Unit tests sit next to the code in `#[cfg(test)] mod tests`.
* `tests/cli.rs` runs the compiled binary with `HOME`, `CLAUDE_CONFIG_DIR` and `SESSIONS_FILE` in a
  temporary directory and checks output, exit status and files. It is the safety net for
  refactoring, because it does not know how the code is organised.
* `tests/readme.rs` fails if a command is missing from the README, `tests/docs.rs` fails on a
  broken link between documentation pages or a page the index does not list, and `tests/plugin.rs`
  keeps the plugin manifests and the hook in step with the crate.

## What is likely to change

The `store` module assumes one Claude config directory and one coding agent. Planned work (several
config directories, then more than one agent) will touch `store`, `transcript` and `setup` first.
ADR 0001 lists what would justify splitting them into more modules or crates.
