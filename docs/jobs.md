# Jobs

Long work launched apart from the chat: how a job runs, what it is told and what it reports, how it
ends, fails and finishes, where it is shown and the time it is measured by, the questions it puts to
the owner, and what the queue tells the agent about it. The program a job's unit runs is
docs/runs.md; the queue that launches it is docs/queue.md; a work order's steps are docs/work-orders.md;
a repository's support workers and their worktrees are docs/worker-slots.md.

## How a job runs

- **A job is a unit of its own.** Work that would keep a chat waiting is launched as a job: the queue's
  launch opens its row and starts a transient user unit, `bmo-task-<millisecond>`, which runs this
  service's `job-run` verb (docs/queue.md, "Starting a job"). A restart of the service does not reach
  it, and the job keeps the program it was started on to its end.
- **Every turn is bmo-agency's to carry.** The runner prepares each turn on bmo-agency as a work turn,
  sends it its prompt and reads it until it answers (docs/api.md, "Work turns"). Where the turn runs —
  the worker's own program, a branch's place, a session on a node — and how it is kept alive is
  bmo-agency's; what the turn says and what is made of its answer are the runner's.
- **The model** is the one the job asked for, or the walk over the providers its worker's route takes
  (docs/queue.md, "Routes"). A job may name another model with `--model`.
- **A job reports to the agent, never to a room.** Its report is handed to the agent in the owner's
  chat as a fact of the job's ending; the agent tells the owner, and a room that asked for the work
  hears one line of what it came to (docs/queue.md, "Telling a room").

## Co-work

- **A job runs as one session unless co-work is turned on for it.** Off is every worker's default, and
  three things turn it on: the item's own setting, a launch that names co-work, and the co-work
  sentinel in the job's own text. Only the job's own text is read for the sentinel, never the prompt
  the worker is finally handed, whose trailer speaks of co-work too. The launch is the one place the
  three are read. A job that fans out becomes a work order before any runner starts
  (docs/work-orders.md).
- **The setting is weighed on impact grounds, and a job weighs them again.** The agent files an item
  that changes code with the graph nodes the change reaches and the outward contract that changes on
  each, which come to a single job or a co-work by one rule (docs/todo.md, "Impact grounds"). A job of
  the item weighs what the change reaches again before its first edit and records its own grounds;
  the agent is told where the job's verdict differs from the setting, or where it filed none.
- **Only the agent turns the setting, and a running job is not moved by it.** The launch reads the
  setting once; a changed setting reaches the item's next run.

## What the worker is given

- **The worker is a code-editing agent,** not the agent of a room. What it is told of how to work is
  its agent definition's, which bmo-agency keeps: the standing rules, the closing skill and the
  trailer the job's text is followed by (docs/runs.md, "The prompt"). The checkout's own `CLAUDE.md`
  is read by the program as any session there would read it.
- **The prompt** is the worker's preamble, the item's impact grounds, the job's own text through
  bmo-gate's sanitiser, the knowledge bmo-gate picks for the job's words, and the trailer
  (docs/runs.md, "The prompt").
- **A worker reads and reports in English** (docs/worker-language.md).

## The report

- **A report has three parts:** a summary paragraph, the detail in a paragraph a part, each under a
  short name line, an Impact part among them where the change reaches the graph, and a closing line of
  figures — what is done, what is left, what passed. A report is never held back for its shape: the
  runner names under it what the shape got wrong, what was left uncommitted, and every line that leaves
  a check undone with nothing putting it beyond a job's reach (docs/runs.md, "The judgement").
- **The agent tells a report only once nothing in it is left unchecked:** a line the runner named is
  settled first, by the agent's own reading or by a row raised to check it.
- **The whole answer is the report the agent reads,** opening on its summary.

## Checking and finishing

- **A job does its own checking** and settles what it does not know before it reports: a value is read,
  a path run for real, a capability in doubt looked up in its maker's own material and tried. The one
  thing that excuses a check left undone is what no job can reach now — a permission only the owner
  holds, hardware not yet there, a judgement of the owner's own taste — and the report names it with
  its reason. The closing refuses a line that leaves a check undone and names no such reason
  (docs/rows.md, "The closing's readings").
- **A job commits, closes once and never deploys** ("A job closes" below).

## A job closes

