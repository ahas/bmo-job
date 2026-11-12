# The command line

The binary `bmo-job` carries these commands.

| Command | Does |
|---|---|
| `bmo-job nouns` | Every noun this unit carries (`job`, `job ask` and `todo`), one a line, `job ask` marked `home` after a tab, which `bmo` reads to find the commands it hands this unit |
| `bmo-job serve` | Serves the API by the settings file (docs/settings.md) until it is told to stop by an interrupt or a termination signal; every open event stream ends then, so the stop is not held |
| `bmo-job mcp --service <url>` | Serves the harness plugin's MCP server on standard input and output, reaching the service at the address (docs/api.md, "The harness plugin") |
| `bmo-job job close` | Hands the job's commit over to its repository's deployment, with the range narrowed to the commit the deployment recorded as live (「`bmo-job job close`」) |
| `bmo-job job ask` | Puts a choice to the owner in the middle of a job and waits for a person to settle it (「`bmo-job job ask`」) |
| `bmo-job todo` | Closes an item, holds it with the run's account on it, or raises a new one, each through the service's own routes (「`bmo-job todo`」) |
| `bmo-job openapi` | Prints the OpenAPI document, the one committed as `docs/openapi.json` |

## `bmo-job job close`

`bmo job close --then '<why this goes live, in English>' [--todo <item>] [--skip-suite '<why, in English>']` runs this, through
the unit command the deployment links into `~/.local/bin` (docs/deploy.md, "The unit command").

| Option | What it does |
|---|---|
| `--then <why>` | Why this goes live, in English; required. A reason holding Hangul is refused |
| `--skip-suite <why>` | Why the change reaches nothing the suite runs, in English; the static check still runs |
| `--todo <item>` | The item the job works on; the close writes where it stands onto that item (below) |

The close reads the live commit from the deployment record (docs/deploy.md, "A job closes"), narrows the job's
own commits to those past it, refuses a commit whose message holds Hangul or one that is not on the remote,
runs the check and the suite, says `waiting to go live: <commit> stands pushed for its deployment`, and then
waits for that commit's deployment, with no clock, as `bmo job ask` waits for a person: the job that closed is
the one that hears how its deployment ended. A commit the record already names as live runs nothing and ends
clean. Where the record's commit is not in the
repository or is no ancestor of `HEAD`, the range is taken from the job's own base commit instead, and the
close says in one line which range it used. It reads no archive and no pending list; the deployment that
records what goes live writes the commit down.

- **Where the close stands is written on the item `--todo` names,** as two of its labels, the rest of its labels
  kept: `close-phase` is `testing` from the moment the check starts and `waiting-live` once the check and the
  suite have passed, and `close-commit` is the commit handed over, written with `waiting-live`. A reader of
  the queue tells from them an item whose work is being tested from one whose commit waits for its deployment.
- **Both labels are taken off** where the check or the suite does not pass, where the close is told to stop
  while they run, and where the commit is already what is deployed, so an item carries them only while its
  close is under way or its commit waits.
- **The item is reached as `bmo-job todo` reaches it** (「`bmo-job todo`」), and a restart is no answer. While
  the service does not answer, nothing at its address, a connection cut or a 5xx, its labels are written again,
  after a quarter of a second at first and every five seconds at most, and the close goes on only once they
  are written or the run is told to stop. An item the service refuses, one it does not hold or a change it
  will not take, stops no close: the close says so in one line and goes on.
- **A close with no `--todo` writes on no item.**
- **A repository that deploys nothing ends its close on main.** Where the checkout's `testbed.toml` reads and
  declares no `[deploy]` (a base library, which is released and never put live), there is no deployment to wait
  for: the close says `live: <commit> stands on main, and this repository deploys nothing` and ends clean.
- **What ends the wait.** The close reads the deployment's progress record and its record of what went live
  under the checkout's deploy root (bmo-testbed's `docs/deploy.md`, 「What a deployment says of itself as it
  runs」), again every few seconds. A record with no ending yet is a deployment still running only while the
  process it names is alive and the user unit it names is not read inactive or failed; one whose process or
  unit is gone was cut off, and the record read once more still unended says so:

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | A deployment begun after the handover, carrying the commit, ended `live`; or, none under way, the record of what went live carries it | `live: <commit> is live` | | 0 |
  | Such a deployment ended `rolled-back`, `failed` or `stopped`, or was cut off, its record unended with its process or unit gone | | `not live:` with the outcome, `cut off` for one cut off, and the deployment's reason, the journal of its unit, and that the commit is to be mended and closed again | 1 |
  | The run was told to stop | | one line saying so | 130 |

  Either ending takes the item's labels off. A commit that did not go live is the same job's to mend: it finds
  the cause, commits, pushes and closes again, and its item is never closed while its commit is not live.

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
  | The question could not be put, the receiver refusing it; it is closed `unsent` | Nothing | Why | 1 |
  | The question was withdrawn, or the run was told to stop, which withdraws a question already opened | Nothing | Why | 1 |
  | The words were refused; no question opened | Nothing | Why, and the usage | 1 |

