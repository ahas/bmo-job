# Running a job

The service's own `job_run` module is the program a job's unit runs: the `job-run` verb, started through
the host's systemd-run with the arguments the queue's launch gave it (docs/queue.md, "Starting a job").
It takes the job from its locks to its handoff: the worker's checkout before and after, the prompt, the
turns, the judgement of the last answer, the ending written, and the session handed over. Every turn is
carried by bmo-agency as a work turn (docs/api.md, "Work turns"); what the turn says and what is made of
its answer are the runner's.

## What the runner is handed

In the order the unit passes them: the chat, the prompt file, the job, the model asked for (`auto` where
none), the unit, the worker and its directory, and for a work order's run its phase (`work-step` or
`work-final`), its session word (`resume`, `fresh` or `handover`), the structure a middle turn answers in
(`plan` or `synthesis`), the baseline a final turn is measured against, and the model a stopped stage's
last retry runs on. A run with no chat, prompt file or job never starts.

- **The verb** is the binary's own, `bmo-job job-run -- <words>`, the program `[host] runner` names
  (docs/settings.md). It reads the settings the server reads, builds the queue's ground from them
  (docs/queue.md, "Where the queue stands") and the runner's ground on it: the queue's calls and the run's
  own, the telling, the host, the documents, the platform's tree, the testbed's logs and the cache siren,
  the policy and the closing's reader built from the deployment's names, the owner's set-aside terms, the
  owner's chat, and the deployment's agent by its key and its name. It then holds the locks below and runs
  the job, ending with the run's exit code. Words that do not read end it with code 2 before anything is
  held, and settings or a database that cannot be had with code 1.
- **What the runner tells the screens** goes to the server at `[server] address`, since the runner is a
  process of its own and not the server's stream.

- **A co-work branch** is a step whose session word is `fresh`: it runs on a session it throws away, in a
  place of its own, and keeps nothing of the worker's.
- **A middle step** (`work-step`) tells every tool it calls through `BMO_WORK_ORDER_STEP=1`, closes no
  item and hands nothing over.

## Locks

The runner holds the checkout's job lock and the job hold of the checkout before it does anything, each
in `[host] locks`, and waits for whoever holds them. The pane a worker's turns go into is bmo-agency's to
serialise.

## The checkout before and after

- **A worker on a node** has its checkout there: every git question goes over one shared SSH login to the
  node, its reach read from bmo-agency. A node with no reach refuses the job before it starts.
- **A support worker, a worker of its own in a linked worktree, and a base worker beside support
  workers** work on the item's branch (`work/<item>`, or `work/job-<job>` for a job with no item): the
  branch is made from main where it is not there yet, what the last job left open is parked on the
  branch that job was on, and a support worker's worktree is made where it is missing. A base checkout
  that could not be put on the branch refuses the job.
- **The baseline** is where the checkout stood before the job: a final turn's own, carried across its
  order; nothing open in a worktree of the worker's own; the checkout as it stands otherwise.
- **As the job ends,** before its report goes out, the checkouts it borrowed are let go — a borrowed
  base checkout put back on main, a borrowed support worktree's open change parked on its branch — and a
  base checkout on the job's branch goes back on main, what the job left open committed onto its branch
  and what was open before it began carried back. What became of them is said under the report.

## The prompt

The prompt is the worker's preamble, the item's impact grounds where the job is not a branch, the job's
own text through bmo-gate's sanitiser, the knowledge bmo-gate picks for the job's words, and the trailer
from the worker's job documents. A step of a work order is told the work step's line and the graph
instead of the trailer. The prompt waits up to three minutes for the gates, asking again every five
seconds, and a job whose prompt never comes ends before its first turn. The English the worker read of
the job's own text is kept on the job's row. The preamble names the workbench and the item's scratch from
bmo-agency's workbench, the branch the job works on, and the dev server's verbs through `[host] tree`.

## The turns

Each run of the job — its first, a later turn's, a shaping turn's, a handoff note's — is walked over the
roads (docs/queue.md, "Routes"): each provider not yet tried, in order, a blocked road passed by, a
registered model's road closed for an outage opened again where its engine answers its health. A switch
is told as a `RoadFact` and a handover line goes ahead of the prompt; a failure closes its road for its
while (a quota fifteen minutes, an outage one, no access five) or until the moment it named, and a full
Claude window rests the login it went out on. A co-work branch runs on the pool lease its task holds and
on Claude Code, Codex or OpenRouter alone. A run with no road left is refused `job-route-exhausted:`.

- **A turn** is prepared on bmo-agency, which answers the session it continues. The runner then opens the
  earlier results the prompt carries: one the session holds in that very version goes as its marker, the
  rest whole. A session that last answered for another item is told so first. Where the walk moves to
  another login whose session differs, the prompt is opened again for it. A co-work branch's turn goes on
  a session thrown away, in the branch place bmo-agency took for it.
