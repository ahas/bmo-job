# The command line

The binary `bmo-job` carries these commands.

| Command | Does |
|---|---|
| `bmo-job nouns` | Every noun this unit carries (`job`, `job ask` and `todo`), one a line, `job ask` marked `home` after a tab, which `bmo` reads to find the commands it hands this unit |
| `bmo-job serve` | Serves the API by the settings file (docs/settings.md) until it is told to stop by an interrupt or a termination signal; every open event stream ends then, so the stop is not held |
| `bmo-job mcp --service <url>` | Serves the harness plugin's MCP server on standard input and output, reaching the service at the address (docs/api.md, "The harness plugin") |
| `bmo-job job close` | Checks what the job changed, merges its commits onto the remote's main and leaves the item waiting for its repository's batch deployment (「`bmo-job job close`」) |
| `bmo-job job ask` | Puts a question to the job's supervisor in the middle of a job, which settles it or puts it to the owner, and waits for it to be settled (「`bmo-job job ask`」) |
| `bmo-job todo` | Closes an item, holds it with the run's account on it, or raises a new one, each through the service's own routes (「`bmo-job todo`」) |
| `bmo-job openapi` | Prints the OpenAPI document, the one committed as `docs/openapi.json` |

## `bmo-job job close`

`bmo job close --then '<why this goes live, in English>' [--todo <item>] [--skip-suite '<why, in English>']` runs this, through
the unit command the deployment links into `~/.local/bin` (docs/deploy.md, "The unit command").

| Option | What it does |
|---|---|
| `--then <why>` | Why this goes live, in English; required. A reason holding Hangul is refused |
| `--skip-suite <why>` | Read as a reason, English like `--then`, and nothing more: the close runs no suite, so it changes nothing, and the close says so in one line |
| `--todo <item>` | The item the job works on; the close writes where it stands onto that item (below) |

