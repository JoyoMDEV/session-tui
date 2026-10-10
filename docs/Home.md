# sessions documentation

`sessions` keeps a searchable log of your [Claude Code](https://claude.com/claude-code) sessions.
This page is the index of the documentation. It is organised after [Diátaxis](https://diataxis.fr):
what you need depends on whether you want to learn, to get something done, to look something up, or
to understand why it works the way it does.

## Tutorial

Start here if you have not used `sessions` before. These sections of the README take you from
nothing to a working log.

- [Install](../README.md#install)
- [Set up the hook](../README.md#set-up-the-hook)
- [Use](../README.md#use): the browser, its keys and the commands

## How-to guides

Steps for a specific task.

- [Find sessions by ticket, tag, branch or directory](../README.md#filtering-list-and-log)
- [Write a timeline of your work for a ticket or a report](../README.md#timeline-with-log)
- [Resume a session from the shell](../README.md#resume-from-the-shell)
- [Keep tags and tickets tidy](../README.md#tags) and [record tickets](../README.md#tickets)
- [Fix a problem](../README.md#troubleshooting)
- [Contribute, build, test and release](../AGENTS.md): conventions and process for maintainers.
  Longer maintainer how-tos will live in `docs/how-to/` and be listed here.

## Reference

Facts to look up.

- [Commands](../README.md#commands)
- [Configuration](../README.md#configuration)
- [`sessions list --json`](reference/list-json.md): the keys of the JSON output

## Explanation

Background and reasons.

- [Architecture](explanation/architecture.md): how the program is put together and why
- [How it works](../README.md#how-it-works): the hook, the file and the transcripts
- [Transcript retention](../README.md#transcript-retention): why a session can lose its transcript
- [Next to `claude --resume`](../README.md#next-to-claude---resume): what `sessions` adds

## Decisions

Architectural decision records, written when a choice is costly to reverse.

- [ADR 0001: One package with a library crate, split by responsibility](adr/0001-code-organisation.md)