- **bmo-agency's turn keeps the worker's session:** it carries out a reset the owner asked of the worker's
  sessions before the turn goes, records the session the turn answered on, and records the turn's usage,
  so the runner asks none of these.
- **While it runs,** the changes the agent made to the item's brief and the wind-down line are pushed into it
  through the sanitiser; a question the program stops on is answered by the runner where it is a command
  that stays inside the job's own places, and put to the owner as a job question otherwise ("A command the
  runner allows itself").
- **After its answer,** the words added to the item are asked over again, up to three rounds; a report
  left with background tasks running, with its own change open or the graph behind it, with its change
  reaching the graph unanswered for, or with its item left open, is asked once more each.
- **A middle turn of a work order** that answered its design is asked for the shape its structure takes,
  and the answer is checked; a refused shape is asked again twice, each refusal kept with its reason under
  the job's folder of `[host] documents` and told as `work_output_refused`.
- **A stopped stage retried on a fresh session** has the worker's kept session handed over first.

## What a failed turn was

A turn that did not answer left text that is read once: a login gone, a full window with the moment it
resets, or the run having died, each by name. A program's own error in place of an answer that names a
server error is the service being out.

## The judgement

- **A middle turn** is done where it reported anything and walked away from no workflow, and failed
  otherwise.
- **Any other run** failed where it reported nothing; ended `live_tasks` where background tasks were
  still running; ended `unclosed` where it walked away from a workflow, left a change of its own
  uncommitted, or changed a file the graph names and left the graph alone; and is done otherwise.
- **A done report** is handed over with notes under it: its shape (summary, detail and a short closing
  with figures), a screen it changed and said nothing of sweeping, a check it left undone, a word the
  owner set aside in the owner's wording document (docs/settings.md), what it left uncommitted, an item it left open, and a change reaching the graph its
  report does not answer for.

## The ending

The job's row is closed with its report and outcome, a run that broke is handed to the agent, and where
the job's time went is written into `job_spans`: the wait for the lock, the preparation, the run and the
finish, the spans its turns kept, and what each model spent. A run that ended on a limit, a person or a
stop is told as `job_stopped`; a middle step is told nothing; any other report is left waiting and
delivered by the telling at once, and a report no row would keep goes to the agent as it stands. The job
is then handed off: the servers its turns left listening are swept, and the queue's telling hears the
ending. A work order's run ends as a job only as its final turn ending done with nothing ahead of it.
Once the job's last turn has answered, the session it answered on is handed over with the worker's own
note or a summary where bmo-agency's record still keeps it — where the worker's state, read for the job's
model, continues that very session — and a done job's usage begins the rest it calls for. A co-work
branch's session was thrown away and is handed over to nobody.

## Landing on main

A job's branch lands on main under the repository's landing lock in `[host] locks`, or, for a checkout on
a work node, under the operations machine's through `node-land`: rebased onto main where it moved,
checked again by the caller where the rebase took in commits its check did not cover, refused where a
commit message is not English by the close's own reading, pushed to the remote's main as a fast-forward,
and only then main moved here. Installed units linked into the checkout main stands in follow its files
through bmo-testbed's unit links: a move bmo-testbed holds is refused, and the links it plans are relinked
once main has moved.

## A command the runner allows itself

A `Bash command` question is read as the shell splits it. It is the job's own work, allowed without
asking anybody, where every path it names lies in the job's places — the checkout, the item's scratch,
the temporary directory, `[host] testbed_logs`, the platform's command line and the standard streams —
it removes nothing but what lies strictly inside one of them, hands a value this reading cannot resolve
only to a program that only reads, and runs nothing as somebody else. A command nothing here can restore
after is recommended refused; every other question is recommended allowed, and settles there unanswered
after a minute.

- **The question arrives on the turn's session:** bmo-agency shows it on the session's state while the
  turn waits, and takes the runner's answer to it; it judges none of it (docs/api.md, "Sessions"). A
  co-work branch's questions are judged as the worker's are.
- **A question that is not the job's own work** is put to the owner as a job question, told as a pane's
  question: allow or refuse for a command, close or leave for a question this reading does not know, the
  recommendation marked. A job with no item to hang it on is refused at once, since waiting is the one
  thing such a run must not do.
- **The settled question is the answer:** allow lets the command go on, refuse and close refuse it, and
  leave leaves it standing for a person in the pane. Words of the owner's own refuse it with a paragraph
  naming the owner as the deployment names them, quoting the words, and telling the program not to retry
  the command as written and to carry on in the same turn; a refusal with no words of the owner's carries
  bmo-agency's own paragraph.
- **A question the pane stops asking** before anybody answered it is settled as answered in the pane, and
  nothing is sent.
- **A question this reading does not know** has the screen it stood on written to the job's run log by
  bmo-agency as it is put, so the refusal of it can point the program there.
