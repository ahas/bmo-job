# The job's verbs

The service's `verbs` module carries, beside the traits the server is built on (docs/api.md, "Routes"),
the verbs a job runs from its own shell and a person runs at one: a question put to the owner, a long
wait marked, another worker's checkout borrowed, a branch landed and a job closed, the shell hook that
judges every command a job runs, the TODO's edits and holds, a work order read and changed, a job
launched, the clocks a run reached answered, research runs, and the readings of what a job cost. The
command line reads its words into these verbs and prints what they answer.

## Where a verb stands

- **The stand** is built once from the settings (docs/settings.md): the database, the queue's ground
  with the clients of every unit (docs/queue.md, "Where the queue stands"), the policy and the closing's
  reader, the deployment's names, the owner's chat from `[deployment] owner_chat`, and the owner as
  a sender, as bmo-receiver names him. A stand that cannot be built is refused before any verb runs, naming why.
- **A job's own process** is told by three variables of the program a work turn starts: `CLAUDECODE`,
  set by the program itself; `BMO_TASK_LOCKED`, set where the program runs a job's turn; and
  `BMO_RUN_NAME`, the run's name the turn was asked under, which is the job's unit. A process carrying
  the first two is a job's, and its job is the open job whose unit is the run's name. A step of a work
  order between its plan and its final turn also carries `BMO_WORK_ORDER_STEP=1`.
- **The server is told the queue moved** at its own `POST /queue/moved` (`[server] address`), so the
  screens read it again; a word that did not land is said on the error stream and fails nothing.
- **A verb runs on a work node** where the settings have a `[node]` section (docs/settings.md): its `name`
  is the node this machine is, and its `home` the command that runs this service's binary on the
  operations machine. A checkout on a node is the worker's whose row names that node and that directory,
  a directory written from the home (`~/`) being read against this machine's home.
- **Times are epoch milliseconds,** and a moment a verb prints is UTC, named as UTC.

## What a verb answers

- **A verb whose area has codes** answers them (docs/api.md, "Codes"): the TODO's edits and holds a
  `TodoAnswer` or a `TodoRefusal`, a job's question a `JobAskAnswer` or a `JobAskRefusal`, a wait a
  `JobWaitRefusal`, a branch reading a `BranchSplitRefusal`, a work order's change a
  `WorkOrderRefusal`, a launch a `TaskAnswer` or a `TaskRefusal`. The command line prints a code as its
  JSON on one line, and a refusal as its sentence.
- **Every other verb** prints English lines for the job or the person reading them, and ends with exit
  code 0, 1 where it refused or failed, 2 where its words did not read, or the code of the command it
  ran.

## The server's verbs

