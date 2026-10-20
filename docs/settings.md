# Settings

The service reads one TOML file of settings when `bmo-job serve` starts. Nothing a deployment chooses stands in
the code: where it listens, the token it takes and the database it keeps its members in come from this file.

## Where the file is

- **The path in `BMO_JOB_SETTINGS`,** where that variable is set and not empty.
- **Otherwise `settings.toml` in the working directory.** That file is the deployment's own and git ignores it.
- `settings.example.toml` at the root is the checked-in example, and loads as it stands.

## What the file holds

Two sections, both required. A key the service does not know is refused, so a misspelt setting fails at start
rather than standing silently at a default.

### `[server]`

| Key | What it holds |
|---|---|
| `listen` | The address and port the service listens on |
| `token_file` | The file holding the bearer token every request carries; its surrounding whitespace is dropped, and a file holding no token is refused |

### `[database]`

| Key | What it holds |
|---|---|
| `url` | The SQLite database the service keeps its members in, as an address starting `sqlite:`, for example `sqlite://job.db?mode=rwc`; any other address is refused. Its tables are made where it has none (docs/schema.md) |

## What the code fixes

The spans and bounds docs/api.md states are the service's contract, not a deployment's choice, so they stand in
the code: the page sizes, the longest wait, a lease's and a subscription's span, the event window, and webhook
delivery's timeout, retries and the failures that move a subscription to `failing`.
