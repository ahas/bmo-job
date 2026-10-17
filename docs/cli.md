# The command line

The service's command line is its one binary, `bmo-job`. The platform's thin entry point, `bmo`, finds it
the way git finds `git-<command>`: by its name on the search path, `bmo-<unit>`. It hands the binary
the words after `bmo` for every command this unit carries, so `bmo todo --add` is `bmo-job todo --add`
and `bmo job land` is `bmo-job job land`.

## The nouns

The first word is a noun, and `bmo-job nouns` prints every one this binary carries, one a line, which
is what `bmo` reads to know which commands are this unit's. A noun of two words is two words.

| Noun | What it is | Stands on |
|---|---|---|
| `serve` | The server (docs/api.md, "Routes"), with the work-order runner beside it | the settings |
| `job-run` | The program a job's unit runs (docs/runs.md) | the settings |
| `job` | A job's own verbs: wait, live-claude, ask, land, close, borrow, guard, tool-calls, figures, branches, cowork-timing, start (docs/verbs.md) | the stand |
| `node-land` | The operations machine's side of a landing from a work node (docs/verbs.md, "Landing a branch") | the stand |
| `todo` | The TODO's verbs (docs/verbs.md, "The TODO's edits and holds"), each flag one verb | the stand |
| `work-order` | One item's work order read, and every change to one asked of the server (docs/verbs.md, "A work order read and changed") | the stand and the server |
| `worker` | The workers listed, read, added, changed, removed and placed on the nodes | the server |
| `overtime` | The agent's answer to a run that reached its clock (docs/verbs.md, "A run past its clock, answered") | the database |
| `checkout setup` | The checkout it is run in set up (docs/verbs.md, "A checkout set up") | nothing |
| `room jobs` | A room's jobs, newest first (docs/reading.md, "A room's jobs") | the database |

- **The stand** is docs/verbs.md's, "Where a verb stands": the settings, the database and the queue's
  ground, and the owner from bmo-receiver.
- **The server** is reached at `[server] address`. A verb that asks it and is refused, or gets no
  answer, says so and ends with 1.
- **The database** is the one `[database] url` names, opened by the verb alone.

## `worker`

The workers as the server keeps them (docs/api.md, "Workers"), each verb one request:

- `worker list` reads `GET /workers`, and `worker show <name>` reads `GET /workers/{name}`.
- `worker add <name> [--label <label>] [--path <path>] [--preamble <text>] [--dev-command <command>]`,
  `worker update <name>` with any of those and `--keepalive-stopped <true|false>`,
  `--pinned-provider <pin>`, `--support-count <n>` and `--support-keepalive <n>`, and
  `worker remove <name>` are `POST /worker`. An empty preamble or dev command clears it.
- `worker node add-checkout <name> --node <node> [--path <path>]`,
  `worker node remove-checkout <name> --node <node>` and `worker node place <name> [--node <node>]
  [--path <path>]` are `POST /worker/node`.

## `todo`

The TODO's verbs from a shell stand on the rows' ground (docs/rows.md, "Where a verb stands") with:

- **who runs it:** the job's own process as docs/verbs.md tells one; the agent whose turn runs it from
  `BMO_TURN_AGENT`; the person at a shell from `USER`, where neither a job nor an agent's turn runs it;
- **its hands:** the server told the queue moved at `POST /queue/moved`, asked to tell the queue's
  standing at `POST /queue/tell` and to stop an item at `POST /todo`, and the owner's chat told of
  words added;
- **the providers** bmo-agency lists, which a job's model may name.

A step of a work order between its plan and its final turn closes no item.

## What it prints

- **A verb's lines** go to standard output, one a line; a coded answer is its JSON on one line, and a
  reading is its JSON.
- **A refusal** is its sentence on standard error. The verb ends with its own code where it gives one,
  2 where its words did not read, and 1 otherwise.
