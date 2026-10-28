# The command line

The binary `bmo-job` carries these commands.

| Command | Does |
|---|---|
| `bmo-job nouns` | Every noun this unit carries (`job` and `todo`), one a line, which `bmo` reads to find the commands it hands this unit |
| `bmo-job serve` | Serves the API by the settings file (docs/settings.md) until it is told to stop by an interrupt or a termination signal; every open event stream ends then, so the stop is not held |
| `bmo-job mcp --service <url>` | Serves the harness plugin's MCP server on standard input and output, reaching the service at the address (docs/api.md, "The harness plugin") |
| `bmo-job job close` | Hands the job's commit over to its repository's deployment, with the range narrowed to the commit the deployment recorded as live (「`bmo-job job close`」) |
| `bmo-job todo --done <item>` | Closes an item through the service's own routes: its report and what it could not reach are kept on the item, and its state is set to `done` (「`bmo-job todo`」) |
| `bmo-job openapi` | Prints the OpenAPI document, the one committed as `docs/openapi.json` |

## `bmo-job job close`

`bmo job close --then '<why this goes live, in English>' [--skip-suite '<why, in English>']` runs this, through
the unit command the deployment links into `~/.local/bin` (docs/deploy.md, "The unit command").

| Option | What it does |
|---|---|
| `--then <why>` | Why this goes live, in English; required. A reason holding Hangul is refused |
| `--skip-suite <why>` | Why the change reaches nothing the suite runs, in English; the static check still runs |

The close reads the live commit from the deployment record (docs/deploy.md, "A job closes"), narrows the job's
own commits to those past it, refuses a commit whose message holds Hangul or one that is not on the remote,
runs the check and the suite, and ends `waiting to go live: <commit> stands pushed for its deployment`. A
commit the record already names as live runs nothing and ends clean. It reads no archive and no pending list;
the deployment that records what goes live writes the commit down.

## `bmo-job todo`

`bmo todo --done <item>` runs this. The worker's run already names the job service in `BMO_JOB_URL` and its token in `BMO_JOB_TOKEN` or `BMO_JOB_TOKEN_FILE`; a shell with neither reads the deployment's `settings.toml` beside the binary, or the file `BMO_JOB_SETTINGS` names.

| Option | What it does |
|---|---|
| `--done <item>` | The item to close |
| `--report <text>` | How the work was done, kept as the item's `final` report through `POST /items/{itemId}/reports` |
| `--unfinished <text>` | What the job's hands could not reach, kept as a `held` report through the same route |
| `--by <name>` | Who writes the closing: the run's `BMO_TURN_AGENT` where left out, then `USER`, which the reports are kept under |

The reports are written first, then `PATCH /items/{itemId}` sets the item's state to `done`; the answer is each report and the item as JSON, one a line. The verb reads no database of its own, and no table of an older era.

## `bmo-job mcp`

| Option | What it gives |
|---|---|
| `--service <url>` | The base address the service's routes stand at; required |
| `--token <token>` | The bearer token every call is made with: the shared token, or a caller's own, under whose name the service records an approval the server gives (docs/settings.md, `[callers]`); read from `BMO_JOB_TOKEN` where it is left out |
| `--holder <name>` and `--queue <name>` | A holder name and the queues it is told of from the start, `--queue` given once for each; each needs the other |
| `--channel <capability>` | The experimental capability each event is also pushed on as a message |
| `--without <tools>` | Tools the server leaves out, their names comma-separated, the option given once or more; read from `BMO_JOB_MCP_WITHOUT` where it is left out. A tool left out is neither listed nor called; a name that is no tool of the plugin refuses the start |

The server ends when the harness closes its input, and then gives back the leases and deletes the
subscriptions it kept, within ten seconds.
