# AGENTS.md

Guidance for people and coding agents working on this repository.

## What this is

`sessions` is a Rust command line tool and terminal UI that keeps a searchable log of
[Claude Code](https://claude.com/claude-code) sessions. A `SessionStart` hook registers every session
and asks the agent to title and tag it. The TUI lists them, and Enter resumes one in its original
directory. Unix only (macOS, Linux). The README is the user documentation.

## Layout

| Path | What it does |
| --- | --- |
| `src/main.rs`, `src/lib.rs` | `main.rs` calls `sessions::run()`; `lib.rs` parses the command line and dispatches |
| `src/cli.rs` | The clap definition of every command |
| `src/hook.rs` | The `SessionStart` hook |
| `src/commands/` | `title`, `ticket`, `tag`, `note` (`edit.rs`), `list`, `prune`, `rm`, `import`, `migrate-tickets` |
| `src/store.rs` | `sessions.json`: data model, locking, atomic writes, recovery from a corrupt file |
| `src/transcript.rs` | Read-only access to Claude Code transcripts: import, title/branch/PR scan, preview text |
| `src/tui.rs` | The ratatui browser |
| `src/setup.rs` | `sessions setup`: merges the hook into Claude Code's `settings.json` |
| `src/tags.rs` | Topic tags: normalisation, removal, the capped vocabulary the hook shows the agent |
| `src/tickets.rs` | Ticket keys: editing, the `ticket:` search syntax, tag migration |
| `src/doctor.rs` | `sessions doctor`: install checks, including whether transcripts still parse |
| `tests/plugin.rs` | Keeps the plugin manifests and the hook consistent with the crate |
| `tests/cli.rs` | Runs the real binary against temporary directories: hook, setup, doctor, edits, list, prune, import |
| `tests/readme.rs` | Keeps the README command list in step with `sessions --help` |
| `.claude-plugin/`, `hooks/` | Claude Code plugin and marketplace (the repo is its own marketplace) |
| `install.sh` | Release installer |
| `scripts/formula.sh` | Generates the Homebrew formula for a release |
| `.github/workflows/` | `ci.yml`, `release.yml`, `homebrew.yml` |

## Build, test, lint

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
claude plugin validate --strict .
shellcheck -s sh install.sh scripts/formula.sh
```

CI runs formatting, clippy and shellcheck on Linux and the tests on Linux and macOS. Rust 1.88 or newer, edition 2024.

## Working on the code

- Never touch real user data in tests or experiments. The binary reads and writes `$SESSIONS_FILE`
  (default `<claude dir>/sessions.json`) and, under `$CLAUDE_CONFIG_DIR` (default `~/.claude`), the
  transcripts and `settings.json`. Point both at temporary directories.
- Transcripts are Claude Code's internal format and change between versions. Parse tolerantly: a
  missing or renamed record means less information, never an error that stops the user.
- The hook runs at every session start and must not get in the way. Its stdout is exactly one JSON
  object, diagnostics go to stderr, and it does no network access and no full-transcript scans.
- All writes to `sessions.json` go through `store::update` (exclusive lock, atomic replace).
- Edits to the user's `settings.json` keep the key order, leave a backup, and never touch a file that
  isn't valid JSON.
- Use `writeln!` and handle the error for output that may be piped. `println!` panics on a closed
  pipe, as in `sessions list | head`.
- Resuming uses `exec`, which is why the tool is Unix only.
- Logic is unit-tested next to the code, and `App` in `tui.rs` can be tested without a terminal. To
  check how the TUI looks and feels, drive it in tmux against a temporary sessions file.
- After moving or renaming your checkout, run `cargo clean`. `tests/plugin.rs` embeds the manifest
  path at compile time, so a stale build looks for the old location.

## Documentation

- The README is the user documentation. Change it in the same PR as any change in behavior, flags,
  commands or configuration. `tests/readme.rs` fails if a command from `sessions --help` is missing
  from the README.
- Put something in `docs/` only when it is too long for the README: a reference (the `sessions.json`
  format) or an explanation (how the hook works, why transcripts are read tolerantly). Keep the
  [Diátaxis](https://diataxis.fr) kinds apart: `docs/reference/` and `docs/explanation/`, one kind
  per page. How-to steps and the tutorial stay in the README. Link every page from the README.
- Record a decision that is costly to reverse as an ADR in `docs/adr/`, numbered `NNNN-title.md`,
  in the [MADR](https://adr.github.io/madr/) format. Don't edit an accepted ADR; supersede it with a
  new one.
- Each module starts with a `//!` comment saying what it owns and what it must not do. Comment the
  why of non-obvious code, not the what.
- `AGENTS.md` holds conventions and process, not user documentation. When the layout table or a
  command here goes stale, fix it in the PR that made it stale.

## Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): subject`.

- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Subject: imperative mood, 50 characters or fewer (72 is the hard limit), no trailing period.
- One logical change per commit. If the subject needs "and", split it.
- Add a body only when the reason isn't obvious from the diff: why this approach, a trade-off, a
  change in behavior. Wrap at 72 characters.
- Mark breaking changes with `!` after the type and a `BREAKING CHANGE:` footer.
- Every commit should build and pass the tests on its own.
- Don't add co-author or tool attribution trailers, or "generated with" lines, to commits or pull requests.

Examples from this repository:

```text
feat(tui): add paging, mouse wheel and scrollbar
feat: add --version flag
ci: add lint and test workflow
```

## Pull requests

- Branch from `main` as `feat/<topic>`, `fix/<topic>` and so on.
- The title follows the commit subject rules, because a squash merge turns it into the commit.
- Describe four things: what changed, why now, the exact commands to verify it, and anything
  deliberately left out or still to do. `.github/pull_request_template.md` has the headings.
- Keep it to one concern and roughly 400 lines of diff or less, not counting `Cargo.lock`.
- CI must pass. PR checks don't run `release.yml` or `homebrew.yml`, so say how you verified changes
  to them, for example a manual run of the workflow.
- Merge with squash and delete the branch.
- Dependabot PRs: green CI is not enough for workflow changes. Read the release notes for breaking
  changes that touch the inputs we use.

## Releases

- The version appears in `Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json`, and they must
  match. `cargo build` updates the lock file, and `tests/plugin.rs` checks the plugin manifest.
- Push a tag `vX.Y.Z` on a green `main`. `release.yml` checks that the tag equals the crate version,
  builds four targets, publishes the archives with their SHA-256 files, and then calls `homebrew.yml`
  to update the formula in the tap repository. That job authenticates with the `TAP_DEPLOY_KEY`
  secret, a deploy key that can write to the tap only.
- To republish a formula for an existing release, run the Homebrew workflow by hand with the tag.

## Boundaries

Ask before you:

- push, tag, create a release, or change repository settings, visibility or secrets,
- rewrite history that has been pushed.

Never:

- commit secrets, a real `sessions.json`, or transcript content,
- override the repository's commit identity with a global or work identity.

## Writing

Plain, direct language in the README, commit messages and PR descriptions, without filler or
superlatives. Statements about how Claude Code behaves should say they come from its documentation
and may change. Don't claim something works unless you ran it.
