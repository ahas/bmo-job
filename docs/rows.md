# Writing the queue

The crate `queue-rows` writes the TODO's rows and work orders (docs/schema.md): an item filed, added to,
approved, weighed, changed, held, merged, closed and opened again, a work order's body read and written
under its lock, and the strands of an item's unfinished note read and settled. What `queue-read` reads,
this crate writes; the verbs, the routes and the command line stand on it (docs/api.md).

## Where a verb stands

A verb runs on a ground, which its caller gives it:

- **The database,** the owner's private chat, the deployment's names and agent key, the policy, the
  terms the owner has set aside (read out of the owner's wording document, docs/settings.md), the providers a job's model may name, and the moment, in epoch
  milliseconds.
- **Who runs it:** whether a job runs it and under which run, the agent whose turn files what it files
  and the label that agent's reply stands under, and the person at a shell where neither does.
- **Its hands,** what it does beyond the database, which the caller carries: the screens told the queue
  moved, with the work order whose body moved and what a job wrote onto an item; the agent told how the queue
  stands; the agent told of words added to an item; and an item's runs stopped.
- **Its units,** the calls it makes on other units: an instruction queued and a message read through
  bmo-receiver, a memory filed and the memories of a room read through bmo-brain, and a turn record kept
  through bmo-agency. A call that fails is refused with 502 under the unit's code (docs/api.md, "A call
  on another unit").

The chat agent's own turn is a run that is no job, filed by the deployment's agent.

## What a verb answers

A verb answers a `TodoAnswer` code with its fields, or a refusal (docs/api.md, "TODO verbs"). An item is
named by its number, or by the title of one that is not finished; a name no item answers to is refused
`no_such_item`. Nothing a verb answers is a sentence in a person's language.

## The rows

- **Statuses:** an item filed waits as `waiting` and a finished one says `done`, beside the flags the
  queue is read by.
- **Filing** puts an item behind every unfinished one, with links to the notes it was raised out of.
- **Finishing** an item closes every item merged into it with it, the merged ones carrying a report that
  names the item that carried them. An unfinished note the closing changed stands open again.
- **Opening again** a finished item opens the items merged into it with it.
- **A merge** carries each merged item's own merges to its target, and is refused, writing nothing, for
  an item missing, the target itself, a finished item, an item merged already or one with a job at work.
- **A wind-down** holds an item to resume later, after another item where it names one; an item the owner
  stopped stays stopped. Letting a hold go withdraws the line a wind-down put on the item for the run it
  interrupted, which opens with `[Stop asked]`.
- **Throwing out** a request removes its row with its links either way and its work order; the items
  merged into it stand as work of their own again. It is the one way a row leaves.
- **Words added** to an item are kept with the job they landed in, and taken back by marking them, never
  by deleting them. Words count as carried by a job once they were handed to one, or once a change to the
  brief made after them was.
- **An approval** given or taken back is written with the hand that made it: who (a person, an agent or a
  job, by name) and the road (the console, the verb or a chat tool), in the same transaction.

## The verbs of filing

- **Words added** are refused where empty, where the item is finished (`item_closed_already`) or merged
  (`item_merged`). A job's words on its own item are that item's record; anybody else's, a job's on
  another item among them, are the agent's to fold into the brief, and the agent is told of them as the fact
  `words_appended`, unless the chat agent's own turn added them.
- **The brief** is the chat agent's alone to rewrite, whole, in English, with what changed named in English.
  The change is kept as a note of its own kind with the job at work on the item, and a work order's
  instruction follows the brief.
- **Words taken back** are named by their note's number.
- **Filing out of a conversation** takes a title, the room, what the work is, what to do, a conclusion,
  the messages the owner said it in and the agent's replies or the reply's text. Each message is read
  through bmo-receiver and copied in with its sender and time; the first one's sender is the requester. A
  missing part is refused `filing_parts_missing`, naming every part missing.
- **Raising a row on the agent's judgement** takes a title, the room, the judgement, the item whose work raised
  it and the strands of unfinished notes it came out of, as `<item>:<strand>`; it takes none of a
  conversation's parts. The agent is told the row is its own to approve and launch now. A note settled before
  that a raised row breaks again stands open again.
- **Both filings** refuse a title or a part using a term the owner has set aside, outside its quoted
  spans, and a model that is no job model: `auto`, `provider::model` of a provider bmo-agency lists, or a
  Claude model by name. Each keeps a draft work order of the item, and a turn record where a run filed it.
- **Impact grounds** are written on the item as given, and the verdict they come to beside the co-work
  setting they never move. A job's own grounds are kept beside them; the agent is told where they come to
  another verdict than the setting, or where it filed none.

## The closing's readings

The closing's patterns and words are the policy's `closing` part (docs/policy.md).

- **An unfinished note's strands:** a listed note is read as its list, whatever stands above the first
  strand its lead; a note that lists nothing is one strand whole. A strand's mark says whose it is: the
  agent's or the owner's.
- **A strand is picked** by its number, and needs none in a note of one strand.
- **The closing refuses,** each in an English sentence naming the lines: strands that carry no mark; the
  owner's strands that are host work and name nothing only he can give; a report's section of what it
  left, or a strand, that proposes or defers an improvement instead of making it; a line leaving undone
  a check a job could make; and a page hosted outside the archive.
- **The live-Claude openings** an item's jobs made are carried into its report under their own heading,
  one line each with what it was for and how many runs it let through; a closing again writes the
  section afresh.
- **A note is settled** once every row raised out of it is finished, its resolution naming those rows,
  and stands open again once one of them is not.
- **The agent's strands are handed over** when an item closes: each becomes a memory of the agent's in
  the owner's chat through bmo-brain, tied to the item, and an instruction to do it; a strand the item
  handed over already is left alone. A memory's action is cut at two thousand characters.
- **Written in English** is the policy's test: code and links are read past, and what is left is held to
  the closing's English policy.

## A work order's lock

A work order's runner and an edit wait on the same row of `work_order_locks`. Within one process the
takers queue first, so only the head of the queue asks the database. A row's holder is its machine, its
process and that process's start, so a process id handed to a later process is not taken for the one
that held the lock; a holder gone is cleared, and one of another machine is waited on.
