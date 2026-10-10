---
name: pre-release
description: Use when the maintainer is about to tag a release of this repository and asks whether main is ready, for a release check, a release review, or release notes
argument-hint: "[version, e.g. 0.2.0]"
disable-model-invocation: true
---

# Pre-release check for sessions

Audit everything that changed since the last tag, find contradictions and practices the project
did not intend, and say what to do about each finding. The audit is read-only. It never tags,
publishes, pushes without asking, changes repository settings, or touches secrets or real user
data (AGENTS.md, "Boundaries").

Violating the letter of these steps is violating the spirit of the check.

## Steps

1. **Set up.** Last tag: `git describe --tags --abbrev=0`. Version to release: the argument, else
   `version` in `Cargo.toml`. Make a detached worktree of `HEAD` in a temporary directory
   (`git worktree add --detach "$(mktemp -d)" HEAD`) and run everything below there, so stray local
   files cannot hide a problem. Point `SESSIONS_FILE` and `CLAUDE_CONFIG_DIR` at fresh `mktemp -d`
   directories for every command. Remove the worktree at the end.
2. **Run the tools.** Every command in AGENTS.md "Build, test, lint", plus `cargo build --locked`.
   Record each as passed, failed or **not run** with the reason (a missing tool is not a pass).
3. **Audit the code in full.** Dispatch one subagent per area, in parallel, each told to read every
   file of its area completely (not grep) and to apply [checks.md](checks.md): `store`,
   `transcript`, `setup`, `doctor`; `hook`, `cli`, `commands/`, `launch`, `query`, `tags`,
   `tickets`; `tui/`; `tests/`, `scripts/`, `.github/`, `.claude-plugin/`, `hooks/`; and the
   documentation (`README.md`, `AGENTS.md`, `docs/`). Each returns findings with `file:line`.
4. **Review the range.** Commits since the tag against AGENTS.md "Commits", issues of the
   milestone against the pull requests that closed them, dependency changes, committed files
   that must not be there. See checks.md, sections E to H.
5. **Report** in the format below, then stop and ask which actions to take.
6. **Act only on what the maintainer picks**, following AGENTS.md: a small, clear problem gets a
   branch, a Conventional Commit and a pull request (ask before pushing); a larger or unclear one
   gets an issue; a blocker gets a recommendation not to tag. When everything is clear, offer the
   release pull request: the version in `Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json`
   (`cargo build` updates the lock file), and the release notes. Print the tag command; the
   maintainer runs it.

## Report format

- **Verdict**: ready, ready after fixes, or not ready, in one line.
- **Findings**, most severe first, each with: what, evidence (`file:line`, or both sides of a
  contradiction), the rule it breaks (AGENTS.md, an ADR, the README), and the action (pull
  request, issue, stop or none).
  - *Blocker*: tagging or running the release would fail or ship something wrong.
  - *Fix before release*: documentation or code that contradicts itself or the rules.
  - *Note*: worth knowing, no action needed.
- **Checks run**: each command and its result, with the ones not run named.
- **Coverage**: files read completely, by area, and what was only searched.
- **Release notes draft**: commits since the tag grouped by type, with pull request numbers,
  breaking changes first.
- **Proposed actions**: a numbered list the maintainer can pick from.

## Common mistakes

| Mistake | Instead |
| --- | --- |
| Judging a file from grep output | Read it; coverage states what was read |
| A tool is missing, so the check is skipped silently | Report it as not run |
| Fixing while auditing | Report first, act after the maintainer picks |
| A contradiction reported from one side only | Quote both sides and say which is likely wrong |
| "Looks fine" without evidence | Name what was checked and where |
