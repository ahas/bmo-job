# Reading the queue

The crate `queue-read` reads the queue as the database holds it (docs/schema.md) and never writes it.
What it reads is answered two ways: to a person as structured values, which the web console or the
command line lays out in the reader's language, and to a worker as English lines, since a worker reads
English. A work order, a room's jobs, a worker's job line and the jobs that have just run are read here
as well; what of a running job's line only bmo-agency holds is read there by the server.

## One read of the database

A read opens with the whole queue and what every line is read against:

- **The items,** what is still open first, each part by priority and then by number.
- **The sources** each raised item came out of.
- **The workers** that are not resting, by name and the base a support worker belongs to.
- **Which items still have a run going:** an item with a job not yet finished, or with a work order
  whose status is `active`.
- **The road each item last ran on,** the provider and model its work last went out on: for a co-work
  item the road its work order records, and for any other item its latest job that names both.

An item's own records are read only once that item is opened: its work order, its job's questions in
the order they were asked, the words added to it and to every item merged into it that were not taken
back, and its runs oldest first, each with the spans its job wrote down, the longest first.

A failure of the database is answered as the refusal `database` (docs/api.md, "A refused request").

### Which rows a listing shows

A listing leaves out finished items unless every item is asked for. It may be narrowed to items
waiting for approval, to held items, to merged items, or to one room's items. Without the held or
merged narrowing it leaves out merged items and held items, except an item whose run still goes and
an item wound down to resume.

### Whose work an item is

An item is the work of the worker it names. A support worker's item is its base worker's, and an item
naming no worker, or one no longer standing, is the built-in worker's.

## The model an item ran on

An item's model is the road its work last went out on, `provider/model`, or the model it asked for
where nothing has gone out yet. An item that asked for the default and has run nothing has none.

- **A model asked for** is `provider::model`, or a bare model where it names no provider.
- **The road is the one asked for** where its model matches and the provider matches or none was
  named.
- **A fallback** is a road on another provider than the one asked for. Where the request names no
  provider, `auto` and the default included, the provider asked for is the head of the provider order:
  the first provider that is on in the list bmo-agency answers, and where none is on such a request
  falls back nowhere. The caller hands that list over; this crate never asks for it.
- **Changed** marks an item whose runs went out on more than one road.
- **The request** stands beside the road only in an opened item, and only where the road is not the
  one asked for.

## A person's reading

A person's reading carries no sentence. Each state and mark is a kind from docs/api.md, and every name,
title, time and count is carried as it stands for whoever shows it.

### An item's line

One item in a listing carries its number and title, and then:

- **Its state:** merged into another item, finished, waiting for its requester's approval, or the
  status it is in.
- **Its marks,** in order: the merge of a finished item, co-work, the owner's note on its approval,
  a report, and an unfinished note with whether it is resolved (`LineMark`).
- **Its model,** with the fallback and changed marks (`ModelMark`).
- **Its worker,** only where the item names one.
- **Its hold:** the held word (`HeldWord`) and the reason given.

The held word is read from who set the item aside and whether its run still goes: the owner's stop,
a pause to resume, a failure, or a plain hold, each with its own word while the run still ends; a hold
whose run still goes and which none of those set is pending.

### An opened item

An opened item carries everything its line does, the model's request included, and then:

- when it was held and why;
- the item it is merged into, when, and whether it finishes with that one, and the items merged into
  it, each with its state (`RowState`);
- the item it repairs, and the items filed to repair it, each with its state;
- who asked for it, whether it is approved, its priority and whether its worker was assigned;
- the impact grounds its co-work setting was weighed on, and those a job weighed again;
- its instruction in the reading order (below), and the draft of its work order where the draft says
  something the item's own blocks do not;
- the owner's note on its approval;
- the words added to it, each with its number, author and time;
- the work of each open item merged into it: its instruction, owner's note and added words;
- each question a job asked, with its state (`QuestionState`) and the option it was settled on;
- the time its runs took (below);
- its report, and its unfinished note with its resolution and the rows raised from it.

### The time an item took

An item nobody has run has no time. Otherwise its reading carries:

- **Each attempt:** its job, how long it took in milliseconds where it finished, and its outcome.
- **The total** of the attempts that finished, and how many did.
- **The phases** the jobs wrote down, added up by name across the runs.
- **How the run's own phase divided,** where its spans divide it: waiting on its branches, running its
  own tools, and the rest, which is writing. The run's own phase is the one named `agent run`.
- **What was busy inside the run:** its tools and co-workers, the twelve longest, and how many more
  there were. A wait on background branches stands only where no co-worker span does.
- **What each model spent,** the most first, by kind of token (`TokenKind`), and how many attempts
  wrote down no spending.

