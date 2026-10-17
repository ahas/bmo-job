# Settings

The service reads one TOML file of settings when it starts. Nothing a deployment chooses stands in the
code: the database it keeps its rows in and the names it says and records come from this file.

## Where the file is

- **The path in `BMO_JOB_SETTINGS`,** where that variable is set and not empty.
- **Otherwise `settings.toml` in the working directory.** That file is the deployment's own and git
  ignores it.
- `settings.example.toml` at the root is the checked-in example. It names a database and neutral
  placeholder names, and loads as it stands.

## What the file holds

The file holds six sections: `[database]` and `[deployment]`, both required; `[server]`, `[units]`
and `[host]`, which the server requires and the libraries read without; and `[words]`, which may be left
out. A key the service does not know is refused, so a misspelt setting fails at start rather than standing
silently at a default.

### `[database]`

- **`url`:** the database the service keeps its rows in, as a connection address.
  - An address starting `sqlite:` is a SQLite database, for example `sqlite://job.db?mode=rwc`.
  - An address starting `postgres:` or `postgresql:` is a PostgreSQL database. Options the driver reads
    in the address, such as a schema's search path, are passed on.
  - Any other address is refused when the file is loaded.
- Both kinds take the same statements. A statement binds its values by number, `$1` onwards, and every
  number is bound at its widest width, so a statement prepared once takes later values of any width.
- On PostgreSQL, a statement met as prepared before the tables' shape moved under it runs once more on
  a fresh connection. The pool never hands out a connection opened before that moment.

### `[deployment]`

The names the deployment goes by, which the service says in what it writes and matches in what it
reads, and the owner's chat:

| Key | What it names |
|---|---|
| `owner_name` | The deployment's owner, where nothing else gives the owner a name |
| `agent_name` | The deployment's agent, as a room calls it |
| `agent_key` | The deployment's agent, as the records name it |
| `sender_name` | The sender the agent's own messages are stored under |
| `receiver_unit` | The service unit the chat receiver runs as |
| `owner_chat` | The owner's private chat with the deployment's agent, by its number: where the queue's telling, the work-order runner and the verbs tell the agent their facts, and where a job's question is put |
| `wording` | The owner's wording document, by its absolute path, which the terms the owner has set aside are read out of |

Every key but `wording` is required and none may be empty; `owner_chat` is a whole number other than 0.
Where `wording` is left out, no term is set aside.

#### The owner's wording document

A markdown document the owner keeps, read once as the service starts, by the server and by a job's
runner alike: what a filing's title and parts are held to, and what a job's report is named under for
(docs/rows.md, docs/runs.md). The service reads three of its sections, each a level-two heading over a
list, and nothing else in it:

- **`Banned terms`:** each item `<lang> · <term> → <instead>`, optionally followed by ` — ` and when it
  applies. `<lang>` is `ko`, `ja` or `en`, the language of the lines the term is found on.
- **`Uses in the word's own sense`:** each item `<lang> · <term> · <phrase> — <why>`, a phrase in which a
  banned term listed above keeps its own sense and passes. The phrase must hold the term.
- An item of any other shape in those sections, and every other section, is read past.

A document that cannot be read, an item naming another language, or a phrase that names no banned term
listed above it or does not hold it, sets no term aside: the service says why in its log and goes on,
since nothing is held to a document nobody can read.

### `[server]`

Where the server stands (docs/api.md, "Routes"):

| Key | What it names |
|---|---|
| `listen` | The socket address the server binds, such as `127.0.0.1:7841` |
| `address` | The `http:` address other units reach the server at, which it gives bmo-receiver as a holder's |
| `unit` | The service unit the server runs as, which it declares itself a holder under |

### `[units]`

The `http:` address of each unit the server calls on (docs/api.md, "Calls on other units"):

| Key | The unit |
|---|---|
| `agency` | bmo-agency, whose provider order a reading of an item's model is read against |
| `gate` | bmo-gate, whose kept English a worker's reading carries |
| `receiver` | bmo-receiver, which the server declares itself a holder to and hands an item's talk |
| `web` | bmo-web, which says whose work an item's job is and whether that user may have it run |
| `graph` | bmo-graph, which says what a checkout's code graph names as a job's ending is judged |
| `brain` | bmo-brain, which an item's closing files the strands left to the agent in, and which a job's write into the brain holds off |
| `brain_token` | The file holding bmo-brain's token, by its absolute path, read once as the queue's ground is built |
| `siren` | the cache siren, which a co-work branch's answer on a stand-in road is told to; it may be left out, and then nothing is told |

