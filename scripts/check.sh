#!/bin/sh
# The project's checks in one place, used by CI, the git hooks and by hand.
#
#   scripts/check.sh lint             format, clippy, shellcheck and the Markdown lint (CI's lint job)
#   scripts/check.sh pre-commit       fast checks on what is staged (the pre-commit hook)
#   scripts/check.sh commit-msg FILE  the commit message rules of AGENTS.md (the commit-msg hook)
#   scripts/check.sh pre-push         clippy and the tests (the pre-push hook)
#
# A missing tool fails a check, unless CHECK_SKIP_MISSING=1: the hooks set it, so a contributor
# without Node can still commit, and CI, which does not, never lets a check go unrun.
set -eu

# The Markdown lint version. Pinned so a new release can't fail the build unannounced; bump it here.
MARKDOWNLINT=markdownlint-cli2@0.23.3

COMMIT_TYPES='feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert'

die() {
    echo "check.sh: $*" >&2
    exit 1
}

# Succeeds if the tool can be run. A missing one fails, or is skipped with CHECK_SKIP_MISSING=1.
have() {
    command -v "$1" >/dev/null 2>&1 && return 0
    [ "${CHECK_SKIP_MISSING:-}" = 1 ] || die "$1 is not installed"
    echo "check.sh: skipped $2: $1 is not installed" >&2
    return 1
}

shellcheck_files() {
    have shellcheck "the shell script check" || return 0
    shellcheck -s sh "$@"
}

markdownlint() {
    have npx "the Markdown lint" || return 0
    npx --yes "$MARKDOWNLINT"
}

lint() {
    cd "$(dirname "$0")/.."
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings
    shellcheck_files install.sh scripts/*.sh .githooks/*
    markdownlint
}

pre_push() {
    cd "$(dirname "$0")/.."
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
}

# What is staged, one path per line. The checks read the files in the working tree, which is the
# same as the staged content unless a file is only partly staged.
staged() {
    git diff --cached --name-only --diff-filter=ACMR
}

pre_commit() {
    files=$(staged)
    [ -n "$files" ] || return 0

    # Files that must never be committed (AGENTS.md, "Boundaries"): a real sessions file,
    # transcripts, environment files and keys.
    bad=$(printf '%s\n' "$files" | grep -E '(^|/)(sessions\.json|\.env(\..*)?|id_[a-z0-9]+)$|\.(jsonl|pem|key)$' || true)
    if [ -n "$bad" ]; then
        echo "check.sh: these files must not be committed:" >&2
        printf '  %s\n' "$bad" >&2
        exit 1
    fi

    # Whitespace errors and leftover conflict markers.
    git diff --cached --check || die "whitespace errors or conflict markers in the staged changes"

    if printf '%s\n' "$files" | grep -Eq '\.rs$|(^|/)Cargo\.(toml|lock)$'; then
        cargo fmt --check || die "run cargo fmt"
    fi

    scripts=$(printf '%s\n' "$files" | grep -E '\.sh$|^\.githooks/' || true)
    if [ -n "$scripts" ]; then
        # The list is one path per line and paths have no spaces here; word splitting is intended.
        # shellcheck disable=SC2086
        shellcheck_files $scripts
    fi

    if printf '%s\n' "$files" | grep -Eq '\.md$|^\.markdownlint'; then
        markdownlint
    fi
}

# The message in the file named by git: a Conventional Commit subject of at most 72 characters
# (50 is the aim) without a trailing period, no attribution trailers, and a wrapped body.
commit_msg() {
    [ $# -eq 1 ] || die "usage: check.sh commit-msg <message-file>"
    message=$(grep -v '^#' "$1" || true)
    subject=$(printf '%s\n' "$message" | sed -n '1p')
    [ -n "$subject" ] || die "empty commit message"

    # Merges, reverts and fixups made by git are not written by hand.
    case $subject in
        'Merge '* | 'Revert "'* | 'fixup! '* | 'squash! '*) return 0 ;;
    esac

    failed=0
    fail() {
        echo "check.sh: commit message: $*" >&2
        failed=1
    }

    printf '%s\n' "$subject" | grep -Eq "^($COMMIT_TYPES)(\\([a-z0-9._/-]+\\))?!?: ." ||
        fail "the subject must read 'type(scope): subject' with a type from: $(echo "$COMMIT_TYPES" | tr '|' ' ')"
    length=$(printf '%s' "$subject" | wc -m | tr -d ' ')
    [ "$length" -le 72 ] || fail "the subject is $length characters; 72 is the hard limit, 50 the aim"
    [ "$length" -le 50 ] || [ "$length" -gt 72 ] || echo "check.sh: note: the subject is $length characters; 50 is the aim" >&2
    case $subject in *.) fail "the subject must not end with a period" ;; esac

    if printf '%s\n' "$message" | grep -Eiq '^(co-authored-by:|.*generated with)'; then
        fail "no co-author or tool attribution trailers and no 'generated with' lines (AGENTS.md, Commits)"
    fi

    long=$(printf '%s\n' "$message" | sed '1d' | awk 'length($0) > 72 && $0 !~ /https?:\/\// { n++ } END { print n + 0 }')
    [ "$long" -eq 0 ] || echo "check.sh: note: $long body line(s) are longer than 72 characters" >&2

    [ "$failed" -eq 0 ] || exit 1
}

case ${1:-} in
    lint) lint ;;
    pre-commit) pre_commit ;;
    commit-msg) shift && commit_msg "$@" ;;
    pre-push) pre_push ;;
    *) die "usage: check.sh lint | pre-commit | commit-msg <file> | pre-push" ;;
esac
