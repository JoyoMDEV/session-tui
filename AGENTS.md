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
| `src/commands/` | `title`, `ticket`, `tag`, `note` (`edit.rs`), `list`, `log`, `resume`, `start`, `prune`, `rm`, `import`, `migrate-tickets`, `archive` and `unarchive` (`archive.rs`) |
| `src/output.rs` | Stdout that survives a closed pipe: `say!` and `text`; `println!` is denied by clippy |
| `src/launch.rs` | Replaces the process with `claude` to resume or start a session, for the browser and the commands |
| `src/preset.rs`, `src/paths.rs` | The tickets and title handed to the hook of a started session (`$SESSIONS_PRESET`); directories typed by a person |
| `src/query.rs` | Which sessions the report commands keep (the `list`, `log` and `resume` filters, the browser's search) and the public JSON view |
| `src/store.rs` | `sessions.json`: data model, locking, atomic writes, recovery from a corrupt file |
| `src/transcript.rs` | Read-only access to Claude Code transcripts: import, title/branch/PR scan, preview text |
| `src/tui.rs`, `src/tui/` | The ratatui browser: `app.rs` state and edits, `input.rs` keys and mouse, `draw.rs` rendering; `tui.rs` starts it and launches `claude` |
| `src/setup.rs` | `sessions setup`: merges the hook into Claude Code's `settings.json` |
| `src/tags.rs` | Topic tags: normalisation, removal, the capped vocabulary the hook shows the agent |
| `src/tickets.rs` | Ticket keys: editing, the `ticket:` search syntax, tag migration |
| `src/doctor.rs` | `sessions doctor`: install checks, including whether transcripts still parse |
| `tests/plugin.rs` | Keeps the plugin manifests and the hook consistent with the crate |
| `tests/cli.rs` | Runs the real binary against temporary directories: hook, setup, doctor, edits, list, prune, import |
| `tests/readme.rs` | Keeps the README command list in step with `sessions --help` |
| `tests/docs.rs` | Checks that links in the Markdown files resolve and that `docs/Home.md` links every page in `docs/` |
| `docs/` | `Home.md` (the index), `reference/`, `explanation/`, `how-to/` (maintainer tasks) and `adr/` |
| `.claude/CLAUDE.md`, `.claude/skills/` | Instructions for Claude Code (an import of this file) and project skills for maintainers, not part of the plugin. `pre-release/` is the check before a release (`/pre-release <version>`) |
| `.claude-plugin/`, `hooks/` | Claude Code plugin and marketplace (the repo is its own marketplace) |
| `install.sh` | Release installer |
| `scripts/formula.sh` | Generates the Homebrew formula for a release |
| `scripts/wiki.sh`, `scripts/wiki*.awk` | Builds the wiki pages from `docs/` |
| `scripts/check.sh`, `.githooks/` | The lint checks (CI) and the optional git hooks that call them |
| `tests/hooks.rs` | Runs `scripts/check.sh` on commit messages and on staged files in a scratch repository |
| `tests/wiki.rs` | Runs `scripts/wiki.sh` on examples and on the real `docs/` |
| `.github/workflows/` | `ci.yml`, `release.yml`, `homebrew.yml`, `wiki.yml` |
| `.markdownlint-cli2.yaml` | The Markdown lint: which files, which rules are off and why |

## Build, test, lint

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
claude plugin validate --strict .
shellcheck -s sh install.sh scripts/*.sh .githooks/*
npx --yes markdownlint-cli2@0.23.3
```

`sh scripts/check.sh lint` runs the formatting, clippy, shellcheck and Markdown lint commands above
and is what CI's lint job runs, so the commands and the Markdown lint version are defined there,
in one place. It needs `shellcheck` and Node.

CI runs formatting, clippy, shellcheck and the Markdown lint (`scripts/check.sh lint`) on Linux and the tests on Linux and macOS.
The Markdown lint needs Node. Its rules and the files it covers are in `.markdownlint-cli2.yaml`; it
covers `README.md`, `AGENTS.md`, `docs/` and the skill files, not the pull request template. Rust 1.88 or newer, edition 2024.

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
- Print to stdout only through `output` (`say!` and `text`). `println!` panics on a closed pipe, as
  in `sessions list | head`, so clippy denies it and `print!`; `output` ends quietly instead and
  reports it, so a loop can stop. Diagnostics go to stderr with `eprintln!`.
- A missing environment variable (`HOME`, say) is an error with a message, never a panic, and the
  hook reports a problem on stderr and still prints its JSON.
- Resuming uses `exec`, which is why the tool is Unix only.
- Logic is unit-tested next to the code, and `App` in `tui.rs` can be tested without a terminal. To
  check how the TUI looks and feels, drive it in tmux against a temporary sessions file.
- Git hooks are optional and off by default. `git config core.hooksPath .githooks` turns them on
  (`git config --unset core.hooksPath` turns them off). `pre-commit` refuses a staged
  `sessions.json`, transcript, `.env` or key file, whitespace errors and conflict markers, and
  runs `cargo fmt --check`, `shellcheck` and the Markdown lint on what changed. `commit-msg`
  checks the rules under "Commits". `pre-push` runs clippy and the tests. They call
  `scripts/check.sh` and skip a check whose tool is missing; CI runs every check regardless.
- `.claude/CLAUDE.md` only imports this file (`@../AGENTS.md`), for versions of Claude Code that
  don't read `AGENTS.md` themselves. It is not at the repository root, because the root is the
  plugin root and a `CLAUDE.md` there makes `claude plugin validate --strict .` fail;
  `tests/plugin.rs` checks that.
- After moving or renaming your checkout, run `cargo clean`. `tests/plugin.rs` embeds the manifest
  path at compile time, so a stale build looks for the old location.

## Documentation

- The README is the user documentation. Change it in the same PR as any change in behavior, flags,
  commands or configuration. `tests/readme.rs` fails if a command from `sessions --help` is missing
  from the README.
- Put something in `docs/` only when it is too long for the README: a reference (the `sessions.json`
  format), an explanation (how the hook works, why transcripts are read tolerantly) or a how-to for
  maintainers. Keep the [Diátaxis](https://diataxis.fr) kinds apart: `docs/reference/`,
  `docs/explanation/` and `docs/how-to/`, one kind per page. How-tos for users and the tutorial
  stay in the README; `docs/how-to/` is for tasks only maintainers do, such as making a release.
- `docs/Home.md` is the index, written by hand and organised by those four kinds. Link every new
  page from it. The wiki is generated from `docs/` on release and builds its sidebar from
  `Home.md`; edit `docs/`, never the wiki (see `docs/how-to/publish-the-wiki.md`).
  `tests/docs.rs` fails on a broken relative link or heading anchor and on a page that `Home.md`
  does not link. Write links between pages as relative Markdown links, and link the README for the
  tutorial and user how-tos.
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
- CI must pass. PR checks don't run `release.yml`, `homebrew.yml` or `wiki.yml`, so say how you verified changes
  to them, for example a manual run of the workflow.
- Merge with squash and delete the branch.
- Dependabot PRs: green CI is not enough for workflow changes. Read the release notes for breaking
  changes that touch the inputs we use.

## Releases

- The version appears in `Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json`, and they must
  match. `cargo build` updates the lock file, and `tests/plugin.rs` checks the plugin manifest.
- Push a tag `vX.Y.Z` on a green `main`. `release.yml` checks that the tag equals the crate version,
  builds four targets, publishes the archives with their SHA-256 files, and then calls `homebrew.yml`
  to update the formula in the tap repository and `wiki.yml` to publish the docs of the tag to the
  wiki. The Homebrew job authenticates with the `TAP_DEPLOY_KEY` secret, a deploy key that can write
  to the tap only; the wiki job uses the repository's own token and writes to the wiki only.
- To republish a formula or the wiki for an existing release, run the Homebrew or the Wiki workflow
  by hand with the tag (`docs/how-to/release.md` has the commands).

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
