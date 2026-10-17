# Work orders

A co-work item's work, carried by the service as a work order: what an order is and how it closes, the
rule of a co-work, the plan and its roles, the scope counted before the build, the build and the final
turn, how a stage fails and is retried, stops, holds and wind-downs, one checkout to one order, reopening,
the verbs that change an order, the model a step ran on, and what a step is called. How the runner carries
an order tick by tick — launching, collecting, branch places, holding a checkout, seats in the pool — is
docs/queue.md, "The work-order runner"; the body's verbs are docs/queue.md, "A work order's verbs".

## What a work order is

- **The TODO keeps** the conversation and the agent's conclusion; a work order of the item is a body of
  its own in `work_orders`, holding its intent, its instruction and its execution state.
- **A launched order's instruction is the item's brief.** The prompt the agent launches a co-work with is
  written as the order's instruction and kept as the item's brief (docs/todo.md, "Added words"), and a
  rewrite of either — the brief verb, or an instruction given on a reopen — writes both, so a later step,
  a rerun and a resumed order read the latest version.
- **A step reads the organized instruction alone.** The item's copied conversation, the words a person or
  a companion adds and the owner's approval note stay for the agent and the console; they reach a step
  only as the agent organized them into the brief.
- **The draft** is filled in the item's own words when the item is written, and can be read and edited
  before it runs. While its intent and instruction still say exactly what the item says, a reading does
  not show them twice; once someone writes one apart, it stands.
- **An order stands only while its item does.** Every door to one begins by reading the item, so throwing
  an item out takes its order with it, and every verb refuses an order whose item cannot be read.
- **An order closes with its item.** Once its item is finished or merged into another, the order has
  nothing left to do, whichever road closed the item. The runner closes it at its next tick once nothing
  of it runs: every task not yet passed over is skipped with the reason, a turn still running is let
  finish first, and an order still holding its checkout lets it go the way a stopped order does. Until
  then the order is closing, with no stop reason, no queue place and no resume. A draft whose item closed
  without the order ever running stays a draft.

## The service runs a co-work item

- **Whether an item is one is weighed on its impact grounds** (docs/todo.md, "Impact grounds"); the plan
  turn weighs them again before it designs anything.
- **The builder is the job worker itself** that took the item: every builder turn — plan, synthesis,
  each build, final — runs under that worker's name, in that worker's checkout, on the session bmo-agency
  keeps for it, so a co-work's builder turns are continuing turns of one conversation as long as they run
  on one model.
- **The plan is two turns.** The builder first designs the stages, roles, models, task titles and
  instructions in its own words; the runner then asks that same session for the design in the fixed shape
  and reads it (「The two turns of plan and synthesis」).
- **The other roles of a stage run as branches,** each its own run, and the order moves on once all of a
  stage succeed.
- **A builder stage holds one task.** A plan that puts a builder task beside other roles in one stage, or
  two builder tasks in one, is not refused: the runner splits that stage into consecutive stages as it
  reads the plan — the other roles first, in one stage, then each builder task in a stage of its own.
  Tasks of one stage never depend on each other, so the split keeps every dependency.
- **A builder task in a stage runs at the stage builders' tier,** the policy's `queue.stage_builder_tier`,
  rather than the builder role's: a stage comes before the build, and what a builder decides there is
  checked by a cheaper role. A tier the plan names for it stands. The plan, the synthesis, every build task
  and the final turn keep the builder role's tier. A turn on another model than the worker's session was
  opened on starts a fresh session with the kept one's note ahead of it.
- **A builder turn too takes its place in bmo-agency's co-worker pool,** as the worker's own turn rather
  than as a branch (docs/queue.md, "Launching a step").

## When the queue is paused

- **The next stage is not launched.** A running stage runs to its end, its result is saved and the phase
  moves on, but nothing new launches until the pause is lifted. A stopped order does not resume itself, is
  not moved to another provider, and reserves no seats for the next stage; seats already reserved are given
  back and waiting requests cancelled, and they are asked again once the pause is lifted. Branches already
  running and their places are not touched.
- **A resume asked from the console or the command line while the pause stands** does not launch the next
  stage either, and the answer says so.
- **The pause is a row of the settings,** so a restart finds it; lifting it carries every order on at the
  next tick.