A close ends at the merge. It checks what the job changed, puts the job's commits on the remote's main, marks
the item waiting for its repository's batch deployment and ends; it runs no suite, waits for no deployment and
confirms nothing live. The suite, the deployment and the live check are the batch deployment's, which puts every
commit waiting on main live at once, its gate running the suite once for all of them (docs/deploy.md, "What gates
a deployment").

```mermaid
flowchart TD
  A[Reasons read] --> B[Remote fetched]
  B --> C{Commits not on main?}
  C -- yes --> D[Main must stand under HEAD]
  C -- no --> E[Range from the record]
  D --> F[Commit messages read]
  E --> F
  F --> G[Check since the base]
  G --> H[HEAD pushed where main lacks it]
  H --> I[Item waits for the batch]
```

- **The main it lands on.** The remote is the one the checkout's branch follows, else `origin`; its main is the
  remote's `main`, else its `master` where the remote holds no `main`. The close fetches that remote first, and
  a remote that cannot be read refuses the close with what git said.
- **The job's own commits.** Where `HEAD` holds commits the remote's main does not, those are the job's own, and
  the base is the remote's main. A branch the remote's main has moved past, so that it no longer stands under
  `HEAD`, is refused with the command that brings it up: the close merges by fast-forward alone and never
  rewrites the checkout.
- **A commit already on main.** Where the remote's main already holds `HEAD`, nothing is pushed, and the range is
  read from the deployment record (docs/deploy.md, "A job closes"): the job's commits are those past the commit
  the record names as live. Where the record's commit is not in the repository or is no ancestor of `HEAD`, the
  range is taken from the job's own base commit instead, and the close says in one line which range it used. A
  commit the record already names as live runs nothing and ends clean.
- **What is refused before the check.** A commit of the range whose message holds Hangul, each such commit named
  with the command that rewords it; nothing is checked, pushed or written on the item.
- **The check.** `bmo-testbed check --since <base>`, which `bmo check --since` runs, runs in the checkout: the
  check a change owes over every path changed since the base (bmo-testbed's `docs/checks.md`). A check that does
  not pass refuses the close with exit 1, and nothing is pushed.
- **The merge.** Once the check passes, the job's commits are pushed onto the remote's main as `HEAD` stands. A
  push the remote refuses, its main having moved while the check ran, refuses the close with what git said and
  the way to close again; nothing is written as waiting.
- **Where the close stands is written on the item `--todo` names,** as two of its labels, the rest of its labels
  kept: `close-phase` is `testing` from the moment the check starts and `waiting-batch` once the commit stands on
  main, and `close-commit` is that commit, written with `waiting-batch`. A reader of the queue tells from them an
  item whose work is being checked from one whose commit waits on main for its repository's batch deployment.
  Taking them off once the commit is live is the deployer's, as is the batch deployment itself.
- **Both labels are taken off** where the close is refused, where its check does not pass, where its push is
  refused, where the close is told to stop, where the commit is already what is deployed, and where the
  repository deploys nothing, so an item carries them only while its check runs or its commit waits.
- **The item is reached as `bmo-job todo` reaches it** (「`bmo-job todo`」), and a restart is no answer. While
  the service does not answer, nothing at its address, a connection cut, a 5xx or a request left unanswered for
  ten seconds, its labels are written again, after a quarter of a second at first and every five seconds at
  most. The close goes on once they are written, once two minutes have passed with no answer, or once the run
  is told to stop; the last two are said in one line. An item the service refuses, one it does not hold or a
  change it will not take, stops no close: the close says so in one line and goes on.
- **A close with no `--todo` writes on no item.**
- **The check is watched.** It is started in the checkout, in a process group of its own, its standard output
  and error read through pipes and written on as they come. It is handed `BMO_TESTBED_HEARD`, naming a file of
  the close's own, which the testbed touches while a command it watches writes but holds its output back; a
  touch counts as output.
- **A check that falls silent ends the close.** A check that writes nothing and touches nothing for the close's
  silence limit is hung. The limit always stands five minutes above bmo-testbed's own bound on a silent unit,
  so the testbed ends a hung unit first and names it, and the close's limit catches what the testbed does not
  watch. Where the checkout's `testbed.toml` declares `[test] silence_seconds`, the close's limit is that many
  seconds and five minutes more; where it declares none, or its declaration does not read, the testbed keeps
  its ten-minute default and the close's limit is fifteen minutes. The close names every process still running under the check, in its process
  group or below it, sends each a TERM, and a KILL to whatever stands ten seconds later. It then reads the
  checkout's `git status` against what it read before the check, and refuses the close with the check's last
  line, what was still running, and whether the checkout stands as it found it or which paths differ:
  `job-close: refused — the static check wrote nothing for 900s and was ended`, the seconds being the limit
  the close kept to. The close itself writes nothing
  in the checkout.
- **A running close says where it stands.** Every minute while the check runs, one line on standard output
  names it, how long it has run, how long ago its output last moved and its last line, so a reader tells a slow
  check, whose output moves, from a wedged one, whose does not. Whatever ends the close, its last line on
  standard output is `job-close: ended, exit <code>`, so a caller reading its log needs no process listing to
  know it has ended.
- **Every wait inside a close has a clock the code names.** Each request writing the item's labels waits ten
  seconds and the writing as a whole two minutes; each local `git` call one minute, and the fetch
  and the push five minutes each, after which it is ended and read as failed; the check's silence fifteen
  minutes, or five more than the checkout's declared testbed silence, and the pause before the KILL ten seconds; the running check is looked at every second.
- **Told to stop, a close ends the check it runs first,** every process under it as for a hung check, so
  nothing it started goes on after it.
- **What the close says is the account of its close.** A close that ends at the merge prints, before its last
  line, the commits it put on main, each as its short commit and subject, and the check that passed with the
  base it ran from. That is what a job's report says of its close: what changed and what was checked, and that
  the commit waits for its repository's batch deployment.
- **How it ends:**

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | The check passed and the commit stands on main, its repository deploying | `merged:` with the commits, `checked:` with the check, and `waiting for the batch deployment: <commit> stands on <remote>/<main>` | | 0 |
  | The same in a repository whose `testbed.toml` reads and declares no `[deploy]` (a base library, released and never put live) | `merged:`, `checked:`, and `on main: <commit> stands on <remote>/<main>, and this repository deploys nothing` | | 0 |
  | The record already names the commit as live | `this commit is already what is deployed` | | 0 |
  | A reason, a commit message, the remote, a branch main has moved past, the check or the push refused it | | `job-close: refused —` and why | 1 |
  | The run was told to stop | | one line saying so | 130 |

## `bmo-job job ask`

`bmo job ask` runs this. It is what a job's own run waits on: nothing ends the wait but what is listed here, and
no clock is among it.

A job asks its supervisor, never the owner directly. The question is opened in the owner's private room as the
supervisor's (docs/api.md, 「The addressee」) and handed to that room's supervisor at once, which judges what it
is: it does or finds what is asked and settles the question with the result, talks it over with the job, or puts
it to the owner in its own words, and from then on it is the owner's question as any other.

| Form | What it does |
|---|---|
| `--todo <item> --question <words> --why <words> [--option <words> --option <words> --recommend <n>]` | Opens the question for the supervisor of the owner's private room, hands it to that supervisor and waits |
| `--talk <question> --say <words>` | Puts the job's answer to the supervisor's or the owner's talk in the same room, then waits on the same question |
| `--talk <question> --say <words> --option <words> --option <words> --recommend <n> [--question <words>] [--why <words>]` | The same, with the question put again in other words |
| `--show <question>` | Reads one question by its number and waits on nothing |
| `--todo <item> --show` | Reads every question asked about the item and waits on nothing |

- **How it ends:**

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | The supervisor settled it | Its answer, as it wrote it | | 0 |
  | A person settled it, once the supervisor put it to him | One line: the option's words, or his own | | 0 |
  | Somebody settled the same question before this run | The same answer | `ask: question #<n> was settled before; its answer stands` | 0 |
  | The supervisor or the owner talked instead of settling; the question stays open | Their words and the call that answers them | | 3 |
  | The question could not be put, the receiver refusing it; it is closed `unsent` | Nothing | Why | 1 |
  | The question was withdrawn, or the run was told to stop, which withdraws a question already opened | Nothing | Why | 1 |
  | The words were refused; no question opened | Nothing | Why, and the usage | 1 |

- **A question asks for what the job cannot settle or do itself,** said in words read at a glance: a decision
  that is not the job's, or something only the supervisor's reach grants, such as a command on a node the job's
  account does not reach or a deployment the work cannot go on without. A choice puts two options or more with
  the one the job would take; a request the supervisor answers in its own words puts none. Either way it says
  why the job cannot settle it itself. Its words are refused, and no question opened, where any of them carries
  a fenced block of code or a token of more than 80 characters with no space — a script, an encoded payload, a
  hash or a long address — or where the question runs past 500 characters or 5 lines, an option past 160
  characters, or the reason past 240. A long reading goes into the archive and its address into the question.
  The job's answer to a talk is refused on code the same way, its length its own.
- **An answer comes back as it was given.** The supervisor's is printed as it wrote it, a result of several
  lines among it; a person's is one line, the option he took or his own words. A recommendation is shown and
  never answers: it is printed only where a person settled the question with it. A settling reply no person or
  supervisor gave is refused: the verb ends 1 and prints no answer.
- **Putting the question.** The verb reads the owner's private room from the receiver's `GET /owner`, opens the
  question there for the supervisor with `toldByAsker` (docs/api.md, "Questions and replies"), and hands it to the
  room's supervisor through the receiver's `POST /instructions`, an order under the sort `job` of the kind
  `question-for-supervisor`. An answer other than a queued instruction is a failed send. A question that is
  settled, put to the owner or stopped has its instruction withdrawn where it has not reached the supervisor yet.
- **What a turn carries.** An order carries the notice's kind and number, the item's number and title, and the data
  it is about: the question, its options numbered with the recommended one marked or the word that it is open, and
  why it is asked; or the job's words in a talk, with the question's words and options again where the talk changed
  it. An order for the supervisor also names the tools that settle the question, talk on it and put it to the
  owner (docs/api.md, 「The tools」). It carries no other instruction, since how a notice is read stands in the
  receiving session's own instructions.
- **While the supervisor holds it,** the question has no due time and no clock bounds the wait; the supervisor's
  notice does. The receiver keeps a notice with no expiry and holds it while the supervisor's session cannot be
  reached, so a supervisor that is busy or down delays the answer and loses nothing. The verb reads the notice as
  it waits. One the receiver let go before the supervisor settled the question, talked on it or put it to the
  owner — its round stopped, its session reset, no turn able to take it — will never be taken, and the question
  then goes to the owner as one no supervisor could take: the verb withdraws it, opens a new question in the
  job's own words put to a person, hands that to the room's supervisor as `question-awaits-answer` and waits on it
  as on any of his, saying on the standard error which question went where. A notice whose turn ended while the
  question still waits is left to the supervisor, since work it started, a deployment among it, can end later.
- **Once it is the owner's,** put to him by the supervisor or by the verb as above, the verb says so on the
  standard error once and waits on his answer: the due time the service counts from that moment, a minute where
  the ask named none, paces the one notice `question.due` tells and settles nothing.
- **A restart is no answer.** Every call the verb makes, reading the item's questions and the answer one was
  settled with, reading the owner's room, opening the question, reading the item's title, handing the order or
  the talk over, reading the order, keeping the job's words and where it heard through, closing the question, and
  every read while it waits, is made again while the service or the receiver does not answer: nothing at its
  address, a connection cut, or a 5xx. It is made again after a quarter of a second at first and every five
  seconds at most. The run told to stop ends it as above. Only an answer ends a call: while the question is put, a
  refusal closes it `unsent`; while it is waited on, only a question the service no longer holds ends the wait,
  with 1.
- **One question, one row, told once.** Before anything is opened, the item's questions are read, and the last
  one of the same asker, words and options that is not `withdrawn` or `unsent` is the question this ask
  stands as. One `open` or `paused` is waited on as it stands, told to nobody again, and the standard error
  says so with its number. One `answered` comes back with the answer it was settled with, its number on the
  standard error, and nothing is opened or told. Only where every such question stands `withdrawn` or
  `unsent`, or none was asked, is a new question opened and handed to the supervisor.
- **At least once.** A call cut after its peer took it is made again, so an order or the job's words in a talk
  can reach the room twice. A question is opened once: where opening it was cut, the item's open questions
  are read before it is opened again, and one of the same asker, words and options is taken as the one opened.
- **Reading a question.** `--show` prints each question as JSON, one a line: the question as the service keeps
  it (docs/api.md, "Questions and replies") with `answer`, the answer it was settled with, or null. It reads
  the service once, opens nothing and tells nobody; a question or an item the service does not hold ends it 1
  with why on the standard error.
- **Pages hosted elsewhere are refused:** an address of a claude.ai artifact, a published Claude page, a gist or a
  paste in the question, an option, the reason or the words of a talk.
- **Where it reaches the receiver:** `BMO_RECEIVER_URL` with `BMO_RECEIVER_TOKEN` or `BMO_RECEIVER_TOKEN_FILE` in
  the run's environment, else `RECEIVER_ADDRESS` and `RECEIVER_TOKEN` in `env/receiver.env` under the data root.

## `bmo-job todo`

The worker's run already names the job service in `BMO_JOB_URL` and its token in `BMO_JOB_TOKEN` or `BMO_JOB_TOKEN_FILE`; a shell with neither reads the deployment's `settings.toml` beside the binary, or the file `BMO_JOB_SETTINGS` names.

| Form | What it does |
|---|---|
| `--done <item> [--report <text>] [--unfinished <text>]` | Leaves the run's account of the finished work on the item: `--report` kept as a `final` report and `--unfinished` as a `held` one through `POST /items/{itemId}/reports`. It moves no state: the item is done once whoever ordered the work takes it — its review, or, for an item whose commit waits on main for its repository's batch deployment (its label `close-phase` reading `waiting-batch`, 「`bmo-job job close`」), whoever puts that commit live. The verb prints one line saying what the item waits on: `deployment` with the commit, `review`, or `orderer` |
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
