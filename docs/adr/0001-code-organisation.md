---
status: accepted
date: 2026-10-06
decision-makers: Johannes Moseler
---

# One package with a library crate, split by responsibility

## Context and Problem Statement

`sessions` has grown to about 4,000 lines in one binary crate. `main.rs` held the command line,
the hook and every small command, `tui.rs` is close to 900 lines, and unit tests could not be
reached from `tests/` because a binary-only crate cannot be imported. More work is planned: several
Claude config directories (v0.3.0) and more than one coding agent (v1.0.0). How should the code be
organised so that it stays easy to change, and when should that be reconsidered?

## Decision Drivers

* Behavior must not change while the code moves, so the real binary needs tests from outside.
* Few, cohesive files over many tiny ones. One file per concern, named for what it owns.
* Follow what mature Rust command line and terminal UI projects do, so the layout is familiar.
* Do not add structure before there is a reason for it.

## Considered Options

* A cargo workspace with several crates (core, transcript reader, TUI, CLI).
* One package with a library crate and a thin `main.rs`, split into modules by responsibility.
* Keep a binary-only crate and split only `main.rs`.

## Decision Outcome

Chosen option: "One package with a library crate and a thin `main.rs`", because it is what projects
of this size do (bat, bottom, gitui keep a single package; ripgrep, atuin and yazi use workspaces
for reusable libraries or dozens of crates, which `sessions` has not reached), and it removes the
binary-only restriction without any extra build machinery.

The rules that follow from it:

* `main.rs` only calls `sessions::run()`. All behavior lives in the library.
* The command line is defined in `cli.rs`. The hook is `hook.rs`. Each non-trivial command is one
  file under `commands/`. `lib.rs` parses and dispatches.
* Modules are `foo.rs` plus a `foo/` directory when they have cohesive children, not `foo/mod.rs`.
  A module becomes a directory when it has parts that can be read and tested on their own. Today
  that applies to `tui` (state, input, drawing). `store`, `transcript`, `setup` and `doctor` stay
  single files.
* Unit tests stay inline in `#[cfg(test)] mod tests`. A test module moves to a sibling `tests.rs`
  only when it is larger than the code it tests or needs shared fixtures. No module qualifies yet.
* `tests/` holds tests that run the real binary against temporary directories (`tests/cli.rs`) and
  checks that documentation, manifests and code agree (`tests/readme.rs`, `tests/plugin.rs`).
* Drawing is tested with ratatui's `TestBackend` and direct assertions on the buffer, not with
  snapshot files, so there is no extra dependency.
* Modules that are not public API are private or `pub(crate)`.

### Consequences

* Good, because the behavior tests in `tests/` can stay unchanged while the code moves.
* Good, because each command and each part of the TUI can be read on its own.
* Neutral, because every file is a little smaller but there are more of them.
* Bad, because a library crate with private modules cannot be used by other projects. That is fine
  until there is a second consumer.

### Confirmation

`cargo test --locked` runs the real-binary tests. Review checks that new code follows the rules
above. The section "When to reconsider" below is the checklist for changing them.

## When to reconsider

Revisit this decision, and write a new ADR that supersedes it, when one of these happens:

* **A second consumer of the code appears**, for example a separate binary or a library other
  projects depend on. Then split a crate along that boundary, not before.
* **Several coding agents are supported (v1.0.0).** If reading each agent's transcripts needs its
  own dependencies or release cadence, a crate for the transcript readers is worth considering.
  A trait or module per agent inside this package comes first.
* **A file passes about 1,000 lines** without a natural place to cut it. Mature projects keep
  files of 500 to 1,500 lines, so this is a prompt to look, not a rule.
* **A test module grows past the code it tests**, or several modules need the same fixtures. Move
  it to `tests.rs`, or to a shared helper under `tests/`.
* **Build or test time becomes a problem.** A workspace lets cargo rebuild less. It is not one yet.
* **Drawing bugs slip through** the buffer assertions. Consider snapshot tests (`insta`) then.

## Pros and Cons of the Options

### A cargo workspace with several crates

* Good, because it enforces boundaries and can speed up incremental builds.
* Bad, because it adds manifests, versions and release steps for no second consumer.
* Bad, because the code is small enough that boundaries are cheaper as modules.

### One package with a library crate and a thin `main.rs`

* Good, because it matches how comparable projects are built and costs nothing extra.
* Good, because `tests/` and unit tests both keep working.
* Bad, because the boundaries are by convention only, which is why the rules are written down.

### Keep a binary-only crate and split only `main.rs`

* Good, because it needs no change to the crate layout.
* Bad, because nothing outside the crate can import it, so behavior tests can only drive the binary.

## More Information

The layout of mature projects was read from their public repositories on 2026-10-06: ripgrep,
gitui, atuin, bottom, yazi, helix, bat and the ratatui templates. Where they disagree (a sibling
`tests.rs`, `mod.rs` against `foo.rs` plus `foo/`, snapshot tests), this ADR picks the simpler
option. Format: [MADR 4.0.0](https://adr.github.io/madr/).