## The rule of a co-work

- **Branches do the branch work, and the worker's own session takes over the result.**
- **Every role but the builder is a branch,** given a name that lives for that one task, reading its
  instruction and the earlier stages' results, keeping no session. The task declares the branch: no
  worker's row is written for it, and it is handed no bench.
- **The name lives one task; the place does not.** A branch borrows one of its repository's standing
  branch places, which a later branch reuses, preferring a place whose provider still holds the prefix
  (docs/queue.md, "Branch places").
- **No branch writes in the worker's checkout.** A branch's report is saved as a result document under the
  item's folder of `[host] documents`, which the next builder turn reads to decide what follows.

## The roles a plan may use

- **The roles are bmo-agency's pool roles,** read once as the roster of a judgement (docs/api.md, "The
  co-worker pool"): each role's name, its tier, whether it is enabled and whether the pool lends it.
- **`builder`** is the worker's own turn: an enabled role the pool never lends.
- **Every other role a plan names must be enabled and lent by the pool,** so a role that shows in a plan
  but cannot launch cannot be named. The plan's shape request names its roles from the same roster the
  check of its answer reads, so there is no second list.
- **The plan turn is told the roster is the live list:** a model an earlier plan holds is no value to
  copy, and a task's instruction names no provider model or effort, since both are settled when an
  attempt starts.

## What is passed to the plan step

- **The roles are a menu, not a checklist.** The plan uses only the roles the item needs, and as few stages
  as it needs; a plan with no branch, going straight to the build list, is a valid plan.
- **The branches are for knowledge** the builder needs without asking — every place to change, every side
  effect and every companion change — so the change is not a thin one. A stage that adds none is not
  planned.
- **The default shape** is one wide stage of parallel branches that finds the places, the side effects and
  the companion changes, then the build list; a check stage follows only where the plan itself is uncertain.
- **Builder turns run in series and are most of a co-work's time,** so a builder stage inside the plan is for
  a decision later branches must check before the build, never for implementing; no stage re-reviews a
  builder stage by default; and no task pushes, runs the whole suite or closes the item but the final turn,
  once. A build task commits its own step and a branch commits nothing. No turn deploys.
- **Overlapping roles:** verification by several votes runs several screeners in one stage and raises only
  split or all-passed items to a verifier in the next; a digest merges branches only where their results
  overlap or run long, since the runner already joins every stage's results into one document.
- **A middle turn checks narrowly:** the static check and the tests its own change touched, never the whole
  suite.

## The scope is counted before the build

- **Why it is counted first.** A build list cut from part of a set grows while it is built, each member
  found late a step added mid-build; every such member could have been read before the build began.
- **An enumerable set** is any set the work covers member by member whose members a source can list before
  anything is built: node types, screens, fields, commands, case kinds, files, call sites, keys.
- **The plan names every such set the item depends on,** and gives each a task that exhausts it from its
  source of truth: the whole list, its count, and the source and how it was read. A set too large for one
  task is split by source across the tasks of one stage, never sampled; an estimate the plan holds is what
  the branch checks, never the scope.
- **A branch enumerates to the end.** A set its task names is listed whole under `SETTLED:` with its count
  and source; a further set it meets that the build depends on is enumerated the same way or stands under
  `OPEN:` with what remains; a sample or "and others" never stands under `SETTLED:`.
- **The synthesis turn settles the scope before it writes the list,** finishing from the named source any set
  left open or partial, and cuts the list by member: each build step names the members it covers, every
  member lands in exactly one step, and no step covers "the rest" of a set.
- **A member found during the build is still built in this order,** as a step added with the add verb, its
  instruction naming the set it was missing from and why the enumeration missed it.

## From investigation to build

