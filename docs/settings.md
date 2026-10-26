# Settings

The service reads one TOML file of settings when `bmo-job serve` starts. Nothing a deployment chooses stands in
the code: where it listens, the tokens it takes and the callers they name, whose approval opens a draft, and the
database it keeps its members in come from this file.

## Where the file is

- **The path in `BMO_JOB_SETTINGS`,** where that variable is set and not empty.
- **Otherwise `settings.toml` in the working directory.** That file is the deployment's own and git ignores it.
- `settings.example.toml` at the root is the checked-in example, and loads as it stands.

## What the file holds

Two sections required, `[server]` and `[database]`, and one optional, `[callers]`. A key the service does not
know is refused, so a misspelt setting fails at start rather than standing silently at a default.

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

A deployment marks as an approver the caller whose word alone puts work into a queue, and gives its token only to
what acts on that word. A deployment that marks none opens no draft by an approval.

### `[database]`

| Key | What it holds |
|---|---|
| `url` | The SQLite database the service keeps its members in, as an address starting `sqlite:`, for example `sqlite://job.db?mode=rwc`; any other address is refused. Its tables are made where it has none (docs/schema.md) |

## What the code fixes

The spans and bounds docs/api.md states are the service's contract, not a deployment's choice, so they stand in
the code: the page sizes, the longest wait, a lease's and a subscription's span, the event window, and webhook
delivery's timeout, retries and the failures that move a subscription to `failing`.