- **A recommendation is shown and never answers.** It is printed only where a person settled the question with
  it. A settling reply no person gave is refused: the verb ends 1 and prints no option.
- **Putting the question.** The verb reads the owner's private room from the receiver's `GET /owner`, opens the
  question there with `toldByAsker` (docs/api.md, "Questions and replies"), and hands it to the room's supervisor
  through the receiver's `POST /instructions`, an order under the sort `job` of the kind
  `question-awaits-answer`. An answer other than a queued instruction is a failed send. A question that is settled or
  stopped has its instruction withdrawn where it has not reached the supervisor yet.
- **What a turn carries.** An order carries the notice's kind and number, the item's number and title, and the data
  it is about: the question and its options numbered with the recommended one marked, or the job's words in a talk,
  with the question's words and options again where the talk changed it. It carries no instruction, since how a
  notice is read stands in the receiving session's own instructions.
- **A restart is no answer.** Every call the verb makes, reading the owner's room, opening the question,
  reading the item's title, handing the order or the talk over, keeping the job's words and where it heard
  through, closing the question, and every read while it waits, is made again while the service or the
  receiver does not answer: nothing at its address, a connection cut, or a 5xx. It is made again after a
  quarter of a second at first and every five seconds at most. The run told to stop ends it as above. Only an
  answer ends a call: while the question is put, a refusal closes it `unsent`; while it is waited on, only a
  question the service no longer holds ends the wait, with 1.
- **At least once.** A call cut after its peer took it is made again, so an order or the job's words in a talk
  can reach the room twice. A question is opened once: where opening it was cut, the room's open questions
  are read first, and one of the same asker, item, words and options is taken as the one opened.
- **The due time.** The question is asked with the service's own due time, a minute where the service names no
  other, which paces the one notice
  `question.due` tells and settles nothing.
- **Pages hosted elsewhere are refused:** an address of a claude.ai artifact, a published Claude page, a gist or a
  paste in the question, an option, the reason or the words of a talk.
- **Where it reaches the receiver:** `BMO_RECEIVER_URL` with `BMO_RECEIVER_TOKEN` or `BMO_RECEIVER_TOKEN_FILE` in
  the run's environment, else `RECEIVER_ADDRESS` and `RECEIVER_TOKEN` in `env/receiver.env` under the data root.

## `bmo-job todo`

The worker's run already names the job service in `BMO_JOB_URL` and its token in `BMO_JOB_TOKEN` or `BMO_JOB_TOKEN_FILE`; a shell with neither reads the deployment's `settings.toml` beside the binary, or the file `BMO_JOB_SETTINGS` names.

| Form | What it does |
|---|---|
| `--done <item> [--report <text>] [--unfinished <text>]` | Leaves the run's account of the finished work on the item: `--report` kept as a `final` report and `--unfinished` as a `held` one through `POST /items/{itemId}/reports`. It moves no state: the item is done once whoever ordered the work takes it — its review, or, for an item whose commit waits for its deployment (its label `close-phase` reading `waiting-live`, 「`bmo-job job close`」), whoever puts that commit live. The verb prints one line saying what the item waits on: its commit going live, its review, or its orderer |
| `--stop-here <item> --report <text>` | Leaves the run's account on the item as a `held` report, for the run that picks the work up: what is finished and where the next run goes on. It moves no state; holding the item is its orderer's |
| `--add --title <words> --judgement <words> --raised-by <item> [--repository <name>] [--from-todo <id>:<strand>[,<id>:<strand>]]` | Raises a draft in the queue and scope of the item it was raised from, the judgement as its brief, `--repository` as its assignee (the repository its work is in), else the assignee of the item it was raised from, and `--from-todo` as its label `from-todo`, and prints it |

- **`--by <name>`** names who writes it: the run's `BMO_TURN_AGENT` where left out, then `USER`.
- **Pages hosted elsewhere are refused** in a `--report` and an `--unfinished`, as in a question.
- **Each answer is printed as JSON,** one a line. The verb reads no database of its own.
- **A run never moves its own item's state.** Whether the item is done, held or open again is decided by whoever
  ordered the work, through the service's routes; the verb leaves the run's words on the item and nothing else.

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
