# The language between the agent and its workers

A job worker is an agent, and what only an agent reads is English. The owner and the rooms read their own
language. So the worker reads English and reports in English, every translation in between is done on
the agent's side and through bmo-gate, and every screen a person reads is the web console's, which
renders this service's English and codes in the person's language. This service writes no sentence in a
person's language.

## The rule

- **A worker reads the agent's organized instruction, never the raw item.** What a job is told of its item
  is the item's brief — the instruction the agent organized out of everything said, which a launch hands
  over as the job's prompt — and the changes the agent hands it when words are added (docs/todo.md, "Added
  words"). The item's copied conversation, the words a person or a companion adds and the owner's approval
  note are for the agent and the console alone: no worker reads them, not in its prompt, not through the
  queue's tools and not pushed into its run.
- **What reaches a worker is English.** The brief and its changes, the words said to the worker, the owner's
  words in the worker's preamble, a work order's texts, and an item as the queue's tools show it reach the
  worker in English.
- **What the agent writes for a worker is English at its source.** The brief and every change to it are the
  agent's own English, so nothing waits on a translator. Only what a person wrote for a worker directly —
  words said to the worker, the preamble — and the item's own texts are put into English on their way.
- **The owner's words are quoted only where his wording is itself the requirement:** in a ```` ```text ````
  fence exactly as he said it, an English rendering right after it saying all it says and nothing more. A
  fence is code to every language check and every translator, so his original reaches the worker untouched.
- **What a worker writes back is English, to the agent.** A job's report, the account it closes its item with,
  the reasons it holds or stops an item for, and its unfinished note's strands are written to the agent in
  English, and kept on the rows as written. The worker is not the agent: it writes in no persona and never
  addresses the owner.
