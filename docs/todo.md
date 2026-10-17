# The TODO

The work the owner is owed, one row per piece of it: how a row is filed, approved, closed, edited,
added to, held, wound down, merged and thrown out, what its unfinished note hands on, and where the
owner works the queue. How a job carries an item is docs/jobs.md; the rows and the verbs that write
them are docs/rows.md; the verbs a shell runs are docs/verbs.md, "The TODO's edits and holds"; the
readings are docs/reading.md.

## What a row is

- **One row of `todos` per piece of work,** so a restart or a compaction cannot lose it. A row carries
  who asked and their id, the room it was asked in, whether the owner approved it, whether it is done
  and when, its priority, its title, the model its job is to run on, how the work was done and what it
  left, and the instruction itself: both sides of the conversation as they were said — the owner's
  messages copied in through bmo-receiver and the agent's own replies — because a summary drifts from
  what was meant, or, where the agent raised the row itself, the agent's own judgement in that place.
- **The session that took the instruction files the row itself** with the TODO verb (`todo --add`) or
  the TODO tool's filing (docs/tools.md). The verb also edits, adds words, rewrites the brief, closes,
  settles, approves, sets the priority, the worker, the co-work setting and the impact grounds, holds,
  stops, winds down, merges and throws out (docs/cli.md, "`todo`"). The queue's pause and resume are
  the owner's and belong to the queue rather than to any row.
- **Reading the queue** is the service's readings: `GET /todos` and `GET /todos/{id}` as a person reads
  them, and the worker's lines as a worker reads them (docs/api.md, "Readings").

## Approval

- **A request is not an approval.** Every row lands unapproved, whoever files it, and the queue never
  picks an unapproved item up however long it waits. Approval is its own verb and the owner's alone:
  his private chat's session approves what it files for him in the same turn, unless he said the row
  goes up as a draft; for anyone else's message a room's session files and does not approve. The queue
  is what is not done and is approved, in priority order, smaller first; a new item goes to the back,
  and moving one is a number rather than a rewrite.
- **An approval records the way it came** in the row's `approved_via`, and a launch reads that record:
  `owner` for the owner's own word, `rule` for a message from someone holding a standing rule in that
  room, `grant` for anyone else's grant there. Where an approval answers several messages, the weakest
  of their ways is recorded. Taking the approval back clears it.
- **Every change of an approval is kept on the item,** given or taken back, as a row of
  `todo_approvals`: who (a person by name, an agent by its turn, a job by its run), the road (the
  console, the verb, a chat tool), when, and for an approval the way it came. The row and the change
  land together or not at all.
- **Nothing takes an approval back on its own.** Stopping, holding, letting go, resuming, a work
  order's steps and a deployment all leave it as it stands, so every row of the record is a hand's.
- **Whether a request is filed at all** is the room's own session's judgement first; this service
  files what it is handed.

## Closing

- **Finishing does not delete.** An item is marked done and stays, so the table is also the record of
  what was done. The marking belongs to the session or job that did the work, or to the owner — never to
  the service, and never on the strength of a report alone: where the report says the tests did not
  pass, the item stays open.
- **A closing carries its own account.** `--done` takes `--report`, how the work was actually done in
  the closing job's own words, and `--unfinished`, the work the job's hands could not reach: what only
  the owner can give, host work no repository's service owns, hardware, a thing no machine here
  touches. A job writes both in English (docs/worker-language.md).
- **An improvement is never left.** One inside the item is made in the same job, and one beyond it is
  raised as a row the job names by its number; the closing refuses a report's section of what it left,
  or a strand, that proposes, suggests or defers an improvement instead.
- **A check is never left.** What a check could settle is settled before the closing; the closing
  refuses a line that says a check was left undone and names nothing that puts it beyond a job's reach.
  Quoted words are read past.
- **A structural fix is never left.** What the work needs to be right — a defect on the same code path,
  a failure the change leaves reachable — is designed, built and tested inside that job, never written
  down as an unfinished strand or raised as a row.
- **The live-Claude openings** the item's jobs made are carried into its report under their own
  heading, one line each with what it was for and how many runs it let through (docs/live-claude-guard.md).
- **A report and a note are kept whole** on the row rather than clipped, so an item read long after is
  the report of itself.
