# What the audit checks

Run each check against the worktree. A finding names `file:line` and the rule. Where two things
contradict, quote both and say which one is probably wrong; do not decide silently.

## A. Documentation against code

- The layout table in AGENTS.md against `git ls-files src tests scripts .github .claude .claude-plugin
  hooks docs`: files that exist but are missing from the table, rows for files that do not exist.
- Commands: `cli.rs` against the README table, the command list in AGENTS.md (`src/commands/` row)
  and `docs/explanation/architecture.md`. `tests/readme.rs` only covers the README.
- Keys: every `KeyCode` arm in `tui/input.rs` against the README key table and the help line in
  `tui/draw.rs`. Flags: every `#[arg]` in `cli.rs` against the README.
- `docs/reference/list-json.md` against the fields of `query::View`.
- Words that go stale: search the docs for `planned`, `will`, `not yet`, `TODO`, `soon`, `later`
  and check each against what exists. Examples naming an old version are fine.
- ADRs: what each says against the code. An accepted ADR is not edited; if the code moved on, say
  so as a note, and a superseding ADR is only warranted when a decision changed.

## B. The rules in AGENTS.md

- `sessions.json` is written only through `store::update`. Search the non-test code for
  `fs::write`, `File::create`, `fs::rename`, `OpenOptions` and check each one.
- Stdout goes through `output` (`say!`, `text`), and `#![deny(clippy::print_stdout)]` in `lib.rs`
  enforces it: look for a removed deny, an `#[allow(clippy::print_stdout)]`, or output written
  with `write!` to `std::io::stdout()` outside `output.rs` whose error is ignored or unwrapped.
  `eprintln!` for diagnostics is fine.
- Transcripts are parsed tolerantly: in `transcript.rs`, `doctor.rs` and anything that reads
  Claude Code's files, look for `unwrap`, `expect`, indexing and `?` on a record that Claude Code
  owns. A missing record must mean less information, not an error.
- The hook: stdout is exactly one JSON object, diagnostics go to stderr, no network access (search
  for `TcpStream`, `reqwest`, `ureq`, `curl`), no full-transcript scan.
- `exec` for resuming lives in `launch.rs` only.
- Edits to `settings.json` keep the key order, leave a backup and refuse invalid JSON.
- Environment variables that can be unset (`HOME`, `CLAUDE_CONFIG_DIR`) do not panic.

## C. Structure (ADR 0001, architecture page)

- `main.rs` only calls `sessions::run()`. Modules in `lib.rs` are private; flag `pub mod`.
- Dependency direction: `store`, `transcript`, `tags`, `tickets` and `query` must not use
  `commands`, `hook`, `tui` or `cli`. Search each for `use crate::`.
- Every module file starts with a `//!` comment that says what it owns and what it must not do.
- Tests are inline in `#[cfg(test)] mod tests`; a sibling `tests.rs` needs a reason. No `mod.rs`.
- A file past about 1,000 lines is a prompt to look (ADR 0001, "When to reconsider").

## D. Tests

- Tests and fixtures use temporary directories and set `SESSIONS_FILE`, `CLAUDE_CONFIG_DIR` and
  `HOME`; nothing reads or writes `~/.claude`. Search `tests/` and test modules for `.claude`,
  `HOME` and `home_dir`.
- No test needs the network or a real `claude`. Tests that start a program use a fake on `PATH`.
- Test names say what is guaranteed. A test that cannot fail (asserting what the setup just wrote)
  is a finding.

## E. Dependencies and workflows

- `git diff <last-tag> -- Cargo.toml Cargo.lock`: each new dependency, why it is needed, how
  maintained. `cargo tree -d` for duplicates. Licences stay compatible with `MIT OR Apache-2.0`.
  Run `cargo audit` or `cargo deny` if installed, otherwise report them as not run.
- Workflows: minimal `permissions`, no `pull_request_target`, no secret echoed, third-party actions
  pinned (by tag with Dependabot, or by SHA), `release.yml` still checks that the tag equals the
  crate version, only one job writes to the wiki and it writes there only.

## F. Practices that were not intended

- `unsafe`; `unwrap()`/`expect()`/`panic!`/`todo!`/`unimplemented!`/`dbg!` outside tests (each
  one needs a reason, an infallible case is fine); `#[allow(` and `#[cfg_attr(.*allow`;
  `std::process::exit` outside `main`; `TODO`/`FIXME`/`XXX`; commented-out code; leftover debug
  output; `clone()` on large data in a loop where a borrow would do.

## G. Repository hygiene

- `git ls-files` for names that must not be committed: `sessions.json`, `*.jsonl`, `.env*`,
  `*.pem`, `*.key`, anything with `secret` or `token` in the name. Search the diff for pasted
  tokens and for content that looks like a transcript.
- Authors in the range (`git log <last-tag>..HEAD --format='%an <%ae>'`) are the repository
  identity only, never a work identity or a work email.
- Commits in the range: Conventional Commit type from the list in AGENTS.md, subject at most 72
  characters, no `Co-Authored-By` or "generated with" trailers (search for those two phrases only; the word "claude" appears in ordinary subjects), breaking changes marked with `!`
  and a `BREAKING CHANGE:` footer.

## H. Release mechanics

- The version in `Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json` agree with each other
  and with the tag about to be made; `tests/plugin.rs` passes.
- `.claude-plugin/plugin.json`, `marketplace.json` and `hooks/hooks.json` are consistent
  (`claude plugin validate --strict .`). A newer Claude Code can add a warning that `--strict`
  turns into a failure, so run it with the installed version and report the exact message.
- Issues of the milestone: open ones that block the release; closed ones that no merged pull
  request in the range closes, and merged pull requests without a link to an issue
  (`gh issue list --milestone`, `gh pr list --state merged`).
- Release notes draft: group the range by commit type with pull request numbers, breaking
  changes first, so `gh release create --generate-notes` can be compared against it.
