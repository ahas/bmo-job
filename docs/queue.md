# Running the queue

The service's own `queue` module runs the job queue on the rows `docs/schema.md` lists and the readings
and writes of `queue-read` and `queue-rows`: who works where, whose turn it is in each worker's line,
what a launch is judged against, what is running, how a job ends and what the agent and the owner are
told of it. Whatever it needs of another unit it asks through that unit's API (docs/api.md, "Calls on
other units").

## Where the queue stands

Every verb of the queue works over one ground: the database, the policy, the deployment's names and the
calls on other units. The calls go through one trait of the module, which the real ports carry and a test
stands in for; a call that fails is refused with 502 under the code of the unit it was made on, as every
other call of the service is.

- **One place builds the ground from the settings** (docs/settings.md): the clients of bmo-agency,
  bmo-receiver, bmo-web, bmo-gate, bmo-graph and bmo-brain, with its token, from `[units]`, and of
  bmo-testbed from `[host] testbed`, the host a job's unit is
  started on from `[host]`, the owner's chat from `[deployment] owner_chat`, and the telling, the
  stops and the reading of what runs from `[host]` and `[server] unit`. The telling and the work-order
  runner tell their facts in that chat. The verbs and the server take the queue from there, and a settings file missing
  `[server]`, `[units]` or `[host]` builds none.
- **Times are epoch milliseconds** throughout, in the rows and in what the queue hands on.
- **Every statement runs on SQLite and on PostgreSQL alike.** A work order's body is read whole and
  filtered in the code, never by a JSON operator of one backend.

## Workers

A worker is a checkout that jobs run in, kept as a row of `workers`.

- **The built-in worker** is named `worker`. It stands whether its row has been written or not, works in
  the service's source root where its row names no path, and is never removed, rested or moved.
- **A base and its support workers.** A base worker may keep support workers, numbered from the policy's
  first support slot up to its support count, each named `<base>-w<slot>`, labelled `<label>#<slot>` and
  working beside the base's checkout in `<path>-w<slot>`. Settling a base writes the support rows its
  count asks for, brings their labels and paths to the base's, and removes those the count no longer
  reaches once nothing of theirs is at work.
- **A checkout on a node** is a support worker numbered by no slot, standing on a work node and labelled
  `<base label>@<node label>`, the node's label read from bmo-agency. It stands until it is removed.
- **A resting worker** is no worker of the list. Only a base the owner added may rest.
- **What a worker may be.** Before anything is written, a worker's name, label, path, preamble, dev
  server command and support settings are judged against the policy's worker rules, and a change or a
  removal against what the worker is. Each refusal is a code of `AgentRefusal`.
- **A worker's pin and keepalive** are bmo-agency's: the queue reads the pin in force through the
  worker's runtime, and checks a pin's value through bmo-agency before it is set.

## Routes

Every job of a worker goes out on the work key `job-worker`, on the run line the policy's profile of that
key names.

- **A selection** is `<provider>::<model>`, the provider being one bmo-agency lists. A Claude model named
  on its own is a selection on `claude-code`, and `auto` asks for the walk.
- **A worker's walk** is a selection on each provider that is on and carries the job's line, in the
  provider order, each model resolved through bmo-agency's routes at the tier asked or at the profile's
  own tier. A worker pinned to a provider and a model walks that one selection; pinned to a provider
  alone, that provider.
- **A route is blocked** while its provider is off, while the deployment rests where the provider
  is `claude-code`, or while a failure that closed it stands: the `job_route_block:` setting of the
  provider, or of the provider and the model where the provider's models are registrations of their own,
  holding an `until` that has not passed.
- **An item has a route** where some selection its job may go out on is not blocked. A walk that could not
  be read is no route.

## What is running

A job is at work while its row is open and its unit still stands.

- **A unit stands** where the machine's systemd user manager holds it active, or an open execution of
  bmo-agency names it as its parent or root. An execution that shares its parent's pane, one of the
  coordinator line that has recorded its outcome, and one a cancel stands over hold nothing up.
- **"Nothing runs" and "nobody could say" are kept apart.** Where systemd cannot be asked, the jobs at
  work are not known, and nothing that reads them takes that for an empty machine.
- **A stopped job** whose item has left the line is no work going on, whatever its run is still doing on
  the way down. One stopped by its unit alone counts until its run is gone.