- **What goes into git is English only.** A commit message, a branch name, a tag, and the close's reasons are
  English, the owner's quoted words in them too; the names of address the close's policy sets aside are the
  one exception (docs/policy.md, `close.toml`). The close and the landing refuse anything else and name the
  commit (docs/jobs.md, "A job closes"; docs/worker-slots.md, "Landing on main"), and the shell hook refuses a
  commit, tag or branch command whose message or name holds it (docs/job-tools.md, "What a job's commits may
  carry").
- **The worker never translates.** A worker that meets another language anyway — a quoted domain term, an
  error message, a file's own contents — reads it as it stands and never puts it into English for anyone.
- **What is never translated:** code identifiers, paths, commands, product and service names, error messages,
  forms of address, and domain terms a reader needs to match against the code or the screens.

## Which hand translates

- **The chat agent writes in English what it hands a worker:** the prompt it launches a job with, kept as the
  item's brief, and every rewrite of the brief and change it hands with the brief verb. What people say for
  the record stays as it was said, and the item's own sections are filed as the agent filed them; of these a
  worker reads only the title, and the sections of an item with no brief yet, both in translation.
- **bmo-gate, for everything copied out of the database for a worker:** words said to the worker, the owner's
  part of a worker's preamble, a work order's purpose, an item's texts as the queue's tools show them. The
  service asks bmo-gate for the kept English of each original first and for a translation where none is kept
  (docs/api.md, "Translations"). A text already English, a fenced quote in it included, passes untouched.
- **No gate for a job's prompt on its way to the worker.** Whoever hands a job its prompt writes it in
  English, and the runner hands the worker the prompt as written, through the sanitiser alone (docs/runs.md,
  "The prompt"); a part in another language handed over anyway reaches the worker as written.

## A job's prompt

- **The job's row keeps two readings of its prompt:** the prompt as it was handed over, never written again,
  since a work order and a rerun read their steps back out of it; and the text the worker was handed, through
  the sanitiser, as its English (`prompt_english`).
- **The header over the job's turns names the item** by its number and its kept English title, or, with none
  kept, the title as written followed by `[not translated]`.
- **A person reads the prompt** on the console, which renders it in the person's language; this service keeps
  no rendering of its own.

## Where the translation is made

- **The runner, before the first turn,** asks bmo-gate to put the item's texts into English — its title, its
  own sections while it has no brief — so they are kept before the job first asks for its item and before the
  header over its turns is made; the owner's part of the preamble goes the same way. A text the gate could not
  render is left unkept and shown under the mark, and the job starts all the same.
- **The runner, as it pushes words into a running job,** puts the words said to the worker into English first,
  and the same for a run asked again over words that came after its answer. The agent's changes to the brief
  are English already and go in as written.
- **A work order, as it composes a step's prompt,** puts the texts it carries from the database into English
  through bmo-gate; a step whose texts could not be put into English waits for the next tick rather than
  failing. An earlier stage's result is a co-worker's own English and passes untouched.
- **Nothing a worker runs calls the translator.** The queue's tools and the TODO verb read the kept
  translations bmo-gate answers and never ask for a translation, so a worker's reading never waits on a model.

## What is kept

- **A translation is bmo-gate's to keep,** one reading per original, keyed by the original exactly as written,
  so a changed original is translated afresh and an unchanged one is read straight back, whichever item, note
  or job it stands in. The original is never overwritten.
- **Only a whole reading is kept.** A text a part of which could not be rendered comes back with what could
  be, the rest standing in its original, and nothing of it is kept, so the next reading asks again.

## When there is no translation

- **A job is never held for a translation,** beyond the gate's own wait.
- **A text with no translation goes in its original, marked:** a line `[Not translated: this stands in its
  original language.]` before it, or `[not translated]` after a text standing inside a line, such as a title
  or a hold's reason (docs/reading.md, "A worker's reading").
- **An item's instruction keeps its English heads even then:** the heads are the queue's own words, so the
  worker still reads each block's label and the fixed words of its source line in English, and only the words
  inside each block stand as written under the mark.

## The queue's tools, as a worker reads them

- **A worker's reading of the queue is English,** labels and all, each text a person or the agent wrote
  replaced by its kept English (docs/reading.md, "A worker's reading"). It is the worker's alone, served by
  the worker's lines routes and the TODO tool's worker entries (docs/tools.md).
- **An item, as a worker reads it, is its organized instruction and its record:** its state and what jobs
  wrote onto it, its brief and the changes the agent handed, and each row merged into it by number, title and
  state. It never shows the conversation, the words a person or a companion added, a job's words from another
  item's run, or the approval note. An item with no brief yet shows its own sections in their place.
- **The worker's reading leaves out the time an item's runs took and what they spent;** nothing a job does
  turns on them.

## What the machine writes for a worker

- **Text no person wrote for the moment is English at its source,** never translated on the way: the policy's
  lines, the work-order prompts, the runner's notes under a report, and every answer of this service's verbs
  (docs/job-tools.md, "What this service's verbs answer with").
- **What it quotes stays as written:** an item's title in another language, a phrase handed to a verb, a row
  read out of the database.

## A worker's report

- **The report is English, to the agent,** in the shape the closing skill gives it: a summary paragraph, the
  detail and a closing line of figures. It carries what the agent needs to tell the owner and to act, and
  never speaks for the agent.
- **The job's row keeps it as written,** and the item's own account is the English the closing wrote.
- **The service does not send it to anyone.** It hands the report to the agent with the job's ending, and the
  agent tells the owner in the owner's language; a room that asked hears one line of what it came to
  (docs/queue.md, "Telling a room").
- **Where no row could keep the report,** the runner hands it to the agent in the owner's chat as it stands,
  under a line saying it is the worker's own report that could not be kept (docs/runs.md, "The ending").

## What stays in the owner's language

- **A question a job puts to the owner** with `job ask`: he reads it himself, with no agent between, so the
  job writes it in his language (docs/jobs.md, "A job's question to the owner").
- **What a job writes into a screen, a room or a document a person reads,** by that place's own rules.
