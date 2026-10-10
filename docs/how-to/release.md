# Make a release

This page is for maintainers. It walks through a release from the check before it to the checks
after it. The rules it follows are in [AGENTS.md](../../AGENTS.md), "Releases" and "Boundaries".

## Before you tag

1. Be on a green `main` with nothing unmerged that belongs in the release. Check the milestone
   for open issues that should block it.
2. Run the pre-release check from the repository root in Claude Code:

   ```text
   /pre-release 0.2.0
   ```

   It is a project skill in `.claude/skills/pre-release/`, run by hand and with your own login,
   and it is not part of the plugin that users install. It audits everything since the last tag
   in a clean worktree: the tools from AGENTS.md "Build, test, lint", the code against the
   documented design, contradictions between documents and code, practices the project did not
   intend, the commits and the milestone, and the version in the three places. It reads
   only. It ends with a report: a verdict, findings ordered by severity with the file and line,
   the checks that were run or not run, which files were read, a draft of the release notes, and
   a numbered list of proposed actions.
3. Pick what to do about each finding. A small, clear problem becomes a branch and a pull
   request, a larger one an issue, and a blocker means no tag yet. The skill asks before it pushes
   anything. Fix, merge, and run the check again until the verdict is "ready".

## The release pull request

When the check is ready, the skill can prepare the release pull request. It changes the version in
`Cargo.toml`, `Cargo.lock` and `.claude-plugin/plugin.json` to the same value (`cargo build`
updates the lock file; `tests/plugin.rs` checks the plugin manifest) in a commit such as
`chore: bump version to 0.2.0`. Review it like any other pull request and merge it with squash.

## Tag

Pull `main` and tag the merge commit. The skill prints the commands, but does not run them:

```sh
git switch main && git pull --ff-only
git tag v0.2.0
git push origin v0.2.0
```

Pushing the tag starts `release.yml`. It refuses a tag that differs from the crate version, builds
the four targets, publishes the archives with their SHA-256 files and generated release notes,
and then calls two more workflows: `homebrew.yml` updates the formula in the tap, and `wiki.yml`
publishes the documentation of the tag to the wiki.

## After the tag

Look at each result, because a failed step does not undo the others:

- The GitHub release has four archives and their checksums. Edit the generated notes if the
  draft from the check says it better.
- `brew update && brew upgrade sessions` installs the new version, or `brew install
  JoyoMDEV/tap/sessions` on a machine without it. `sessions --version` shows it.
- The wiki's Home page shows the release tag in the footers of its pages, see
  [Publish the documentation to the wiki](publish-the-wiki.md).

If the formula or the wiki step failed, run that workflow again by hand with the tag, for example
`gh workflow run homebrew.yml -f tag=v0.2.0` or `gh workflow run wiki.yml -f ref=v0.2.0`. Fix the
cause first if it was a workflow bug, in a pull request like any other change.

## What the skill never does

It does not push a tag, publish a release, change repository settings or secrets, or touch real
user data. Its commands run against temporary directories. Everything it proposes is for you to
accept.