- **A job commits and closes; it never deploys.** It runs the static check and the tests its own
  change touched as it works, and closes once at its end with `job close --then '<why this goes live>'`
  (docs/verbs.md, "A job's close"). Every deployment is bmo-testbed's, one repository at a time; there
  is no batch of commits put live together.
- **The suite is not owed.** A change that reaches nothing the suite runs — a document no test reads,
  a file no test loads — may skip it with `--skip-suite '<why>'`, and the job writes the same reason in
  one line of its report.
- **The close reads its language first.** Before anything is checked it refuses a reason or a commit of
  the job's own whose words are not English, past the names of address the close's policy sets aside
  (docs/policy.md, `close.toml`), naming each commit by its short hash and subject. The landing refuses
  such a commit again as its last word (docs/runs.md, "Landing on main").
- **One close of a repository checks and lands at a time on the machine.** The close stands in the
  repository's close line through bmo-testbed, which runs its checks in their turn; inside the line it
  rebases a job's branch onto main, runs the change check, the test count and the suite, with what
  failed or never started run once more, lands the branch, and records the commit as waiting to go
  live through bmo-testbed. A suite that failed records nothing, so nothing the suite did not pass
  ever waits to go live; a test that passed only on its second run is named in the close's last line,
  for the agent to raise an item that mends it.
- **A close that waited out its job ends as not run** (exit 75) and records nothing; the job runs it
  again.
- **The job's end carries what it left waiting** to the agent, so the agent knows what bmo-testbed has
  to put live.

## The agent decides, the service says what happened

- **The service says what happened, and the agent decides what happens next.** A job's ending is told
  to the agent as one fact: which worker, which job, which item, how it ended and what that means for
  the item — nothing about what to do (docs/queue.md, "Telling the agent"). The same holds for the
  queue's standing at a moment that moved it: the agent is told which workers stand with a free
  checkout and work of their own waiting, never which item to launch.
- **One thing is never left to judgement:** a second job in a checkout already at work is refused where
  it is launched, and the refusal names the job holding it and that job's item.

## Pausing the machine

- **The owner can stop the machine moving.** The queue's pause is one switch for the whole machine,
  thrown from the console or with `todo --pause` and `--resume`. While it is on, the agent is told of no
  waiting queue and nothing is launched: the queue, the approvals and the order stand as they are, a
  job already running runs to its end, and a work order finishes the stage it is in and waits at the
  boundary.
- **A deployment's hold** holds the queue the same way while bmo-testbed puts something live that
  running jobs could not survive: it names what it waits for and until when, is never let go by the
  owner's resume, and is let go by hand only through its own release (docs/api.md, "The queue's
  holds").
- **Both are rows of the service's settings,** so a restart finds them as they were left, and lifting
  either tells the agent how the queue stands at once.

## A failed job

- **A job that failed is mended before anything moves on.** The agent works out the cause, files the
  repair linked to the item it repairs, runs the repair and then the failed item again. A repair of a
  repair links to the repair, so the chain goes back to the item that first failed; a chain that has had
  three rounds and fails again holds the item and its open repairs and tells the agent once.
- **What counts against an item is an ending the agent itself arrived at:** it answered with nothing,
  it answered while leaving its own work running, or it answered while its own change was still open.
  Three of them set the item aside with a hold of kind `failure` (docs/queue.md, "How a job ends").
- **The count is read off the item's own state.** A run that ended after the item's work was already
  closed tried nothing, so an ending counts only where the item was still open when the run ended.
- **Answering with nothing is not an empty answer alone.** A run that put down nothing but a room's
  silence marker has reported no work at all, and is refused as an empty reply is, its report naming
  what it actually answered with.
- **A run that never reached the work is counted apart:** an `error` or a runner that `died` buys the
  item a widening pause before its next launch — a minute, doubling with each failure in a row, up to a
  quarter of an hour — and three in a row set it aside too. A usage limit is in neither count. Nothing
  of this is stored: the item's job rows say when each run ended and how.
- **A broken run is handed to the agent as an order** to find the cause (docs/queue.md, "A broken run").

## Marking an item done

- **Only work that is finished and whose tests passed marks an item done,** and marking is not deleting:
  the row stays as the record of what was done. The job that did the work closes or holds its own item
  (docs/todo.md, "Closing").
- **What a job left uncommitted is named under its report,** a file git never saw included, so a job
  that skipped the finish cannot pass quietly; what was open before the job began is named apart as
  somebody else's work, since a checkout is shared (docs/runs.md, "The judgement").

