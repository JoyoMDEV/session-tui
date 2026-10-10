# `sessions list --json`

`sessions list --json` prints a JSON array with one object per session that matches the filters,
in the order of `sessions.json`. No match is an empty array (`[]`). This is a view made for
scripts. It is not the layout of `sessions.json`, which can change without notice.

```sh
sessions list --json --ticket ABC-123 | jq -r '.[] | "\(.updated_at)  \(.title)"'
```

## Keys

Every key is always present. A missing value is `null` (or `[]` for lists), so a script never has
to test whether a key exists.

| Key | Type | Meaning |
| --- | --- | --- |
| `id` | string | The Claude Code session ID, as used by `claude --resume` |
| `title` | string or null | The best title there is: your own, else the one Claude Code generated, else the start of the first prompt |
| `title_source` | `"own"`, `"claude"`, `"prompt"` or null | Where `title` comes from. `null` exactly when `title` is `null` |
| `cwd` | string | The directory the session was started in, which `claude --resume` needs |
| `branch` | string or null | The last git branch Claude Code recorded for the session |
| `pr_url` | string or null | The last pull request URL Claude Code linked to the session |
| `tickets` | array of strings | Issue tracker keys such as `ABC-123`, in the order they were added |
| `tags` | array of strings | Topic tags, lower case |
| `note` | string or null | Your note |
| `agent` | string | The coding agent that wrote the session, `claude-code` today |
| `archived` | boolean | `true` for a session moved out of the default list with `sessions archive`. `list --json` includes archived sessions only with `--all` |
| `created_at` | string | When `sessions` first saw the session, RFC 3339 in UTC |
| `updated_at` | string | The last activity, RFC 3339 in UTC |

## Example

```json
[
  {
    "id": "4c05a435-3968-42d3-b58b-f9cc4bdab590",
    "title": "Fix login redirect",
    "title_source": "own",
    "cwd": "/work/app",
    "branch": "feat/login",
    "pr_url": null,
    "tickets": [
      "ABC-123"
    ],
    "tags": [
      "auth"
    ],
    "note": null,
    "agent": "claude-code",
    "archived": false,
    "created_at": "2026-10-06T08:12:03.417365Z",
    "updated_at": "2026-10-06T09:40:11.002931Z"
  }
]
```

## Compatibility

Existing keys keep their name, type and meaning. New keys may be added, so a script should ignore
keys it does not know. A change that breaks this is listed under a breaking change in the release
notes. These are intentions of this project, not something Claude Code guarantees: the values come
from Claude Code's transcripts, whose format is internal and may change, so `branch`, `pr_url` and
the generated title can become `null` for sessions where they used to be set.