Every key of both sections is required where the section stands, but for `siren`. An address that is
not `http:`, and a token file's path that is not absolute, is refused when the file is loaded; a token
file that cannot be read builds no ground.

### `[host]`

What the queue stands on of the machine it runs on (docs/queue.md): the programs it starts, the
directories it writes in, and the checkout the built-in worker works in. Every path is absolute.

| Key | What it names |
|---|---|
| `source` | The checkout the built-in worker works in where its row names none |
| `testbed` | bmo-testbed's program, which the queue asks for the commits a job left waiting to go live |
| `systemctl` | The program the user's systemd manager is asked through whether a unit stands, and told to stop one |
| `systemd_run` | The program a job's unit is started through |
| `runner` | The program whose `job-run` verb a job's unit runs |
| `runner_directory` | The directory a job's unit runs in |
| `runner_path` | The search path a job's unit is started with, as `PATH` |
| `runner_environment` | The file of variables a job's unit reads, where one is given; it may be left out |
| `locks` | The directory the checkouts' job locks stand in |
| `prompts` | The directory a job's text is written to for its unit, readable by the service's user alone |
| `lock_wait_seconds` | How long a launch waits for the last checkout's lock before it refuses, a whole number above 0 |
| `launcher` | The program a runner is launched through, with the runner's name as its verb |
| `launcher_directory` | The directory a runner's launcher runs in |
| `documents` | The directory a work order's prompts, result documents and check logs are written under, one folder to an item, with the records a job's run keeps of a checkout — its baseline and what it landed — under `worktree/` |
| `tree` | The directory whose `bin/` holds the platform's command line, which a builder turn of a work order adds steps through |
| `testbed_logs` | The directory bmo-testbed writes the logs of the runs it carries in, which a job's own commands may read and write unasked; it may be left out |
| `brain` | The directory the brain's documents stand in, a write into which by a job holds the brain's editors off (docs/verbs.md, "The shell hook"); it may be left out, and then no write is read as the brain's |
| `worker_homes` | The directory a worker added with no path is given a folder of its own under, named by the worker; it may be left out, and then a worker is added with a path alone |

Every key but `runner_environment`, `testbed_logs`, `brain` and `worker_homes` is required where the section stands. A path that is not absolute,
and a wait that is not above 0, is refused when the file is loaded. The environment the user's systemd
manager is reached on is the service's own process environment, not a setting.

### `[node]`

Where the file stands on a work node rather than on the operations machine (docs/verbs.md, "Where a verb
stands"). The section is left out on the operations machine, and its presence is what tells a verb it runs
on a node.

| Key | What it names |
|---|---|
| `name` | The node this machine is, as the workers' rows name it in `node` |
| `home` | The command, as a list of words, that runs this service's binary on the operations machine, such as an SSH login followed by the program; the verb's own words follow it. It may not be empty |

### `[words]`

The words of the deployment's own language that the service's policy reads and writes by, one table to
a policy part: `[words.closing]`, for example, gives the closing part's words. Each part says which
words it takes and what its English ones are (docs/policy.md); a key given here takes the place of the
part's own. The settings file loads whatever this section holds, and the policy refuses at its own load
what no part takes. Where the section is left out, every part reads by its English words.

## Why a load fails

A load that fails names the file and one of these:

- the file cannot be read;
- the file is not TOML of this shape: a section or a key is missing, unknown or of the wrong type;
- the database address is of a kind the service does not keep rows in;
- a deployment name is empty, or the owner's chat is 0 or not a whole number;
- a listening address is not a socket address, or a unit's address or the server's own is not `http:`;
- a host path or the wording document's path is not absolute, or the lock wait is not above 0;
- a node's name is empty, or its home command names no word.

The server also refuses to start where `[server]`, `[units]` or `[host]` is left out.