A span inside the run carries its name as it was written and, where it reads as one, what it names
(`SpanName`): a tool called through an MCP server, a tool of the CLI, or a co-worker by its agent type,
and beside a co-worker the words it was handed. Which names are MCP calls is the toolbook's reading,
which the caller hands over as a function from a name to its server and tool. A co-work branch's span
is named `branch` and its index.

## A worker's reading

A worker reads an item, and a listing of items, as English lines. Every text a person or the agent
wrote stands as its kept English translation, which the caller reads from bmo-gate beforehand in the
language `en`; nothing here asks the translator.

- **A text reads as English** where, outside its fenced code blocks and inline code, its Hangul
  syllables times four are fewer than its Latin letters. Such a text stands as it is.
- **A text with no kept English** stands in its original under the line
  `[Not translated: this stands in its original language.]`, or, inside a line, followed by
  `[not translated]`.
- **The instruction** a worker reads is its own sections, the copied conversation left out, in the
  reading order. Once a job was launched the agent's brief stands in its place, with the agent's
  changes to the brief, each with when it was written and whether a running job was handed it.
- **The words added to an item** a worker reads are only its record: the words written under a job's
  name, `Job` or `Job` and its number, by a job of that item.
- **A settled question** names the owner as the deployment names them.

## The blocks of an instruction

An item's instruction is written as labelled blocks and loose paragraphs between them. This shape is
the service's own, so it is read by hand rather than by a general reader.

- **A block** is a line of its label and, in parentheses, where its words came from, ended by a colon;
  then a fence of three backticks or more longer than any run of backticks inside, the word `text`, the
  words, and the same fence alone on a line.
- **Loose text** is any paragraph that is not a block, and none is dropped.
- **The reading order** is the copied conversation first, under the line
  `The conversation, word for word:` and in blocks labelled `Instruction as said` and `Reply as said`;
  then `What the work is`, `Conclusion` and `What to do`; then everything else where it stood.
- **A draft repeats the item** where both its purpose and its instruction are the words of one of the
  item's own blocks.

## A room's jobs

A room's jobs are listed newest first, twenty where no number is asked for and never more than two
hundred. Each carries its label (`JobLabel`), or the purpose it was launched for where it has no item;
its unit, when it was launched and finished and whether it worked; its prompt in the deployment's
language where that reading was kept, otherwise as written; the English its worker was handed, where it
differs from the prompt; and its report.

## A worker's job line

A worker's screen keeps a line for one job: the job the screen was opened at, where that job is the
worker's; otherwise the worker's job still running, the latest; otherwise the worker's last job. The line
carries the job's unit, its item and the item's title where it has one, the purpose it was launched for,
when it was launched and when it finished, how many tools it called and what each model spent.

- **What a finished job called and spent** is read off the spans it wrote as it ended (docs/jobs.md,
  "Where a job's time went"): its tool calls are what its tool spans count, and each model's spend is its
  usage span, the four kinds of tokens apart, the model that spent most first.
- **A running job has written no spans yet,** so its tool calls are counted in its run's log at
  bmo-agency: the worker's lines of the kind `tool` in the run its unit names. What each model has spent
  so far is bmo-agency's to say while the turn goes, and no route of its answers it, so a running job's
  line names no model.
- **Its prompt** is read apart, since it is kilobytes a line read again and again has no use for: the
  prompt in the deployment's language and the English its worker was handed, as a room's jobs carry them.

## The jobs that have just run

The jobs that have just run are every room's jobs newest first, ten where no number is asked for and
never more than two hundred. Each carries its number, its unit, its room, the model it was launched on,
when it was launched and finished, its outcome and whether it worked, its label (`JobLabel`), the purpose
it was launched for where it has no item, and its worker. The runner runs beside them are bmo-agency's
run ledger, which the service does not keep.

## A work order

An item's work order is the one the database holds, or the draft its own title and instruction would
start where it holds none. A work order is read in English lines for an agent or the command line: its
number, status, phase and stage, its worker, model and checkout, its purpose and instruction whole, each
stage's tasks by state, role and the name the plan gave them, the build steps that belong to no stage,
and the facts that decide whether it can be resumed. A task the plan named nothing is told by the first
sixty characters of its instruction. The history and the earlier rounds are counted, never listed;
their contents are the order's structure.

### Where a task stands

Every task of an order stands at one place: an earlier round of its history by its position there, a
stage's task by the stage and its position in it, a build step by its position in the build list, or
the control turn, which plans, synthesises and closes. A task is found by the name it carries, the
first that carries it in that order.

The tasks an order is at now are the present stage's tasks while it researches, the first build step
not yet done or skipped while it builds, and the control turn otherwise, where the order has one.
