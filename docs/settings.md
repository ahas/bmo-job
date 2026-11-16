# Settings

The service reads one TOML file of settings when `bmo-job serve` starts. Nothing a deployment chooses stands in
the code: where it listens, the tokens it takes and the callers they name, whose approval opens a draft, and the
labels whose items open by themselves and the database it keeps its members in come from this file.

## Where the file is

- **The path in `BMO_JOB_SETTINGS`,** where that variable is set and not empty.
- **Otherwise `settings.toml` in the working directory.** That file is the deployment's own and git ignores it.
- `settings.example.toml` at the root is the checked-in example, and loads as it stands.

## What the file holds

Two sections required, `[server]` and `[database]`, and two optional, `[callers]` and `[unattended]`. A key the
service does not know is refused, so a misspelt setting fails at start rather than standing silently at a
default.

### `[server]`

| Key | What it holds |
|---|---|
| `listen` | The address and port the service listens on |
| `token_file` | The file holding the shared bearer token, which a request may carry and which names no caller; its surrounding whitespace is dropped, and a file holding no token is refused |

### `[callers]`

One table for each caller with a token of its own, keyed by the caller's name: 1 to 64 letters, digits, dots,
underscores and hyphens. A request carrying a caller's token is that caller's, and an approval it gives is
recorded under that name (docs/api.md, "Approvals").

| Key | What it holds |
|---|---|
| `token_file` | The file holding the caller's bearer token, read as `[server] token_file` is; a token that is the shared one or another caller's is refused |
| `approves` | Whether the caller's approval opens a draft: `true` makes it an approver, whose approval is of the kind `approval`; left out it is `false`, and the caller's approval is a recommendation |
| `supervises` | Whether the caller supervises the work: `true` lets it settle a question put to the supervisor, ask a person, put such a question to a person instead and take one back, and bars it from settling a question put to a person (docs/api.md, 「The addressee」); left out it is `false`, and the caller answers for a person |

A deployment marks as an approver the caller whose word alone puts work into a queue, and gives its token only to
what acts on that word. A deployment that marks none opens no draft by an approval. Where a deployment marks the
caller a chat agent's own token names, the agent's approval opens the draft it has read and judged sound; the rule
for when to give it stands in the agent's own instructions, not here. A caller the deployment does not mark, and
the shared token, record only a recommendation, which opens no draft.

A deployment marks as supervising the caller whose token the supervisors of its work carry, and no other. Every
caller it leaves unmarked, and the shared token, answers for a person: the console a person answers from among
them. A deployment that marks none has no caller that settles a question put to the supervisor or asks a person, so
every question waits with the supervisor until it is withdrawn.

### `[unattended]`

The labels whose items need no approval, each a key at a value. An item raised carrying one opens at once, with a note that it opened by
itself under this setting (docs/api.md, "Unattended labels"). Left out, every item is raised a draft.

| Key | What it holds |
|---|---|
| `labels` | The labels, each `key=value`, matched where an item's labels hold that key at that value, as the `label` filter reads that shape; an entry with no `=`, an empty key or an empty value is refused |

A deployment names here the work a worker takes up and settles on its own, such as the review of a pull request a
worker holds, which is filed with the label `lane` at `pr-review` (docs/base-libraries.md, "The road").

### `[database]`

| Key | What it holds |
|---|---|
| `url` | The SQLite database the service keeps its members in, as an address starting `sqlite:`, for example `sqlite://job.db?mode=rwc`; any other address is refused. Its tables are made where it has none (docs/schema.md) |

## What the code fixes

The spans and bounds docs/api.md states are the service's contract, not a deployment's choice, so they stand in
the code: the page sizes, the longest wait, a lease's and a subscription's span, the event window, and webhook
delivery's wait for an answer, its retries, the failures that move a subscription to `failing` and the pace
a failing one is tried at.