- **A worker has something at work** where a job of its own runs, a work order of its item is under way
  or stopped still holding its checkout, or a running job has borrowed its checkout.

## A worker's turn

Each worker has one line: the open items whose work is its, in the owner's order. An item's work is the
worker its row names, that worker's base where it is a support worker, and the built-in worker where it
names none or one since removed. An item whose work order is under way is out of the line while the
order runs; a stopped order keeps its item's place.

Whose turn it is in one line is one judgement, which the queue's telling and the work-order runner both
ask, so the agent is never told a worker is open while a stopped order is about to resume there. Down the
line, the first item that has a route is launched, unless a stopped work order stands before it:

- an order being closed is passed (`closing`);
- an order with no route open is passed (`no_route`);
- an order stopped with no other item's job launched on the worker since gives its turn to what stands
  behind it, once (`just_stopped`); with nothing behind it, it waits out the minute after its stop
  (`waiting_after_stop`);
- any other stopped order is resumed by its runner.

The orders passed are named with their reasons (`PassReason`).

## Launching a job

A launch is judged before anything is started; each refusal is a code of `TaskRefusal`.

- **Who launches.** A launch says who launched it: `chat:<chat>` from a chat session, or the launcher's
  own name. A run of an item is named by the item and takes no line of why; a job of no item launched by
  anything but a session says in one line why it was launched.
- **What a session launches** is a TODO item, which must exist, be open, not merged, not held, approved
  and the work of the worker asked.
- **The pacing of an item whose launches die.** Each launch of an item that ended `error` or `died`, back
  from its last, doubles the item's wait from one minute up to fifteen, counted from that run's end.
- **The checkouts of a repository.** A launch may take the base or any support worker within the count,
  tried in the order: a support worktree already on the item's branch `work/<item>`, the base, then the
  support workers by number. A checkout is held while a job of its own worker, of any worker pointed at
  the same directory, or a job that borrowed it runs there, and while a work order of another item is
  under way there or stopped holding it.
- **A co-work** runs where the item asks for it, the launch names it, or the job's text holds the
  co-work sentinel.

## How a job ends

A job's ending is one word of its record:

| Ending | Counts as |
|---|---|
| `done` | the work done |
| `work-step` | a work order's step done |
| `failed` | an attempt: the agent answered with nothing where a report was due |
| `live-tasks` | an attempt: the agent answered and left its own background work running |
| `unclosed` | an attempt: the agent answered while its own work was still open |
| `limit` | the usage limit was reached |
| `error` | an execution failure: the run never reached the work |
| `died` | an execution failure: the run went down with its row still open |
| `stopped` | a person stopped it |
| `unanswered` | the agent reported it could not answer everything |
| `blocked` | it stopped waiting for a person's answer |

- **An item stops being launched** after three attempts at its work that ended while the work was still
  open, or three execution failures in a row.
- **A stop asked of a job** is marked under `job_stop:<job>` before its unit is asked to stop, so whoever
  closes its row closes it as stopped, whatever that closer read of its last answer.
- **Where a person reads an ending** it is a `JobEnding` code, which the console and the room's receiver
  put into words.
- **The item an ending is told against** is read off the job's row: a stage of a work order is told with
  none, since the order counts its own stops, while the order's final turn, with no step added ahead of
  it, is the item's own job and carries it.
- **An item is set aside** with a hold of kind `failure` when its ending brings it to either ceiling, or
  when a chain of repairs of it has had three rounds and failed again, which holds the whole chain and is
  told to the agent once.

## Starting a job

A launch the judgement let go is started by the user's service manager as a unit named
`bmo-task-<millisecond>`, which runs the runner's `job-run` verb.

- **The checkout lock.** The unit is started in the first of its checkouts whose lock is free: a file
  under the host's lock directory named by the checkout's path, held by `flock`, so a lock of a process
  that died is no lock at all. A checkout before the last is given a second, the last the host's whole
  wait; a checkout a job borrowed meanwhile takes no job.
- **The row is opened under the lock,** then the unit started. Where systemd-run could not reach the
  manager the row is taken back and the launch refused `manager_unreachable`; where the unit did not
  start the row is closed as `error` and the launch refused `unit_not_started`.