The binary `bmo-job` serves its routes on one type that carries every trait the server is built on
(docs/api.md, "Routes"), and starts the work-order runner beside it (docs/queue.md, "The work-order
runner").

- **It stands on the queue's ground** built from the settings, the policy and the closing's reader, the
  deployment's names, and the server's own event stream, so the news it publishes reaches the screens
  listening: `queue_moved` where a verb moved the queue, `order_changed` where it changed an order's body.
- **The owner as a sender** is asked of bmo-receiver when a TODO action or the judgement of whose work
  a launched item is first needs it, and kept from then on.
- **The TODO actions** go through the TODO's verbs where one is theirs (docs/rows.md), the closing among
  them; its hands are the server's own: the screens told on the stream, the queue's standing told at
  once, words added told in the owner's chat, and an item stopped by the item stop below.
- **An item stop** holds the item as stopped with the owner's reason inside its order's lock, stops a
  work order of it, marks every run of it stopped, gives back the pool places still waited for, and asks
  the runs to end after it has answered. The owner's chat is told of the stop as an `owner_stopped_item`
  fact, and then the queue's standing at once.
- **A work order's change** is made inside the order's lock on the body read there, saved, and told on
  the stream where the body moved; a change that freed the order's worker tells the queue's standing
  once the lock is let go.
- **A stop of everything** asks the units of the open jobs that still stand first, then the open
  executions that stand under none of them and are no co-worker, so the turn that asked is stopped last.

## A job's question to the owner

`job ask` puts a question to the owner from the job's shell and waits on it there (docs/queue.md, "A
job's question"): the job holds its checkout's lock, so its own run is what waits.

- **The question** names its item, the question, at least two options of one line each, the option the
  job recommends by its number from 1, and one line of why, and goes to the owner's private chat
  alone. It is refused, before the row is written and before the chat is told, where any part is
  missing or wrong (`JobAskRefusal`); where it names another room (`question_room`); where its words
  use a term the owner set aside (`question_banned_terms`); and where it hands the owner a page hosted
  outside the deployment's own archive, an address of a page published on Claude's artifacts, a gist
  or a paste site (`question_outside_hosting`).
- **The wait** is one minute, after which the recommendation settles it. A settled question comes back
  as one line, the option's words or the owner's own, with exit code 0 however it was settled.
- **Where the owner talks instead of choosing,** the question stays open and what he said comes back
  under exit code 3 as `owner_talked`, with the question's number and his lines. `job ask --talk <id> --say
  <words>` answers in the talk, optionally putting the question again with new options, a new
  recommendation, question or why, and waits on the same question again. A question settled meanwhile
  is not answered into: the wait finds it settled at once.
- **The room is told** each step as a `QuestionFact`, queued for the agent in the question's chat; a
  fact that was not queued is said on the error stream and the wait runs all the same.

## A job's mark on a long wait

`job wait` sets the mark the sweep reads before it calls a job quiet (docs/queue.md, "The sweep"):
what the job waits on, its target, a unit or a process, and when the wait should end.

- **The end** is a length from now (`40m`, `2h`, `1h30m`) or a clock time in UTC (`14:30`), today or
  tomorrow where that time has passed.
- **The mark belongs to the job running the item named;** an item with no running job carries none.
  A target already gone is refused, since there is nothing to wait on.
- **A command after `--`** runs under the mark, the mark's target being this verb's own process: every
  interrupt, terminate and hangup meant for the wait is passed to the command, the mark is taken away
  the moment the command ends where no later mark took its place, and the verb ends with the command's
  own exit code.
- `--clear` takes the item's mark away. Every change tells the server the queue moved.

## A run past its clock, answered

`overtime` is the agent's answer to a run that reached its clock (docs/queue.md, "A run past its
clock"): `--list` the asks still open, each with where its answer stands; `--keep <id>` for a length
(`20m`, `1h`, `90s` or a plain number of minutes), the run's own clock where none is given, and never
more than the longest keep; `--cut <id>`. A later answer replaces an earlier one while the run goes on.
An ask whose moment passed more than a minute ago is no longer watched and has nothing to decide. The
answer is written as the deployment's agent's.

## The live-Claude guard

`job live-claude --purpose <why>` opens the guard for the job running as this process's run, so the
shell hook lets through the commands it refuses before that (below), each one kept against the opening.
`--list` names what the job opened and what each opening let through. Nobody is asked; the count goes
into the item's report when it closes. Outside a job there is nothing to open.

## Borrowing a checkout

`job borrow <worker>` lends the job another worker's checkout for the rest of the job: a free support
worker's worktree first, then the base checkout.

- **Refused** outside a job, for the job's own repository, for a worker no row names, and where the
  machine cannot say which jobs run, since then no checkout can be called free.
- **A checkout is free** where no job or work order holds it, no other job borrowed it and its job lock
  stands free; the loan is a row of `checkout_leases`, given back at once where a launch took the lock
  in between.
- **A support worker's worktree** is made where it is missing and put on the item's branch made from
  main; **a base checkout beside support workers** is put on the job's branch too, what was open there
  carried across and recorded as its baseline; **any other base checkout** is lent as it stands.
- The job is told where the checkout is and how its work there is closed, by the deploy tree's
  `bin/bmo` named through `[host] tree`. When the job ends its runner lets the loan go (docs/runs.md,
  "The checkout before and after").

## Landing a branch

`job land`, run in a checkout standing on a job's `work/` branch, lands the branch on main the way
docs/runs.md, "Landing on main", states, with no check of its own. Main is read off the base checkout
the worker rows name for it; what a base checkout had open before its job began is set aside around
the rebase; a repository whose checkouts on work nodes push main takes the remote's main in first. It
prints the commit main stands at and how many commits it put there, or why it refused.

On a work node the landing is held on the operations machine: main is the checkout's own, read as the
remote's, and taken in from the remote before each rebase; the verb starts the `[node] home` command
followed by `node-land --worker <name>` for the worker this checkout is, with `--record <reason>` where a
close records what it lands. It lands once that says `held`, writes `landed <commit>` or `nothing` to it,
and passes on what it says after. A checkout on a node that no worker's row names, a home that never says
`held`, or one that cannot be started, is refused as a landing whose lock was not had.

`node-land --worker <name>` is the operations machine's side of a landing from that worker's checkout on
a work node: it takes the landing lock of the repository the worker's base checkout belongs to, says
`held`, and waits for the node's word on its standard input, `landed <commit>` or `nothing`. Landed, the
base checkout here follows the remote's main and, where a manifest, a lockfile or the submodules moved,
is set up again; with `--record <reason>` the commit is recorded as waiting to go live through
bmo-testbed.

## A job's close

`job close --then <why>` closes a job in the repository its checkout belongs to, once, at its end. It
never deploys.

- **Refused** where `--then` is empty, where `--skip-suite` gives no reason, where either reason or a
  commit of the job's own holds Hangul past the names of address (docs/policy.md, `close.toml`), and
  where the commit is on no remote.
- **The close line.** The close runs its own check-and-land again as a command in the repository's
  close line through bmo-testbed's `close`, so one close of a repository checks and lands at a time on
  the machine; a close whose job ended before its turn ends as not run (exit 75).
- **Inside the line:** a job's branch is rebased onto main and set up; a commit already live or already
  waiting is handed over again as it stands; otherwise the change check runs from main's head (or from
  the newest commit live or waiting, on main itself), then the test count, then the suite with what
  failed or never started run once more, unless `--skip-suite` gave why the change reaches nothing the
  suite runs. Each check is a call on bmo-testbed, its log under `[host] testbed_logs` or the
  documents' `close/`; a check whose turn did not come ends as not run.
- **Landed and recorded.** A job's branch lands on main, re-checked where its rebase took in commits
  the check did not cover; the commit is then recorded as waiting to go live through bmo-testbed. A
  suite that failed records nothing, so nothing the suite did not pass ever waits to go live. The last
  line names each test that passed only when run once more. On a work node a job's branch lands under the
  operations machine's hold ("Landing a branch"), and the record of what it landed is made there, through
  `node-land --record` with the close's `--then` reason, its line the last the operations machine said.

## The shell hook

`job guard` is the hook a job's program runs before every shell command and every file edit, the hook's
input on its standard input. It answers nothing for a call it lets through as it is, and a refusal with
the reason otherwise; a hook that cannot read its input stops nothing.

- **Never let through,** whatever the job opened: the agent program started in print mode, a removal
  the program would stop on for a confirmation (with the way to write it), a shell habit that stalls a
  job (a pager, an interactive editor, a wait with no end), and a commit message, tag or branch name
  holding Hangul where the command line shows it.
- **A heavy command** — a test or build a language of bmo-testbed claims — started outside the testbed
  is refused with the line that runs it there, as bmo-testbed's `classify` answers it.
- **Opened by the live-Claude guard:** a command that writes into the agent program's session records,
  starts the program itself or forks a session. Without an opening it is refused, naming the verb that
  opens it; with one it runs and is recorded against the opening.
- **The structure document** `STRUCTURE.md` is changed only by a job whose item the owner requested
  himself and whose words name it.
- **Another registered worker's checkout** is not written into: the hook names the borrow verb. A file
  of the job's own checkout written with an inline script, an in-place editor or a redirect is refused
  with the verb that edits it.
- **The brain.** A job's first write into the brain's directory holds the brain's editors off through
  bmo-brain, and a write while an editor holds its lock is refused with the way to wait for it.
- **A command let through** runs with its standard input closed.

## The TODO's edits and holds

The edits and holds of an item stand on the ground of `queue-rows` (docs/rows.md, "Where a verb
stands") and answer `TodoAnswer` codes.

- **An edit** rewrites the filer's own parts of an item — its title, what, do, conclusion and
  judgement — and a draft work order's intent and instruction where the draft still says what the part
  said. Refused for a part named with no words, for a finished item, for words a running job is at work
  on, for a part the item does not have, and for an order that has started.
- **A merge** puts items into another, which carries their work and finishes them with itself; **an
  unmerge** takes one out again. **A resolution** settles an item's unfinished note with one line of
  why.
- **Closing** an item writes its report and unfinished note, each held to the closing's readings
  (docs/rows.md, "The closing's readings"); finishes the items merged into it; settles the notes it was
  the last open strand of; hands the strands left to the agent to it as memories; and tells the agent
  of every held item waiting on it. An item wound down or stopped is not closed as done. **Opening
  again** reverses the closing with the items merged into it.
- **The settings:** the state line, the worker (a support worker is refused), the item it repairs —
  a chain of repairs stops at three rounds, where it is the owner's to look at — co-work, priority and
  model. **Dropping** throws a row out.
- **A hold** sets an item aside, with a resume condition where one is named: another unfinished item
  finishing, or commits going live, an item's commits waiting to go live or one commit on main and not
  live yet, as bmo-testbed reads them. **A stop** has the item's runs asked to end and holds it as
  stopped. **A wind-down** holds an item to resume later, telling the running job at the end of its
  step. **A stop-here** leaves where a wound-down, stopped or told-to-stop run stopped on the item as
  its words and keeps it open and held; a step of a work order holds its item only at a final turn
  with nothing ahead of it, and only once. **Letting a hold go** is never a job's.
- **The queue's pause** stops or starts the queue's own progress, and **letting a deployment's hold go**
  by hand releases it; each start tells the agent how the queue stands.

## A work order read and changed

`work-order --todo <id>` reads the item's order, or the draft an item with none would start from, as
lines under a ceiling or whole with `--json`. A change — one verb a call: resume, reopen, a task's
edit, skip, restore, rerun, move, add, close, recover or checkout — is never written from here: it goes
to the server's `POST /work-order`, which makes it in the order's lock beside its runner, and the order
as it then stands is printed. A server that does not answer is a refusal. A resume while the queue is
paused or a deployment holds it says which.

## A job launched from a shell

`job start` asks the server's `POST /task` for the launch its words name, so the running service
decides every launch; the answer or the refusal is printed as it came. A server that does not answer
launches nothing.

## Research runs

A research run — a search or a browse — runs through bmo-agency and stands on what bmo-agency keeps:

- **Its run** is opened in bmo-agency's run ledger when it starts, with its agent, room, unit, depth and
  clock, and closed there once, with how it ended and the context it ended carrying; the first ending
  reached is the one that closes it.
- **Its profile** is the research key's values as the launcher took them, read back for the key it is
  for and refused where it is another key's or breaks a check the policy's research part holds a field
  to (docs/policy.md, `research.toml`). A clocked key's clock closes research before its ceiling by the
  reserve kept for writing the result.
- **Its branches' places** are seats reserved in bmo-agency's co-worker pool, held from before its
  clock starts until it ends, under the owner `<source>-run:<run>`; every seat and request of its own
  is given back however it ends, and a sweep reclaims what a run whose ledger entry is closed and whose
  unit no longer runs left behind. An optional deeper run with no free place in the pool goes at the
  shallowest depth instead, saying why; an explicit one waits for its first seat up to its profile's
  wait.
- **A branch** takes a place in the pool and waits for one, runs its angle on the place's model, is
  stopped at its run's research close, gives the place back however it ends, and answers with its notes
  framed as working notes.
- **Its model** is carried as a work turn on bmo-agency on the search line's work key, its web server
  told the run's settings in the variables `research.toml` names; the run's prompt names the moment it
  was asked at in UTC, and the knowledge its words call for is handed as a reference block naming the
  deployment's agent.

## What a job cost

- **`job figures <item…>`** reads, for every job of each item, its wall time, the model's share of its
  run, its shell calls and their time by what they ran, its model requests, its tokens, its graph tool
  calls, its compactions and its longest results kept whole: the spans it wrote as it ended and its
  lines in bmo-agency's agent log.
- **`job tool-calls --todo <id>`** reads the last job of the item's own thread call by call: each tool
  call paired with its result in the agent log, how long it was out, a call still out read as running
  until the job ended, the suite's own calls counted apart, longest first.
- **`job branches`** reads what went on inside a Claude Workflow's branches out of the records the
  workflow kept for them under the job's own configuration directory: each branch's length, the
  stretch some call it made was out, and the rest, which is the model's.
- **`job cowork-timing`** reads where a work order's time went: the time it ran, the part in builder
  turns and in branch stages, the tool time and output tokens inside builder turns, the stretch from
  the plan's start to the last turn's end and how much of it nothing ran, and the time bmo-agency's
  pool kept it waiting.

## A checkout set up

`checkout setup` sets up the checkout it is run in the way its repository sets one up: the submodules
its commit pins brought to their pins, then the repository's own setup script where it names one. A
setup that could not finish says why and ends with 1.