- **Naming a flag writes it, naming it empty takes it off, and leaving it unnamed keeps the row's
  value.** Opening an item again clears the account along with the closing it was taken back from.
- **The owner closes one himself from the console** through the same closing: refused for an item
  finished or merged already, wound down or stopped, or one a job is still running for. It settles all a
  closing settles: the items merged into it finish with it, a note it was the last open strand of is
  settled, a strand left to the agent is handed over, and the agent is told of an item waiting on it.

## Editing a draft

- **A draft is written again, not appended to.** `--edit <id>` rewrites the title and the parts the
  filer wrote — what the work is, what to do, the conclusion, or the judgement of a row the agent raised —
  each flag replacing that whole part, and the draft work order's intent and instruction while the draft
  still says what the part said. The words copied into the row from somebody — the conversation, the
  owner's words from the console, the note a row came out of — are never among them.
- **Refused** for a finished item, and for an item a job is at work on, since that job was launched on
  its words and never sees new ones: what it needs is folded into its brief. A title on its own passes
  while a job runs, since the title is the label the item is shown by.

## Added words

- **Words can be added to an item that already stands** (`--append <id> --text '<words>'`, `--by` where a
  session adds them for somebody else). Each is a row of `todo_notes` of its own, never rewritten: its
  number, who added it, when, and the job it came with — the job that wrote it, or for anybody else's
  words the job at work on the item then. An empty note and a finished or merged item are refused.
- **The words people add are for the agent and the console, never for a job.** A person's words and a
  companion's are read by no worker; the agent folds them into the item's brief, and a job learns of them
  only as the agent's change. What a job writes onto its own item is that item's record, which a later
  run reads. A job's words on another item are not that item's record and are read like anybody else's.
- **The agent is told of words it did not add,** as the fact `words_appended`: the item, the note, who
  added it and whether a job is at work on the item. The chat agent's own turn adding words wakes nothing,
  since the turn that adds them folds them in, and neither does a job adding words to its own item.