- **Merging is the runner's.** When the investigation stages end, the runner joins the round's branch results
  in stage order into one document, `branch-results.md` (with the round's prefix past the first), each piece
  under the title the plan gave its task; the synthesis turn reads it once and cuts the build list.
- **Every implementation stage is a build task.** Where the plan or a document the branches settled lays the
  work out in stages, each stage still to be built is one task of the list, in that order; none is left for a
  later run, a job of its own or a deployment to come first.
- **A build task commits its own step** once the static check and its touched tests pass, so each stage of
  the work is a commit of its own. The whole suite runs once, at the final turn's close.
- **No wait stands between two steps,** neither for a deployment, a resume nor a person; a check that needs
  the change live is made once, by the final turn, after every step is done.
- **An empty list** goes straight to the final turn.

## Work the item committed to is a step of its own order

- **What becomes a step:** everything the item committed to — the stages its plan laid out, the stages a
  settled document lays out — and the follow-up work doing it turns up. It is a step added to this order and
  run to its end, never handed to a job, a new item or a later run.
- **What goes elsewhere:** only work separate from this item — another part of the system the item never
  touched, a question only the owner can decide — is raised as a row of its own.
- **Who adds a step:** the synthesis turn by its list; any later builder turn of the order, with the add verb;
  and the agent, with the same verb, for work the owner asks to have done in the order. A step with no place
  named joins the end of the build list under the next free name, `build-<n>`.
- **A final turn that added steps hands the order back to the build:** its turn is retired as a done turn of
  this round, the new steps run one at a time, and a fresh final turn follows. That final turn's run is
  neither the item's job end nor a failure; the fresh one continues its attempt count.

## Every turn that decides what to make holds one line

- **A fix structurally needed for the work to be done properly** — a defect on the code path the item
  changes, a failure the change still leaves reachable, a repair needing a little more design — is
  designed, built and tested within the order, never passed on as unfinished or as a new item. A branch
  that finds one records it with its grounds; the plan and the synthesis put its repair into the work; the
  final turn adds what is left as build steps.
- **What a final turn may leave unfinished** is only what a job's hands cannot reach: what only the owner
  can give, host work no repository's service owns, or hardware. An improvement is never one of those.
- **The four places — branch, plan, synthesis, final — receive this line as one same sentence,** the
  work-turns policy's (docs/policy.md).

## How a middle turn fails by its own hand

- **It reported nothing, or it walked away from a Claude Workflow it launched.** A workflow cut mid-course is
  the turn's own thinking left unfinished, so whatever the next stage built on it would be wasted.
- **A background task left running does not fail a middle turn:** it died with the turn, the turn did its
  work, and what was left is named in the run's log (docs/runs.md, "The judgement").
- **A middle turn closes no item** and is no final report.

## A stage that has produced no result

- **Two measures:** how many times the run was started over, and how long it has run. A stage that finishes
  properly starts once, and one that must ask again for its shape has never passed three; past the policy's
  most, a run is spinning rather than working. A branch past its clock with no result is ended too.
- **Only a branch is ended,** since its place gets nothing back; a builder turn commits in the worker's own
  checkout, so it is told of once and never ended by the runner, and its clock alone notifies nothing.
- **An ended stage is a failed stage:** the order stops with its reason, a resume redoes that stage, and the
  finished stages stand.

## The final turn's checks

- **It is held to the job's own checks** (docs/runs.md, "The judgement"): no change of its own left
  uncommitted, no background task left running, no workflow walked away from, and no graph left behind.
- **A background task a middle turn is forgiven is a failure here,** since no stage comes after to reap it.
- **What was open before the order began is not the final turn's failure:** the final turn is measured
  against the baseline the order took when it took its checkout, carried across its stages, and, where a
  change parked on a branch was merged back in, the tree just before that merge.

## The final turn's end is the item's job end

- **A final turn that ended done with nothing ahead of it is the item's job end,** told to the agent as any
  job's is, with the commits its close left waiting to go live and its report.
- **Every other ending of an order's run is the order's,** never a job's: a stage's ending, and a final turn
  that failed or was stopped. The order counts it and tells it as its own stop, so the agent hears each once.
- **A final turn that ended done and left its item held is a pause, not a failure:** the one place an order
  waits — a check that needs the finished change live, once every step is done, or a stop the owner asked
  for. The order stops with every finished stage kept and its checkout let go, counting nothing; resuming it
  once the hold is let go runs the final turn again as its next attempt, which reads the account the last run
  left.
- **A final turn that ended done with its item neither finished nor held** is refused: the order stops with
  that reason and counts it.

## When a stage fails: the reason first

- **The reason is written first,** naming what was left — which background task, which workflow — and the
  turn's own words stand below it, as the job's report and the order's error alike, so the two never
  disagree.
- **A stop with no reason line of its own** — every road exhausted, a shape that never held — has its own
  sentence.
- **Nothing is added inside a passed turn's report,** which is what the next stage reads.

## The result document

- **A task's report is saved as its result document** under the item's folder of `[host] documents`, named
  by the task and its attempt, with two header lines of the runner's own on top: a title, the name the plan
  gave the task, and a line with the task's name, its role and its model. The report itself changes not one
  character.

## Every builder request

- **What is passed:** the whole intent, the finished work, the skipped work and why, the current task, and the
  tasks still to run after it, never naming the current one again (`none` where it is the last)
  (docs/queue.md, "What a step is handed").
- **Each rule is said once in a turn.** Every builder turn closes on the step rules of the work-turns policy;
  a middle turn is followed by the work-step trailer, which says what every middle turn shares; a branch
  carries its own rules.
- **An earlier result goes whole only to a session that has not read that version of it,** and as a one-line
  mark with its document's path to one that has: what a session received is recorded per session as the
  result's version, after the session answers; a compacted session and a fresh one have received nothing.
- **Stage, attempt, job, result and error are kept in the body,** so a restart of the service does not restart
  a running step.
- **A resume redoes only failed work;** finished investigation and build do not run again on their own.

## A worker running a co-work is a busy worker

- **An active order occupies its worker and checkout by itself,** since between one stage's end and the next
  launch no job row stands under the worker; a judgement counting job rows alone would read the checkout as
  empty and pile jobs onto it. A launch into it is refused for the same reason.
- **A stopped order is not a busy worker:** what it holds is a place in the worker's line, which the queue's
  one judgement weighs (docs/queue.md, "A worker's turn"). It is busy only while it checks its change, commits
  it and lets its checkout go.

## A stopped order's turn

- **A stopped item is still an approved, unfinished item,** so it keeps its place in its worker's line; a
  support worker's item stands in its base's line.
- **One judgement decides that line's turn,** asked by the telling, the runner and every reading of where a
  stopped order stands, so no two places disagree (docs/queue.md, "A worker's turn").
- **Its turn to resume:** first in its line and able to go now, the runner resumes it itself as soon as the
  worker's checkout is free — no open job, no live unit of a recent job, no launch waiting and the job lock
  free — against the body read inside the order's lock, failed work back to waiting and finished work kept.
  The checkout is taken again through the same door every stage launch passes.
- **Yielding the turn:** stopped on its provider with no road open now, it waits for a road; just stopped, it
  gives one turn to what stands behind it, or with nothing behind it waits out the minute after its stop.
- **Orders that do not resume:** none while the queue is paused; a held, unapproved or finished item's order
  has no turn; a closing order does not resume. An order under way whose item is held launches nothing more:
  its running turn is let finish, then it stops keeping its finished stages and lets its checkout go, counting
  nothing.
- **A resume the owner asks** from the console or the command line does not wait for the turn, and still takes
  its checkout by the same rule.

## The owner stops an item now

- **A stop is the owner's verb for work that must end at once.** A hold waits for the running turn; a stop
  does not. It takes a reason, which the item carries until it is taken up again (`todo --stop <id> --why
  '<reason>'`, or the console, both through the server's `POST /todo`).
- **The item** is held as `stop` with that reason and leaves the line at once; the agent is told of the stop
  and then of the queue's standing (docs/verbs.md, "The server's verbs").
- **A work order of it** has every running task stopped at once, branches and builder turns alike, through the
  service's one stop (docs/queue.md, "Stopping runs"); every interrupted task returns to waiting with its
  attempt kept, finished tasks stay, and the order stands stopped with the reason. Its pool places still
  waited for are given back, and the ones its running branches hold are given back as they end.
- **An ordinary job of it** is stopped the same way, and its row closes as `stopped`.
- **A job of a stopped item stops counting as work going on the moment it is asked to stop.** What keeps a
  second job out of its checkout meanwhile is the checkout's own lock.
- **Nothing a stop ends is a failure:** it adds nothing to the order's stop count or the item's attempts.
- **Taking it up again is letting the hold go;** the order then resumes at its turn, or at once by the resume
  verb, from the interrupted tasks onward.

## The owner winds an item down where it stands

- **A wind-down lets the running step finish and stops what would come after it** (docs/todo.md, "Winding
  down"): the item is held as `pause`, and the running turn is handed the wind-down line at once. A work order
  of the item stops at the end of its running turn with its finished stages kept, as under a plain hold; where
  that turn is the final one, resuming runs it again.
- **A run of a work order holds its own item only at its final turn, with no step ahead of it, and once;**
  from a middle step, or a final turn with a step ahead or after the order already paused once on its own
  hold, the verb refuses and names the add verb, since work that continues the item is a step of its order.
- **A stop is not a close.** Closing an order (「The verbs」) ends it for good; stopping keeps everything to
  continue from.

## How many times a stopped order is retried

- **A counted stop** is every stop that did not stop on its provider: a step whose job failed, left work
  running or left its work open; a stage the runner ended; a turn whose result the runner refused; a run that
  did not answer as it should; a runner that died; a unit that could not be started. A limit, a closed road
  and every road exhausted are not counted — the run never reached the work, and it is carried to another road
  instead — nor are the owner's stop, a held item's turn ending and a final turn that held its item.
- **The retries follow the count** at the order's turn: after one stop, the failed work as it was; after two,
  on a fresh session, the worker's kept session handed over first; after three, on the retry model — the
  setting `work_retry_model` where it names a Claude Code model, the policy's `queue.work_retry_model`
  otherwise — in place of the model the tier picked on the Claude Code road. The fourth sets the item aside
  with a hold of kind `failure`, the count and the first line of the last error as its reason, and the agent
  is told.
- **A resume the owner asks** retries the failed work as it was.
- **The count is kept on the order** and goes back to nought once a task of the current stage completes with
  no failed work left in it; a stop in an earlier stage is not summed with one far later, since a work order
  is different work across its stages.
- **One failure is counted in one place.** A stage's ending is never a job's end: it counts in no item's
  attempts, and the sweep that closes a dead runner's job leaves a stage of an order under way to that order's
  runner. A stopped order's job is no longer the runner's, so the sweep closes it like any job.
- **Where a stopped order stands** — its stop reason, why it is not resumed yet, and its counted failures — is
  kept on the order by the runner as its `LinePlace`, and every reading shows what is written there.

## One checkout, one work order

- **A checkout is lent per order, not per turn,** since a middle turn's change stays open in it until the
  final turn commits and a branch place takes those open changes on. Several orders using one checkout take it
  one at a time, by the priority of their items, and a holder does not let go for a more urgent order that
  comes later. Who holds it is written in the body, so it carries across a restart.
- **A repository with support workers has more than one checkout** (docs/worker-slots.md); each is held by one
  order at most.
- **Ordinary jobs and orders do not alternate on one checkout:** while an order holds it the queue assigns its
  worker nothing and a launch refuses an ordinary job there, and the order launches a step only once what ran
  there before has fully ended (docs/queue.md, "A checkout held").
- **A stopped order reaps its change while it lets the checkout go:** the change its own turns wrote, told by
  the files' times against the turns' windows, is committed onto the checkout's branch and pushed where the
  repository's own check passes with it through bmo-testbed, and onto `wip/<item>` where it does not or keeps
  not finishing. A support worker's worktree parks everything open onto the item's branch, since nothing else
  writes there; a base checkout beside support workers commits its order's change onto the order's branch and
  goes back on main. A file mixing the order's lines with others' is left open and named. What became of the
  change is kept on the order as `CheckoutNote`s, and what the order waits on as `ReleaseWait`.
- **On resume** a change left on a branch is merged back uncommitted as the order takes its checkout again,
  the tree just before the merge its new baseline; a conflict does not stop the order: its next builder turn
  is told to resolve the conflicted files first. Branch places are rewound to the order's commit each time
  they take work.

## The default model

- **`auto`.** Where the owner names no model for the work, the item is filed with `auto`, and the worker's
  walk settles the road (docs/queue.md, "Routes").
- **A co-work task's `model`** is a tier of bmo-agency's ladder, or null to follow its role's tier until its
  first attempt, except a builder task in a stage, which is written at the stage builders' tier. The attempt
  freezes the tier; retries and a move to another provider resolve the same tier there. A builder task may be
  set to any tier of the ladder, since it is the worker's own turn; every other role takes only a tier the
  pool lends at.

## The co-worker pool

- **Every branch and builder turn takes a place in bmo-agency's co-worker pool before its program starts,**
  and gives it back with what it came to once its task no longer runs and its unit is gone (docs/queue.md,
  "Launching a step"; docs/api.md, "The co-worker pool"). The pool's lines, limits, leases, reservations and
  the warming of reserved places are bmo-agency's.
- **A stage as wide as its design asks for** stands whole in the plan; how many run at once is the pool's. A
  task given no place waits, its program not started and its clock not running, and the next tick launches it
  once a place is free.
- **A branch on one provider does not cross to another inside its job:** that would run another model than its
  lease recorded. Its job ends as failed, the place is given back, and the next attempt asks the pool again on
  the next provider's same tier.
- **While a stage runs, the seats the next stage's branches will take are reserved** (docs/queue.md, "Seats
  for the next stage"), and given back when the order stops, fails or ends.

## Reopening a finished order

- **A round is one run of an order.** A finished order is not run again by a launch and takes no draft edit;
  the road to run it again is reopening (`work-order --todo <id> --reopen`, or the console).
- **The previous round's work stays:** every finished and skipped task moves to the history under its round as
  `r<round>-<task>`, keeping its state, result and result document, and the round's intent and instruction
  stay with it. The order returns to a draft and its item to waiting.
- **The new round's builder turn takes it as input:** the previous intent and instruction quoted, its tasks
  among the finished work under their own names, each result's first two thousand characters with the rest
  pointed at by its document, and the turn told to confirm only what the changed intent and instruction
  changed.
- **Intent, instruction and model are given in the reopen itself,** since the moment the item waits again the
  queue may launch it.
- **The new round's names carry the round** in unit names, branch names, prompt files and result documents, so
  nothing of the previous round is overwritten.

## The verbs

- **Every change to an order's body goes through one door:** the server's `POST /work-order`, which makes it
  inside the order's lock on the body freshly read there, beside the runner waiting on the same lock, and
  answers the order as it then stands (docs/api.md, "Work orders"). The lock is the database's, so it holds
  between processes (docs/rows.md, "A work order's lock"). A command line that cannot reach the server writes
  nothing.
- **From a shell** every verb follows `work-order --todo <id>` (docs/verbs.md, "A work order read and
  changed"); a task is named by the name a reading shows (`stage-2-0`, `build-3`, `r1-build-3`).
- **The task states** are pending, running, done, failed and skipped. Skipped is decided not to be done: it
  passes like done, keeps its reason and time, and is handed to the next turn as skipped work rather than as a
  result. The plan, the synthesis and the final turn are never skipped.
- **Edit an instruction** (`--task <task> --instruction … [--model <model>]`, a tier or a selection): only a
  pending, failed or skipped task. A running task never sees a saved change; the way to speak to a running run
  is the agent's change to the item's brief.
- **Skip** (`--skip <task> --why …`): a pending or failed task, or a running branch, whose run is stopped. A
  running builder turn is refused: stopping the item is the way to end one. A done task may be marked skipped
  to correct the record, in a finished order or an earlier round too.
- **Restore a skip** (`--restore <task>`): back to done where it holds a result, to pending otherwise.
- **Run one again** (`--rerun <task>`): back to pending with its result, job and error cleared and its attempt
  kept, so a new result document does not overwrite the earlier one; in the research phases a task of the
  current or an earlier stage, the stage place moving back to it; in the build a build task while nothing
  runs.
- **Add a step** (`--add <title> --instruction … [--before|--after <task>]`): a new build task under the next
  free name, at the end or beside a pending build task, in any phase of an unfinished order that is not
  closing; at the final turn the order goes back to the build once that turn ends.
- **Reorder** (`--move <task> --before|--after <task>`): two pending build tasks, or two research stages still
  ahead with every task pending.
- **Close** (`--close --why …`): ends an order under way or stopped for good, every task not passed over
  skipped with the reason and a running branch stopped; an order holding its checkout stops closing until it
  has let it go. The item is held with that reason, since an approved item with a finished order would be
  launched again for nothing.
- **Point it at another checkout** (`--checkout <path>`): an unfinished order only, and only to a git
  checkout.
- **Recover an overwritten round** (`--recover <first job>-<last job>`): a finished order run again from the
  start without a reopen keeps only the later run, but the job rows still hold each step's unit, prompt and
  result; given the earlier run's job range, it comes back as a previous round.
- **Each refusal is a `WorkOrderRefusal` code** (docs/api.md, "Work order edits: `WorkOrderRefusal`").

## The model a step ran on

- **A step keeps two models apart:** its `model`, the road its launch asked for, and `ranOn`, the provider and
  model its last execution actually went out on as bmo-agency reported it, with `changed` set once two
  executions of the step went out on different roads.
- **A line shows the model that did the work,** `provider/model`, with a fallback mark where the provider
  order fell to another provider than the one asked for and a changed mark where `changed` is set
  (`ModelMark`, docs/api.md). The request is said only in a detail, where it differs; `auto` reads as the
  model it settled on.
- **An item is shown by the road its work last took:** a co-work item's is its order's own `ranOn`, copied
  from every turn of the order's own line; any other item's is its latest job's (docs/reading.md, "The model
  an item ran on").

## What an earlier stage produced is a settled fact

- **The builder trusts a finished branch's result** and goes straight to building; it opens a file to write
  the change, to finish at synthesis a set a result left open, or where a result and the code in front of it
  plainly disagree — and then says which result disagrees rather than redoing the investigation.
- **A branch's result is written knowing it will be merged:** first the facts established and where, then the
  build work they call for, then what could not be established and what would; what stands in the first group
  a later turn takes as fact.

## The two turns of plan and synthesis

- **The first turn requires no shape.** The plan turn asks the graph, opens only enough code to name each
  branch's area and questions, and designs the stages, roles, titles and instructions in the worker's own
  words, passing counting and listing to the branches; the synthesis turn first finishes any set left open,
  then reports the build work cut by member, with each task's title and done condition.
- **The second turn is the runner's:** it asks the same session for what it just designed in a fixed JSON
  shape, and only that answer is read (docs/runs.md, "The turns"). The prompt carries the whole contract — the
  shape, the fields every task holds, one or more tasks a stage, a role from the roster, a model null or one
  tier — and no provider is handed an output-schema option, which would be part of the request's cached
  prefix. The shape is asked even of a turn that left a background task running; it is skipped only where the
  first turn reported nothing or walked away from a workflow.
- **The shaping turn continues the session that wrote the design,** so it is sent only the request and the
  design's text; only a run that cannot continue that session has the whole job prompt ahead of it.
- **Every answer is checked** for its syntax and its rules before the step ends. A refused answer is asked for
  again, up to twice more, with the exact refusal, each refused answer and its reason kept under the job's
  folder of `[host] documents`; a step whose shape never held fails, and its cause goes to the agent.
- **A task's title** is a name of two to four words for what that task does, made by the builder; a task that
  still comes with none has its stage's name stand for it.

## What a step is called

- **Every step wears the name of its kind,** never a bare count (`WorkStep`, docs/api.md): `plan`,
  `investigate` for a stage that finds things out, `review` for one that checks a finding or the plan,
  `synthesis`, `build` and `final`. The screens put each into words through the web console's language packs.
- **Only the build steps are numbered, among themselves** — the second of five build steps — wherever the
  design steps stood before them, and an order's progress counts the build steps alone.
- **The plan names each stage's kind** beside its title. A builder stage split off a mixed stage keeps the
  kind of the stage it came from; a stage of no kind the order knows reads as `investigate`.
- **The final turn is final verification alone:** work that turns up there is added as a build step.

## The language of the prompts

- **Machine-internal text is English:** the plan, synthesis and final turn instructions, the shaping request
  and its examples, the stage titles and task instructions a plan writes, and the section names of every
  step's prompt.
- **Text carried from the database reaches a step in English:** the order's instruction is the item's brief,
  the agent's own English; the item's title and the order's purpose go through bmo-gate's translations as the
  step's prompt is composed (docs/worker-language.md); an earlier result is a co-worker's own English. A text
  that could not be put into English reaches the step in its original under an English line saying so.
- **No step translates anything itself:** code, identifiers, error messages and domain terms are quoted as
  they stand, and every step's prompt says so once.
- **The order's report is English, to the agent,** as every job's is.
- **The refusal of a shape is an English sentence,** handed to the shaping turn as it was raised and kept as a
  stopped order's error; a person reads it in the console's own language.
