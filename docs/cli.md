# The command line

The binary `bmo-job` carries these commands.

| Command | Does |
|---|---|
| `bmo-job nouns` | Every noun this unit carries (`job`, `job ask`, `todo` and `work-order`), one a line, `job ask` marked `home` after a tab, which `bmo` reads to find the commands it hands this unit |
| `bmo-job serve` | Serves the API by the settings file (docs/settings.md) until it is told to stop by an interrupt or a termination signal; every open event stream ends then, so the stop is not held |
| `bmo-job mcp --service <url>` | Serves the harness plugin's MCP server on standard input and output, reaching the service at the address (docs/api.md, "The harness plugin") |
| `bmo-job job close` | Hands the job's commit over to its repository's deployment, with the range narrowed to the commit the deployment recorded as live (「`bmo-job job close`」) |
| `bmo-job job ask` | Puts a choice to the owner in the middle of a job and waits for a person to settle it (「`bmo-job job ask`」) |
| `bmo-job todo` | Closes an item, holds it with the run's account on it, or raises a new one, each through the service's own routes (「`bmo-job todo`」) |
| `bmo-job work-order` | Adds a step to an item's work order (「`bmo-job work-order`」) |
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

## `bmo-job job ask`

`bmo job ask` runs this. It is what a job's own run waits on: nothing ends the wait but what is listed here, and
no clock is among it.

| Form | What it does |
|---|---|
| `--todo <item> --question <words> --option <words> --option <words> --recommend <n> --why <words>` | Opens the question in the owner's private room, puts it to him and waits |
| `--talk <question> --say <words>` | Puts the job's answer to his talk in the same room, then waits on the same question |
| `--talk <question> --say <words> --option <words> --option <words> --recommend <n> [--question <words>] [--why <words>]` | The same, with the question put again in other words |

- **How it ends:**

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | A person settled it | One line: the option's words, or his own | | 0 |
  | He talked instead of choosing; the question stays open | His words and the call that answers him | | 3 |
  | The question could not be put; it is closed `unsent` | Nothing | Why | 1 |
  | The question was withdrawn, or the run was told to stop, which withdraws it | Nothing | Why | 1 |
  | The words were refused; no question opened | Nothing | Why, and the usage | 1 |

- **A recommendation is shown and never answers.** It is printed only where a person settled the question with
  it. A settling reply no person gave is refused: the verb ends 1 and prints no option.
- **Putting the question.** The verb reads the owner's private room from the receiver's `GET /owner`, opens the
  question there with `toldByAsker` (docs/api.md, "Questions and replies"), and hands it to the room's supervisor
  through the receiver's `POST /instructions`, an order under the sort `job` of the kind
  `question-awaits-answer`. Anything but a queued instruction is a failed send. A question that is settled or
  stopped has its instruction withdrawn where it has not reached the supervisor yet.
- **What ends no wait.** A service or a receiver that does not answer while the verb waits is asked again; a
  restart is not an answer. Only a question the service no longer holds ends it, with 1.
- **The due time.** The question is asked with a due time of one hour, which paces the one notice
  `question.due` tells and settles nothing.
- **Pages hosted elsewhere are refused:** an address of a claude.ai artifact, a published Claude page, a gist or a
  paste in the question, an option, the reason or the words of a talk.
- **Where it reaches the receiver:** `BMO_RECEIVER_URL` with `BMO_RECEIVER_TOKEN` or `BMO_RECEIVER_TOKEN_FILE` in
  the run's environment, else `RECEIVER_ADDRESS` and `RECEIVER_TOKEN` in `env/receiver.env` under the data root.

## `bmo-job todo`

The worker's run already names the job service in `BMO_JOB_URL` and its token in `BMO_JOB_TOKEN` or `BMO_JOB_TOKEN_FILE`; a shell with neither reads the deployment's `settings.toml` beside the binary, or the file `BMO_JOB_SETTINGS` names.

| Form | What it does |
|---|---|
| `--done <item> [--report <text>] [--unfinished <text>]` | Closes the item: `--report` kept as a `final` report and `--unfinished` as a `held` one through `POST /items/{itemId}/reports`, then `PATCH /items/{itemId}` sets its state to `done`. A move the service refuses, a held item's among them, leaves the item as it stood |
| `--stop-here <item> --report <text> [--why <reason> [--after <item> \| --after-deploy <item or commit>]]` | Leaves the run's account on the item as a `held` report, and holds it. An item already held takes only the account; one not yet held needs `--why`, kept with its condition in the labels `hold-why`, `hold-after` and `hold-after-deploy`. A condition releases nothing, and no form takes a hold off |
| `--add --title <words> --judgement <words> --raised-by <item> [--from-todo <id>:<strand>[,<id>:<strand>]]` | Raises a draft in the queue and scope of the item it was raised from, the judgement as its brief and `--from-todo` as its label `from-todo`, and prints it |

- **`--by <name>`** names who writes it: the run's `BMO_TURN_AGENT` where left out, then `USER`.
- **Pages hosted elsewhere are refused** in a `--report` and an `--unfinished`, as in a question.
- **Each answer is printed as JSON,** one a line. The verb reads no database of its own.

## `bmo-job work-order`

`bmo work-order --todo <item> --add <title> [--instruction <words>]` appends a step to the item's work order
through `PATCH /items/{itemId}/work-order` and prints the work order. A step is one line of the plan: the title,
then the instruction after a colon where one is given. An item with no work order is refused.

## `bmo-job mcp`

| Option | What it gives |
|---|---|
| `--service <url>` | The base address the service's routes stand at; required |
| `--token <token>` | The bearer token every call is made with: the shared token, or a caller's own, under whose name the service records an approval the server gives (docs/settings.md, `[callers]`); read from `BMO_JOB_TOKEN` where it is left out |
| `--holder <name>` and `--queue <name>` | A holder name and the queues it is told of from the start, `--queue` given once for each; each needs the other |
| `--channel <capability>` | The experimental capability each event is also pushed on as a message |
| `--without <tools>` | Tools the server leaves out, their names comma-separated, the option given once or more; read from `BMO_JOB_MCP_WITHOUT` where it is left out. A tool left out is neither listed nor called; a name that is no tool of the plugin refuses the start |

The server ends when the harness closes its input, and then gives back the leases and deletes the
subscriptions it kept, waiting for the service to answer each.
