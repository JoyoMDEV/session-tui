# Publish the documentation to the wiki

The wiki is generated from `docs/` and rebuilt on every release. This page is for maintainers: it
says what happens, how to set the wiki up once, and how to publish by hand.

## What happens on a release

`release.yml` calls `wiki.yml` with the release tag after the release is published. The workflow
checks out `docs/` at that tag and the wiki repository, runs `scripts/wiki.sh`, and pushes the
result with the repository's own `GITHUB_TOKEN`. It does not need a secret. Its job is the only one
with write permission, and it writes to the wiki repository only.

The script turns the folders of `docs/` into the flat page names a wiki needs:

| In `docs/` | In the wiki |
| --- | --- |
| `Home.md` | `Home` |
| `reference/list-json.md` | `Reference-list-json` |
| `explanation/architecture.md` | `Explanation-architecture` |
| `how-to/publish-the-wiki.md` | `How-to-publish-the-wiki` |
| `adr/0001-code-organisation.md` | `ADR-0001-code-organisation` |

Links between pages are rewritten to those names. A link to any other file of the repository, such
as the README, points at that file on GitHub at the release tag, so the wiki of a release links to
the files of that release. The sidebar is built from `Home.md`: each section heading links to that
section of the Home page, and sections that have wiki pages list them below it. Links that leave
the wiki, such as those to the README, are only on the Home page. Each page ends with a note saying which file and tag it comes from.

Because the wiki is overwritten, a page that is deleted from `docs/` disappears from the wiki at
the next release, and changes made in the wiki itself are lost. Edit `docs/` instead.

## Set it up once

The wiki feature is on by default, but its repository does not exist until a first page is
created in the browser. Open the repository's **Wiki** tab, choose **Create the first page**, and
save it. The workflow replaces that page at the first run. Until then it fails when it checks out the
wiki.

## Publish by hand

Run the workflow with the tag or branch to publish:

```sh
gh workflow run wiki.yml -f ref=v0.2.0
```

or use **Actions > Wiki > Run workflow** in the browser. Publishing a branch such as `main` is
useful to look at a change before a release; the pages and their footers then name that branch.
The next release replaces them.

## Check the result

Open the wiki's Home page and follow a few links: one to another page, one to the README, and one
from the sidebar. To see the pages without publishing, build them into a scratch folder:

```sh
mkdir /tmp/wiki-preview
sh scripts/wiki.sh . /tmp/wiki-preview v0.2.0 JoyoMDEV/session-tui
```

`tests/wiki.rs` runs the script on small examples and on the real `docs/`, and fails if a link in a
generated page leads nowhere.

## If it fails

- The checkout of the wiki fails: the first page does not exist yet, see "Set it up once".
- `docs/Home.md not found`: the tag or branch has no documentation index. Releases before the
  index existed cannot be published.
- The push is rejected: someone edited the wiki in the meantime. Run the workflow again; it
  rebuilds from `docs/` and the checkout is fresh.