- **The job's text** is written to a file only the service's user may read, and the unit is handed its
  path: arguments stand in the process list for anyone on the machine to read.
- **An item's brief** becomes the text its job was launched with.
- **Where the machine is short of room** bmo-agency says why; the queue writes it under
  `machine_launch_held` and tells bmo-agency. Once the room is back bmo-agency says so on its event
  stream, and the service forgets the hold and tells the queue again (docs/api.md, "The machine's
  room").

## Whose work a job is

An item's job is the platform's own work where the owner filed it, no linked user filed it, it was
approved on a standing rule, or bmo-web's account model is not in force. Otherwise it is the work of the
user bmo-web links the sender to, who must hold `run` on the worker and `change` on the repository, and a
Claude account of their own turned on, which bmo-agency answers; an item whose user lacks any of them
waits. The user rides the job's unit as `BMO_TURN_USER`.

## Stopping runs

A run is stopped by its name, a unit's or an execution's: bmo-agency is asked to cancel it and every
execution it covers, the runs are given a while to reach their own ending, and a unit systemd still holds
after that is stopped through `systemctl --user stop`. A change to a work order that ends runs asks every
unit first and confirms each gone before it closes a row; a unit still standing refuses the change
`runs_still_alive`, writing nothing. Once every unit is gone, each row they left is closed with the ending
named, each place they held in the co-worker pool finished, and each cancel dropped.

## A work order's verbs

A work order's body is the runner's JSON, kept whole in `work_orders`; each verb changes it member by
member inside the order's lock, so whatever the runner keeps that no verb names comes back as written.

- **The frame** — the plan, the synthesis and the final turn — is touched by no step verb.
- **A step is edited** while waiting, failed or skipped; **skipped** with a reason of 1 to 2000
  characters, a running branch coming back as the run to stop and a running builder turn refusing it;
  **run again** from done, failed or skipped, moving the research stage back to it where the order is in
  research; **restored** from a skip, done again where it holds a result; **moved** before or after
  another of its sort, a build task while both wait, a research stage while both stages are still ahead.
- **A step is added** to the build list under the next free build name, at its end or beside a waiting
  build task. At the final turn the order goes back to its build: at once where the final turn is not
  running, once it ends where it is.
- **A stop** ends every run of the order and stands it stopped, ready to resume where it was cut; **a
  close** skips every task not passed over and ends the order, or stops it closing where it holds its
  checkout; **a resume** puts its failed tasks back to waiting; **a reopen** moves every finished task
  into the history under its round and the order back to a draft; **a recovery** brings back a finished
  run a later run wrote over, from the job rows it left.
- Each refusal is a code of `WorkOrderRefusal`.

## A job's question

A job may put a question to the owner: options, the one it recommends, one line of why, and how long it
waits. The question is a row of `job_questions` opened before it is asked, so it is on record from the
moment it exists.

- **The first settlement stands.** The owner's answer, a choice or words, settles an open question; the
  wait running out settles it on the recommendation, but only once its due time has come.
- **The clock** may be stopped by the owner for `queue.question_stop_seconds` and started again counting
  what it had. A line of talk over a question stops its clock the same way and moves its end to a hold of
  that length from the line; the owner's lines wait until the asker hears them, the job's are heard as
  said. A line is written only on a question still open, and the owner's only on one that takes talk.
- **A room's open questions** are those nothing has settled whose wait has not run out, the one asked
  last first: the one asked last is the one the owner's next plain message in his chat answers.
- **The asker waits** by reading the row until something settles it or the owner talks, and may stop its
  own wait, settling the question with words of its own.
- **What the room is told** at each step is a `QuestionFact`.

## A run past its clock

A run that reaches its clock is not cut on the clock alone. The agent is asked in the owner's chat, by an
order, whether to keep it or cut it, and the ask is a row of `clock_holds`. The run goes on until the agent
answers; a keep holds for at most two hours and is asked again when it runs out; with no answer within
thirty minutes the service cuts the run itself and records that it did. An order the agent never took is
withdrawn once nothing is left to decide.

## Telling a room

- **A fact a room is told** goes through bmo-receiver as a voiced notice: the fact, what the line is for
  and what follows it, said by the room's voice in the room's own language.
- **What a run came to** is queued for the agent whose turn set the run going where it still answers the
  run's room, and for the room's voice otherwise. A room whose registration is gone hands it to its work
  chat instead, saying so, and its road and kind are not carried.
- **A job's report** is the worker's English to the agent. The room that asked for the work, where it is
  not the owner's chat, hears one fact of what it came to: whether it finished, and only the sentences of
  the report's first block that say nothing of the system — no commit, test, worker, model, provider,
  path, command or deployment, nothing written as code, and none of the deployment's own names. An item a
  companion filed is told by the companion in its own room.

## Telling the agent

What the agent is told about the queue is a `QueueFact` in the owner's chat; none names an item to
launch.

- **The standing** is told at a moment that moved the queue: the workers with a free checkout and work of
  their own that can launch now. Nothing is told while the queue is held, while what is running cannot be
  read, or while the machine is short of room; a standing already told is not told again unless the
  moment repeats. A worker standing open for five minutes with no job started is told once as stalled.
- **A job's finish** carries how it ended, what it means for its item, the repair chain it stands in, the
  commits it left waiting to go live, what holds the queue, and the report where it was delivered.

## The sweep

- **A dead runner.** A job whose unit is gone five minutes after its launch, with its row still open and
  no work-order runner collecting it, is closed as `died`; the room that asked is told it is gone, and the
  run is handed to the agent as a broken one. A probe nobody could answer closes nothing.
- **What runners left waiting** — a report or an ending no service took — is taken, a report before its
  ending, each once.
- **A circling job,** started over more than twelve times, is told once.
- **A quiet job,** five minutes with nothing written, is told once for that stillness. A job that marked a
  long wait under `job_wait:` is judged by the mark first: while its target, a unit or a process, lives
  and its end has not passed, it is not quiet. A stage of a work order and a job asked to stop are not
  told.

## A broken run

A job that ended `error`, `failed` or `died` is handed to the agent in the owner's chat as an order to
find the cause: what broke, when, how it ended, what came back, its last tool call, where its record is
and the journal window of the service's unit and the job's. One cause is one order for six hours: the
same cause again is counted into it, and an order not yet taken is replaced by one carrying the count.
Where the agent cannot answer — nothing of its own answered within five minutes, or the owner's own chat
had a run die since — the owner is told the `job_broke` fact the order keeps, once.

## The work-order runner

The `order_run` module carries every active or stopped work order on its own clock. A tick never runs
beside another; each order is taken in its lock (docs/rows.md, "A work order's lock") in the owner's
order of their items, read again inside it, and saved before the lock is let go.

- **An order whose item is finished or merged** is closed once nothing of it runs.
- **A stopped order** that holds no checkout is resumed at its turn (below); one that holds its checkout
  lets it go first.
- **A running order's stage** is collected: a task whose job ended done or as a work step has its report
  written as its result document, and one that failed, reported nothing or whose unit is gone a minute
  after its start fails. A running task is watched for a runaway.
- **The phase moves on** once every task of the stage it stands at is settled: the plan's answer read
  into stages, each stage in turn, the synthesis's answer read into build tasks (with the steps added
  meanwhile after them), the final turn once everything is built, back to the build where the final turn
  added steps, and done once the final turn has closed its item. A final turn that held its item stops
  the order uncounted; one that did neither is refused, and the order stops.