## A job is not finished while its own work is open

- **Four facts of the process say a job's work is still open,** none of which the words of its report
  can reason away: a background task its run launched and never heard the end of; a Claude Workflow
  that did not report it completed, silence included; a change of its own still uncommitted in a file
  git tracks; and a commit to code the dependency graph names with nothing said about the graph in the
  job's own commits. What is the job's own is measured against where the checkout stood when the run
  began, file by file in changed lines, so another job's open work refuses nothing. An untracked file
  alone refuses nothing and is named.
- **The session is asked once before it is refused** for a task left running, a change left open, a
  graph left behind, a change that reaches the graph unanswered for, and an item left open: the runner
  names what it found, tells the session its answer was not sent, and judges what the session does
  then (docs/runs.md, "The turns"). A workflow walked away from is asked nothing, since that work died
  with the run.
- **Such a job is recorded as a failure,** its report saying which of the causes fired with the run's
  own words under it. The shape of a report, a word the owner set aside, an item left open and a graph
  reach left unanswered after the asking refuse nothing and are named under the report.

## A job's run row and its launcher

- **Every job is a row of `jobs`:** the item it is a run of, the prompt and the model it was launched
  on, the English the worker read of its own text, its report or failure, and how it ended. A room's
  jobs are read back newest first with `room jobs` (docs/reading.md, "A room's jobs").
- **The row records who set the job off** as it is opened: `chat:<chat>` for a chat session's launch,
  or the launcher's own name, the console's among them. A run belongs to the room it reports to and
  names the one that started it, and the two are the same only where a room's own agent launched it.
- **The jobs at work are shown where someone looks for them,** never pushed into a room: the console's
  screens and `room jobs` read the rows. No room is sent a message listing them.

## Where a job's time went

- **It is worked out as the job ends.** Its wall time is the row itself; how that time split is rows of
  `job_spans` beside it: the phases, cut at the row's own two ends so they cover exactly the stretch the
  total counts — the wait for the checkout's lock, getting ready, the run, and finishing — and inside the
  run each tool by name with what its calls took and how many there were, and each subagent branch. The
  inner spans are no partition and are never added to the phases: calls side by side each count their
  own seconds.
- **The spans come from the turns as bmo-agency reports them,** and are stored at the end, so an item read
  long after has the breakdown with nothing left to work one out from. A row closed by the sweep for a
  runner that died writes none.
- **What each model spent is written the same way,** one span a model with the four kinds of tokens apart.
- **An item's time is every job that carried it added up,** a job still running counting as nothing, and
  the run is divided three ways where the spans allow: the wait on its branches, its own tools, and the
  rest, the model's (docs/reading.md, "The time an item took").
- **What a job cost** is read a job at a time with `job figures`, `job tool-calls`, `job branches` and
  `job cowork-timing` (docs/verbs.md, "What a job cost").

## The item is the only number

- **The item is the only number a person is shown.** A job is a run of an item, so every notice, report
  and screen names the item; a run still going says how far its work has come — a step of a work order
  by the kind of its step (docs/work-orders.md, "What a step is called") — rather than which run it is.
  A record of one run, such as a notice that a job died, still says which run of the item it was. The
  job's row id stays in the table as the key, and is never put in front of a person. A job launched with
  no item is named by the line its launcher wrote for why it was launched (`JobLabel`, docs/api.md).

## One job per checkout

- **Jobs run one at a time in one checkout:** the runner takes the checkout's job lock and the job hold
  before anything else and holds both until the job is recorded (docs/runs.md, "Locks"); jobs in
  different checkouts share nothing and run side by side.
- **A launch into a checkout a job already has is refused where it stands,** before any row is opened or
  any unit started, the same way one into a checkout a work order holds is. It gives the lock a moment
  first, since a job hands the queue on while its runner is still letting go. A second job is never
  left to wait its turn on the lock: a row standing open for work that has not begun reads everywhere
  as a job at work.

## Launch rules

- **A launch is decided in one place:** the server's `POST /task`, which `job start`, the agent's launch
  tool and the console all ask (docs/api.md, "Jobs"). While the server does not answer, nothing is
  launched.
- **The rules** are docs/queue.md's, "Launching a job": who launched it, an item open, approved, not
  held, not merged and its worker's own, the pacing of an item whose launches die, and the checkout it
  takes. Each refusal is a `TaskRefusal` code.