- **The brief is the item's organized instruction:** the agent's English, kept on the item apart from its
  own sections. The prompt an item's job is launched with is kept as its brief, and `--brief <id>
  --change '<what changed>'`, with the whole new brief on standard input, rewrites it to its final version.
  It passes while a job runs, since that job is handed the change rather than the brief, and it is the
  chat agent's alone. Every later reading for a worker reads the brief as it stands now, and every step
  of a work order reads it as the order's instruction (docs/work-orders.md).
- **A change is a note of the agent's own kind,** kept with the job it was handed to, so the item says
  what moved its instruction and when.
- **Words can be taken back** (`--unappend <id> --note-id <n>`): the row is marked, never deleted, with
  who took it back and when, and every later reading leaves it out. A run already handed a change has it
  in its context; the answer says so where the words may be in one.
- **A job running now is handed the agent's changes before it may finish.** A change is pushed into the
  running turn through the sanitiser where bmo-agency can take it, and asked over once the turn has
  answered otherwise, up to three rounds, each marked handed over as it goes (docs/runs.md, "The
  turns"). A person's words are never among them, and a change written before the launch was already in
  the brief and is not handed over twice.

## An unfinished note and its strands

- **An unfinished note says whose each of its strands is.** One strand to a line, each opening with the
  agent's mark, `<agent> share:`, for host work no repository's service owns, or the owner's, `<owner>
  share:`, for what only he can settle; the names are the deployment's (docs/policy.md, "Words"). A note
  that lists nothing is one strand whole. A strand carrying neither mark is refused before anything is
  written, so the item stays open and the note is rewritten.
- **Host work is never the owner's strand.** A step that needs root, a restart or the owner's word first is
  still the agent's; the agent asks him for the word and then does it. The owner's strand is kept for what
  he alone can give — his password or an account of his, a judgement of his own taste, a change to the
  structure document, a hand on hardware no machine here reaches — and the closing refuses an owner's
  strand that reads as host work and names none of those.
- **What is the agent's goes to the agent as the item closes.** Every agent's strand becomes a memory of
  its own in the owner's chat through bmo-brain, carrying the item it came out of, with an instruction to
  do it; two strands are two pieces of work. Closing the same item again hands nothing over twice.
  Repository work is no strand at all: a job that found separate work in its own repository raises it as
  a row.
- **A note can stop being outstanding** (`--resolve <id> --why '<one line>'`): the note itself stays, and
  the row gains when it stopped being outstanding and the line that says why. The unit is the item, not a
  strand.
- **A note broken into rows settles itself.** Which note a raised row came out of is a row of
  `todo_sources`, written as the row is raised; once every row a note was broken into is finished, the
  note is settled by that closing, naming the rows. A row waiting, set aside or at work is outstanding;
  throwing out the last outstanding row settles the note; opening a row again takes the settlement back;
  a settlement written by hand is never written over.

## Raising rows

- **A row the agent raises is the agent's own request.** Where a job's work raises a row — a strand that is
  still real work, or anything the work found — it is filed on the agent's judgement: `--add --judgement
  '<judgement>' --from-todo <id>:<strand> --raised-by <id>` writes what the work is, why it was left and
  what is proposed, and under it the one strand of each source note the row takes up. A bare item number
  stands only for a note of one strand. The row carries the agent as its requester and no requester id,
  since nobody asked for it, and lands unapproved like every other filing.
- **The owner raises one himself from the console,** naming a strand and writing what is to be done: his
  request, since the words are his. Raising is not approving, and the row it came out of is untouched.
- **A row may come out of more than one note,** each of which settles when this row and its siblings are
  finished. The reading of an item names, under a note, the rows it was already broken into and how far
  each got, so nobody proposes a second row for a strand that is one.

## Assignment and fan-out

- **An item says the model its job is to run on** (`auto` by default) and is read by the one it ran on,
  with its fallback and changed marks (docs/reading.md, "The model an item ran on").
- **An item says which worker is to do it** (`--worker <id> <name>`, `-` to take it off). The queue reads
  it to decide whose turn the item is, and a launch to decide which worker the job runs as. An item naming
  none, or a worker since removed, goes to the built-in worker; a support worker is refused, since an
  item's worker is its base (docs/worker-slots.md).
- **An item says whether its job fans out** (`--cowork <id> on|off`, or `--add --cowork`), weighed on the
  change's impact rather than its size.

## Impact grounds

- **An item that changes code carries the grounds its co-work setting was weighed on,** in the row's
  `impact`: every feature node of the code graph the change reaches, by its id, each with the outward
  contract that changes on it — an API, a schema, a wire format, a policy file, a documented behaviour —
  or none where only its inside changes, and whether a new home must be decided first. They are English.
- **The grounds come to a verdict by one rule,** which `queue-read` states and every reader asks: a change
  inside nodes alone, however many files, is a single job; one node whose outward contract changes is a
  single job; two or more, or a new home to decide, are a co-work.
- **They are written as JSON** in one shape, `{"nodes":[{"id":"<node>","contract":"<what changes>" or
  null}],"new_home":false}`, as the row is filed or afterwards with `--impact <id> '<json>'`, which answers
  the verdict beside the setting. Grounds that do not read, name no node and no new home, leave an id or a
  contract blank, or name a node twice are refused and nothing is written.
- **The grounds never move the setting;** co-work is still the one switch.
- **They are handed to a job of the item** in a block of their own ahead of its text (docs/runs.md, "The
  prompt"); a co-work branch is not handed them.
- **A job weighs them again before it changes anything** and records its own with `--impact-check <id>
  '<json>'`, kept in `job_impact`: where they come to the setting's verdict and the agent filed grounds of
  its own, nobody is told; where they come to the other, or the agent filed none, the agent is told with
  both sets, both verdicts and the setting. The setting stays the agent's to change.

## The hold

- **An item can be set aside** (`--hold <id> [--why '<one line>'] [--after-deploy <item|commit>]`) without
  being closed or thrown out, and brought back with `--unhold <id>`, never from a job's shell. A finished
  item cannot be held.
- **Nothing launches a held item.** It falls out of the queue and a launch refuses it by name; a run
  already going is not stopped by it. Its priority is untouched, so letting it go puts it where it stood.
- **Until that run ends the item reads `pending`,** waiting for its run to end before the hold takes
  (`HeldWord`, docs/api.md), and a work order of it stops at that same moment: its running turn is let finish,
  then the order stops with its finished stages kept and its checkout let go, and letting the hold go resumes
  it at its turn.
- **A reason is kept where one is given and never asked for,** and goes with the hold when it is let go.
  It is kept as written.
- **A hold has a kind:** none for a plain hold, `pause` for a wind-down waiting to resume, `stop` for the
  owner's stop, and `failure` where the item's runs set it aside (docs/queue.md, "How a job ends").

## Winding down

- **An item can be wound down where it stands** (`--wind-down <id> --why '<reason>' [--after <item> |
  --after-deploy <item|commit>]`): it is held as `pause`, the reason is the hold's, and the resume condition
  is kept in its own column. A run of the item going at that moment is handed one fixed line through the
  same road as the agent's changes — finish the step under way, start nothing new, commit, and leave where
  it stopped — before it may report. That line opens with `[Stop asked]`.
- **An item held as `pause` or `stop` cannot be closed as finished:** `--done` refuses it and names
  `--stop-here <id> --report '<what is finished and where the next run picks up>'`, which leaves the run's
  account on the item as words the next run reads, the row staying open and held.
- **A run told in words to stop holds its own item with the same verb:** `--stop-here <id> --why '<reason>'
  [--after …] --report '…'` winds the item down as `--wind-down` would, then leaves the account. It is
  taken only while a run of the item is going. A step of a work order holds its own item only at its final
  turn with nothing ahead of it, and only once; the work left otherwise is a step of its order
  (docs/work-orders.md).
- **Letting the hold go withdraws the wind-down line,** since it was an instruction to the run it
  interrupted. A wound-down item stays in view in the queue with its reason, because it is work the owner
  asked to have resumed.

## Resume conditions

- **A hold can carry a condition the service reads:** `--after <item>`, met when that item is finished,
  and `--after-deploy <item|commit>`, met when commits go live. A hold carries one at most, each in a
  column of its own.
- **`--after-deploy` is settled to commits when the hold is put,** through bmo-testbed's record of what
  waits to go live: an item names its commits waiting at that moment, and one with none is refused; a
  commit must be on main and not live yet.
- **Meeting a condition releases nothing.** The service tells the agent in one quiet instruction naming each
  item, its reason and what met it, and the item stays held until the agent lets the hold go and launches
  it. Finishing an item meets every `--after` naming it or an item merged into it; a deployment going live
  meets every `--after-deploy` whose commits it carries, and the condition then comes off the row.
- **A job never lets a hold go.** A job whose work makes another item ready says so in its report, and where
  that item's hold carries the condition, the agent is told as well.

## The merge

- **An item can be merged into another** (`--merge <id>[,<id>] --into <id>`): rows that are one piece of
  work stand under the row that carries it. Merged is a state of its own: `merged_into` names the carrying
  row, and the database's triggers keep the reference true — never the row itself, never a missing row,
  never a row merged in its turn, never a row others are merged into, and no delete from under them
  (docs/schema.md, "The merge of TODO items"). The queue, a hold, a release, a resume and a launch never
  pick a merged row up. Its work is the carrying row's job's work.
- **Closing the carrying row closes every row merged into it that is still open,** each with a line
  pointing at the carrying row's report; opening the carrying row again opens them as merged; `--unmerge
  <id>` takes one back into the queue at its old place.

## Throwing out

- **The one thing that removes a row is `--drop`,** for a request that should never have been filed; its
  links and its work order go with it, and the items merged into it stand as work of their own again.
  Taking an approval back is not one: an unapproved item leaves the queue with everything it carried.

## The approval's note

- **An approval can carry the owner's own word** — a further remark or a question he wants answered. It is
  kept on the item for the agent and the console: the agent organizes it into the job's brief, and the job
  answers a question in it in its report. No worker reads the note itself.

## Where the owner works the queue

- **The owner works this queue from the web console,** which reads the service's readings and makes every
  change through the server's routes (docs/api.md, "The TODO"), so this service stays the one writer of
  the table: approving with or without a note, throwing a request out, moving an item within the queue
  or to its front, choosing its worker, adding words, setting it aside, closing it himself, taking an
  approval back, and the queue's pause. Words in the console are the web console's language packs; every
  answer this service gives there is a code.
- **What moves the queue:** a job that ends tells the service, and the service tells the agent the fact, so
  nothing waits for a restart to move — unless the owner paused the queue, and then nothing launches until
  he lifts it.
