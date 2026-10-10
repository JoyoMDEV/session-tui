#!/bin/sh
# Builds the wiki pages from docs/.
#
#   scripts/wiki.sh <repo-dir> <wiki-dir> <ref> <owner/repo>
#
# A GitHub wiki is a flat folder of pages, so docs/reference/list-json.md becomes
# Reference-list-json.md, links between pages are rewritten to the new names, links to other files
# of the repository point at that file on GitHub at <ref>, and _Sidebar.md is built from the lists
# in docs/Home.md. Pages that no longer exist in docs/ are removed from <wiki-dir>. Everything else
# in <wiki-dir>, such as .git, is left alone.
set -eu

die() {
    echo "wiki.sh: $*" >&2
    exit 1
}

[ $# -eq 4 ] || die "usage: wiki.sh <repo-dir> <wiki-dir> <ref> <owner/repo>"
src=$1
wiki=$2
ref=$3
repo=$4

[ -f "$src/docs/Home.md" ] || die "$src/docs/Home.md not found: $ref has no docs/Home.md"
[ -d "$wiki" ] || die "$wiki is not a directory"

find "$wiki" -maxdepth 1 -type f -name '*.md' -exec rm -f {} +

here=$(dirname "$0")

find "$src/docs" -type f -name '*.md' | sort | while IFS= read -r file; do
    rel=docs/${file#"$src"/docs/}
    name=$(awk -v mode=name -v rel="$rel" -f "$here/wiki.awk")
    {
        awk -v rel="$rel" -v repo="$repo" -v ref="$ref" -f "$here/wiki.awk" "$file"
        printf '\n---\n\n_This page is generated from [%s](https://github.com/%s/blob/%s/%s) at %s. Edit it there: the next release overwrites changes made in the wiki._\n' \
            "$rel" "$repo" "$ref" "$rel" "$ref"
    } >"$wiki/$name.md"
done

awk -f "$here/wiki-sidebar.awk" "$wiki/Home.md" >"$wiki/_Sidebar.md"
echo "wiki.sh: wrote $(find "$wiki" -maxdepth 1 -type f -name '*.md' | wc -l | tr -d ' ') pages for $ref"