- **The judgement is one worker's.** What another worker runs in another checkout is no part of it.
- **Which worker the launch runs as** is the item's own assignment where the launch names none.
- **A launcher that is not a session says who it is,** and where its launch has no item, one line of why
  it is made; a launch with no item and no line is refused.

## The worker's session

- **The worker's sessions are bmo-agency's.** Which session a turn continues, the login it goes out on,
  the record kept per model and login, a full session's fold, keeping prefixes warm and a session that
  fell to a lesser model are bmo-agency's to keep (docs/api.md, "Workers' runtime").
- **What the runner asks of them:** a reset the owner asked carried out before the first turn; the
  session the job's last answer came back on recorded, which bmo-agency keeps, keeps as full, or drops
  where it answered on a lesser model; and, once the job's report has gone out, that session handed
  over with the worker's own note, or a summary bmo-agency writes, for the next job of the same item.
  A session bmo-agency dropped is never handed over (docs/runs.md, "The ending").
- **A note belongs to its item:** a handover names the item's thread, so the next job of the same item
  opens on it, and a job of another item opens on no note of it.
- **The middle turns of a work order are no job's end** and hand nothing over; the handover comes after
  the final turn. A stopped stage's retry on a fresh session hands the kept session over first.

## A job's question to the owner

- **A job settles its own work.** The one thing it does not settle is a choice that is really the owner's
  — hard to undo, or one only his taste decides — which it puts to him with `job ask` (docs/verbs.md, "A
  job's question to the owner"). The job's own run is what waits, so its checkout stays held meanwhile.
- **The question goes to the owner's private chat** as a short numbered list with the recommendation
  marked, one line of why it is asked and what happens if nobody answers. A question naming another room
  is refused.
- **The job writes the question in the owner's language itself,** and nothing renders it on the way. A
  term the owner set aside, and a page hosted outside the deployment's own archive, are refused before
  the row is written.
- **He answers with a number, a word naming exactly one option, or words of his own,** from his chat or
  the console. Recording a choice is no answer from a model: it costs no run and cannot be paraphrased.
- **A minute with no answer settles it on the recommendation,** and the chat is told what was taken. The
  owner can stop the clock for at most `queue.question_stop_seconds` and start it again counting down
  what was left. The chat, the console and the deadline settle the same row with one conditional write,
  so the first to land stands (docs/queue.md, "A job's question").
- **Every question is a row of `job_questions`,** never deleted: which job and item asked, the options,
  the recommendation, the why, when it was asked and due, and what it was settled on, when and by whom.

## Talking a job's question over

- **While a question is open, what the owner says in his chat that names no option is talk about it,**
  and the job that asked answers it, since the job holds what the options were written out of. The talk
  stops the clock, the waiting `job ask` hands the job his words in place of an answer, and the job
  answers with `job ask --talk`, optionally putting the question again with new options.
- **No deadline runs while they talk:** every line holds the clock stopped for the same stop length,
  counted again from that line; a talk left silent that long settles on the recommendation.
- **He ends it by choosing,** recorded as any answer. Every line is a row of its own beside its
  question, with whether the job has read his yet.

## A confirmation a job's turn stops on

- **The program asks a person before a few things even in bypass mode,** and a job's turn has nobody to
  answer. bmo-agency reports the question the turn stands on; the runner answers it itself where it is
  a command that stays inside the job's own places (docs/runs.md, "A command the runner allows
  itself"), and puts any other to the owner as a job question of its own — allow or refuse, with the
  same minute and the same stop of the clock — answering the turn with what he chose. The recommendation
  is to allow, so an unanswered question lets the work go on, except for a command that destroys what
  nothing here can restore, where it is to refuse.

## A job's own mark on a long wait

- **A job about to stand still on purpose says so first,** so the quiet watch reads what it said instead
  of asking the agent to go and look. `job wait` marks what the job waits on in its own words, the one
  thing whose life says the wait goes on — a user unit by name or a process by its id — and when the
  job expects it to be over (docs/verbs.md, "A job's mark on a long wait"). A command handed after `--`
  runs under a mark that clears itself the moment the command ends.
- **The mark belongs to the running job of one item,** one a job, kept as the `job_wait:` setting of the
  job.
- **The code believes the target, never the job.** Once a minute the sweep asks each mark's target: a
  unit is alive while systemd reads it active, activating, deactivating or reloading, a process while it
  exists. While the target lives and the end has not passed, the job is not told of as quiet; once the
  target is gone, the job's quiet is counted from the later of its last line and that moment; past the
  end with the target still alive, the agent is told once what the job waited on and that its time has
  passed.

## A clock asks before it cuts

- **A run of work that reaches its clock is not cut on the clock alone.** A job's turn carries no clock;
  the runs that do — a browse among them — ask the agent before they are cut (docs/queue.md, "A run past
  its clock").
- **The ask is an order queued for the agent in the owner's chat,** carrying which run it is and what it
  is doing, how long it has run, when it last did anything and what, and its clock. The agent decides
  alone, keep or cut, with `overtime` (docs/verbs.md, "A run past its clock, answered"); a keep names how
  long, the run's own clock where it names none and two hours at most, and the run is asked about again
  when it runs out.
- **Nothing is cut before an answer** for thirty minutes; with none by then the service cuts the run,
  recorded as its own call for want of an answer, since a queue held behind a run nobody decides on costs
  more than the cut. A run that ends on its own while asked closes its ask, and an order not yet taken is
  withdrawn.
- **Every ask is a row of `clock_holds`:** the run, the clock, the facts the order carried, the verdict,
  who gave it and why, and when.

## What the sweep tells about work that is open

The sweep, once a minute, tells the agent facts about the work it walks, none of them an order; what to
do about any of them is the agent's (docs/queue.md, "The sweep").

- **A dead runner.** A job whose unit is gone five minutes after its launch, with its row still open and
  no work-order runner collecting it, is closed as `died`; the room it reports to is told it is gone, and
  the run is handed to the agent as a broken one. A probe nobody could answer closes nothing. A dead
  runner is no attempt at the item, but it is one of the failed launches that pace the item and, three in
  a row, set it aside.
- **A job circling** — started over more than twelve times — is told once.
- **A job gone quiet** — five minutes with nothing written to its log — is told once for that stillness,
  carrying what its last line was, so a job waiting on a long tool call reads apart from one that stopped
  in its own turn. A job asked to stop is winding down and is not watched; a job with a wait mark is
  judged by the mark first; a stage of a work order is the work-order runner's to watch.
- **What runners left waiting** — a report or an ending no service took — is delivered, each once.

## A worker standing with nothing to do

- **A worker standing open with work of its own waiting** is told to the agent as the queue's standing at
  the moment it opened, and once more as stalled where it stands that way for five minutes with no job
  started (docs/queue.md, "Telling the agent"). The owner's pause and a machine short of room are no
  stall and are never counted as one.
- **The program a worker stands with when nothing runs,** and a pinned worker's program put back after a
  pin moved, are bmo-agency's: a worker's place and what stands in it are its own.

## A command the runner allows itself

- **Most confirmations a job's turn raises are its own ordinary work** in a shape the program's check
  gives up on. The runner reads such a command the way the shell splits it and allows it itself where
  everything it names stays inside the job's own places, removes nothing but what lies strictly inside
  them, and runs nothing as somebody else (docs/runs.md, "A command the runner allows itself"). The
  run's log says the runner allowed it; no question row is opened and no room is told.

## The job queue's routes

- **The queue's routes are this service's own,** served by `bmo-job serve` (docs/api.md, "Routes"): the
  TODO's actions, a job's question answered and its clock, the queue's pause and a deployment's hold, a
  job's end and the report it left waiting, the queue told at a moment a verb saw move it, a runner
  started, a job launched, a work order changed, a run stopped, a word to a worker and a worker added,
  changed or removed. What the service decides there is the checks of the request, the rows read and
  written, and every refusal with its status and its code.
- **The queue's telling is this service's own** (docs/queue.md): a job's end with what it means for the
  item, a report delivered on its roads, the queue's standing at a moment that moved it, the sweep on its
  own clock, and an item's stop. Each fact goes to the agent through bmo-receiver.
- **What is running is this service's own reading** (docs/queue.md, "What is running"): a name is alive
  where the machine's systemd holds its unit or an open execution bmo-agency answers names it, and a
  systemd that gave no answer is never read as one with nothing running.
- **What a job's end leaves standing** — the servers its turns left listening — is swept through
  bmo-agency as the job is handed off.
