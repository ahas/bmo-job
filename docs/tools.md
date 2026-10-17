# The tools

A tool bound to this service is the service's own, kept under `tools/` in this repository, one folder a
tool: its manifest `tool.toml`, the call schemas its manifest names, and its crate, a member of the
workspace with its tests in its own `tests/`. The tool host reads the manifests as it reads any other and
runs a tool's executable once a call; nothing here knows the host beyond what a call hands it.

## The tools and their entries

| Tool | Entry | What it does | Through |
|---|---|---|---|
| `todo` | `worker/todo_list` | The queue in its own order as a worker reads it | `GET /todos/worker-lines` |
| `todo` | `worker/todo_show` | One item whole as a worker reads it | `GET /todos/{item}/worker-lines` |
| `todo` | `bmo/todo_list` | The queue in its own order as a person reads it | `GET /todos` |
| `todo` | `bmo/todo_show` | One item whole as a person reads it | `GET /todos/{item}` |
| `todo` | `bmo/todo_add` | A row filed, out of the conversation or out of the agent's own judgement; filing approves nothing | `bmo-job todo --add` |
| `todo` | `bmo/todo_append` | Words added to an open item | `bmo-job todo --append` |
| `todo` | `bmo/todo_approve` | An item approved into the queue, or its approval taken back | `bmo-job todo --approve`, `--unapprove` |
| `launch` | `bmo/task` | A job launched on a worker, which reports to the room when done | `POST /task` |
| `state` | `bmo/jobs` | A room's jobs, newest first | `GET /rooms/{chat}/jobs` |
| `state` | `bmo/work_order` | One item's work order: its state, its stages and each task | `GET /work-orders/{item}` |

- **Through the API** (docs/api.md, "Routes"), at the `[server] address` of the settings file the
  service's own verbs read (docs/settings.md, "Where the file is").
- **Through the unit CLI** (docs/cli.md), the program `bmo-job` on the search path, started with the
  turn's variables named below so the verb files under the turn's agent and run.
- **A worker's reading** is its lines; **a person's reading** and every other answer is the JSON the
  route or the verb answered, as the lines of the reading schema.

## Where a tool is placed

- **A deployment places each tool** in its folder's `.placed/` inside the release, the folder the tool host
  reads: the executable the release's `bin/` holds, beside the manifest and the schema files it names. The
  steps that do it stand in `testbed.toml` (docs/deploy.md).

## A call

- **What a tool is handed** is one JSON object on standard input: `protocol` (1), `kind` (`call`), `entry`
  (`<namespace>/<name>`), `arguments` and `session`. A request of any other kind is answered as a failed
  call, and a request that does not read ends the executable unclean, which the host takes as a failed call.
- **The session's turn** is read from its `runVariables`: `BMO_TURN_CHAT` (the room the turn answers),
  `BMO_TURN_AGENT` (its agent), `BMO_RUN_NAME` (its run) and `BMO_WORK_ORDER_STEP`. A call naming no room,
  in a turn that answers none, is refused.
- **What a tool answers** is one JSON object on standard output: `content` (each `type` `text` with its
  `text`), `isError`, and `structuredContent` where the entry declares an output schema. A refusal of the
  service, or a call it could not make, is an answer marked as an error with the reason; arguments that do
  not fit the entry's input are an error answer saying so.

## The manifest

- **`[tool]`:** `name` (the folder's name), `protocol` (1), `language` (`rust`), `build` and `check` (the
  commands the host builds and checks the tool with, in this repository's checkout), `run` (the
  executable and its words) and `timeout` (seconds a call may take).
- **`[[entry]]`:** `namespace`, `name`, `title?`, `description`, `input` (the call schema's file),
  `output?` (the reading schema's file), `hints` (`read_only`, `destructive`, `idempotent`,
  `open_world`), `capabilities` (what a session must hold to be shown the entry), `grant?` (what the
  person whose message a call acts on must hold), and `bundle.<capability>` (`shown`, `required?`): the
  arguments a session holding that capability is shown and must give.
- **Every entry a manifest names is answered** by its tool's executable, every schema it names reads as a
  JSON object whose required fields it names, and an entry acting on a grant takes the `message` it acts
  on: each tool's own test holds its manifest to its entries.
- **No text of a manifest or a schema is in a language other than English,** and none names a person, a
  machine or an agent: the person the queue awaits is the owner, and the agent is the turn's.