- **A failed stage** stops the order once nothing of the stage runs: its waiting tasks fail with the
  reason, the stop is counted, the agent is told, and its branch places are retired.

### Launching a step

- **Its tier** is settled once and kept on the task: the tier it names, the one its model names where its
  role takes that tier, and its role's default otherwise.
- **Its place is asked of the co-worker pool** through bmo-agency: a branch's seat as a child of its
  order, a builder turn's place as the worker's own on its checkout, under a key naming the order, the
  task and the generation it asks at, so a tick that died between asking and keeping the token asks
  again for the same request. The task waits while the pool lets others in first.
- **Its prompt** is the step's context (below); a step whose texts could not be put into English waits
  for the next tick rather than failing.
- **Its unit** is `bmo-work-<item>-<task>-<attempt>`, started through the host's systemd-run as the
  runner's `job-run` verb with the step's phase, session word, structure and baseline. The cancels left
  standing on the name, or on the root its retries share, are dropped first. The prompt and the baseline
  are written under the order's own folder of `[host] documents`, readable by the service's user alone.
- **A branch runs in a branch place** of its repository's pool (below); a builder turn in the worker's
  own checkout.

### What a step is handed

- **A branch** reads its task, the results before it cut to six thousand characters (two thousand for an
  earlier round's), the steps decided against, and the branch rules and result shape of work-turns.
- **A builder turn** reads the item's title, the order's purpose and instruction and every earlier
  round's, put into English through bmo-gate; where it stands; what is done, skipped and left; and every
  earlier result wrapped with the key it is kept under and a digest of its text, so the runner sends it
  whole only to a session that has not read that version. The branches of a round are joined into one
  document. A plan turn is also told the tiers and the roles a plan may name, from bmo-agency's pool.
- **The order's own words** are its purpose and instruction alone: the item's conversation, the words
  people append and the approval note are never carried into it.

### Branch places

A branch works in a standing place of its repository's pool, which bmo-agency keeps under its branch
slots: a free place whose row in `branch_agents` still names a cache for its provider and model, the one
whose answer is oldest first; then a free place with nothing to lose; then a new number. A row is alive
while the provider's published lifetime, which bmo-agency answers, has not run out since the place last
answered, and for Claude Code while the prefix the place would open on now, which bmo-agency answers too,
is the one the row names. A place is busy while a task holding it runs. Places past eight, and places
rested past a week with no live row, are taken away; pools no worker's checkout names any more go too. A
place is laid out of its checkout before a branch starts: rewound to the checkout's commit, the
checkout's open change applied on top and its new files copied in. What stands for a place in bmo-agency
is retired once the order that used it ends.

### A checkout held

One work order works in a worker's checkout at a time, and only once what ran there before has left it:
an open job of the worker, a unit of its jobs ended in the last fifteen minutes still alive, or the
checkout's job lock held. What the order waits on is kept on it as a `ReleaseWait`.

- **Taking it.** A support worker's worktree, a worker of its own in a linked worktree, and a base beside
  support workers are put on the item's branch, the worktree made where it is not there yet; a change the
  order parked on a branch when it stopped is merged back uncommitted, conflicts named for the next
  builder turn. The tree as it stood just before is the order's baseline.
- **Letting it go** as the order stops: once none of its own turns is alive and the checkout is quiet,
  the change its own turns wrote — told by the files' times against the turns' windows — is committed
  onto the checkout's branch and pushed where the repository's own check, run through bmo-testbed, passes
  with it, and onto `wip/<item>` where it does not or keeps not finishing. A support worker's worktree
  parks everything open onto the item's branch; a base checkout goes back on main, its units relinked
  through bmo-testbed. What became of the change is kept on the order as `CheckoutNote`s.
- **As the order ends,** a base checkout goes back on main the same way.

### Stops and retries

- **A runaway.** A running stage started over more than the policy's most, or running past its clock in
  a branch, is a runaway: a branch is stopped, since its place is thrown away, and a builder turn is told
  of once. A stop that did not take is asked again after a minute; the stage fails once its unit is gone.
- **A stop is counted** unless it stopped on its provider — a limit, a failure of the road, or every
  route exhausted. A counted stop is retried at the order's turn, again, then on a fresh session, then on
  the retry model (the setting `work_retry_model`, else the policy's); the fourth sets the item aside
  with a hold of kind `failure`.
- **The order's turn** in its worker's line is the queue's one judgement (above); where it stands is kept
  on the order as a `LinePlace`. It resumes only where its item may still run, the queue is not held,
  nothing of it runs and its checkout is quiet.
- **A stage that stopped on its provider** goes out again on a provider it has not tried, then on one
  whose block has lapsed, resolved for its role and tier through bmo-agency.
- **The agent is told** each stop, runaway, set-aside, resume and move as a `WorkOrderStop` fact in the
  owner's chat; the queue's standing is told when an order stops or frees its worker, and the screens
  hear every change of an order's body.

### Seats for the next stage

While a stage runs, the seats the next stage's branches will take are reserved in the pool, one
reservation to a provider and tier, keyed by the order and the stage. A reservation is kept only while
the stage it was made for can still use it; a stopped or finished order, and any order while the queue is
held, holds none, and the reservations of an order no longer carried are given back. A place a task took
is given back with what it came to once its task no longer runs and its unit is gone; one waiting for a
task that no longer waits is given up.
