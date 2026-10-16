# Policy

The service's fixed numbers and words stand in TOML files under `policy/`, one file to a part, and the
crate `job-policy` reads them. Each file is built into the crate, so a file that does not read as its
part fails the build's tests rather than a running service. No name a deployment goes by and no word of
a deployment's own language stands in a file: the names come from the settings' `[deployment]` section
and the words from its `[words]` section (docs/settings.md).

## The parts

| File | What it holds |
|---|---|
| `queue.toml` | The job queue's limits: how long the owner may stop a question's clock, how many files one word to a worker carries, the model a stopped work order's last retry runs on, and the tier a stage's builder task runs at where its plan named none |
| `job-prompt.toml` | What a job's prompt is made of beside the job's own text: the lines of the preamble a worker's job prompt opens with, the workbench names those lines cite, what a step of a bot-managed work order is told, and the block an item's impact grounds stand in |
| `research.toml` | A research run's keys, which of them are clocked and which open no branches, the ways a page is fetched, the capabilities a run's settings open and the variable opening each, the bounds of each field of a profile, and the rulebooks a search is bound by |
| `work-keys.toml` | Every work key and the profile it starts on: its kind, run line, road, tier and the model choices its first seed drew on, with the gate settings a gate adds and the answer floors an editor adds |
| `work-turns.toml` | What the bot tells the turns of a work order: the structural-fix line, the planning turn's paragraphs, the synthesis and final orders, how a turn adds a step, a branch's rules and result shape, and the blocks for merge conflicts and earlier rounds |
| `worker-words.toml` | A word said to a worker: its clocks, what the question at its clock names, the paragraphs it reaches the worker in, and the lines written under it for what held it or how its turn ended |
| `workers.toml` | What a worker may be: the pattern its name matches, the longest its label, preamble and dev server command may be, and how many support workers a base may have |
| `close.toml` | What a job's close says: its usage lines, the lines it writes about itself, its refusals of Hangul in a reason or a commit, the gate's lines, the close line's, the static check's and the test count's |
| `closing.toml` | What the closing of a TODO item reads a report by: the strands of an unfinished note and whose each is, the lines that propose or defer an improvement, the lines that leave a check undone, how a text is read for English, and a report's impact part |
| `landing.toml` | What a landing says: the refusals given to a branch that did not land, the lines about unit links, the operations machine's lines and the lines the land verb prints |
| `runaway.toml` | When a stage that has not reported is a runaway, and the label and lines its reason is told in |
| `hold-commits.toml` | The commit messages a work order holding its checkout writes its change under |

Every part refuses a key it does not know, so a misspelt key fails the part's test.

## Marks

A line of a part may hold marks, a name in braces such as `{worker}`, that are filled in with what the
mark names when the line is used.

- **Every mark is filled at once.** A value that itself holds a mark is left as it is, and a mark that
  nothing names stays in the line unchanged.
- **The deployment's marks are filled in every line** beside the values the caller gives:
  - `{owner}`, `{agent}` and `{sender}`: the owner's name, the agent's name and the sender name;
  - `{unfinished}`: what the note listing a closed item's open strands is called;
  - `{agent_mark}` and `{owner_mark}`: the marks a strand opens with to say it is the agent's or the
    owner's.
- A value the caller gives under one of those names takes the place of the deployment's.

## Patterns

The parts that read text hold regular expressions, each with the flags it is compiled with written
beside it. A pattern may hold the marks `{owner}`, `{agent}`, `{share}` and `{unfinished}`, and
`{names}`, standing for the owner's name or the agent's. A word is filled into a pattern escaped, so it
matches only itself.

The loader builds the patterns the closing reads with once, when the policy loads:

- **A strand's mark** is the opener's grammar around the names and the share word: either name, the
  share word after it, and a colon, with the emphasis marks markdown allows on each side. The marks a
  strand is written back with are a name followed by the share word and a colon.
- **The words read past when a text is read for English** are the names, a name followed by the share
  word, and what the unfinished note is called.
- **A report's impact heading** is a line holding only one of the configured impact words.
- **Every other reading pattern** is the configured one with its marks filled.

## Words

A part whose reading or writing depends on a language holds a `[words]` table of its own, written in
English. A deployment gives its own language's words under `[words.<part>]` in the settings file, the
part named by its file's name without `.toml`. Each key it gives takes the place of the part's own
value whole, so a list a deployment gives replaces the English list rather than adding to it. A key the
part does not take, or a part that takes no words, refuses the load.

The parts that take words:

- **`closing`:**
  - `share`: the word a strand's mark names whose it is by, after the name;
  - `unfinished`: what the note listing a closed item's open strands is called;
  - `left_headings`: the headings of a report's own section of what it left;
  - `impact`: the words a report's impact part is headed by;
  - `host_work`, `owner_only`, `raised_row` and `beyond_reach`: the patterns of host work, of what
    only the owner can give, of a row already raised, and of what puts a check beyond a job's reach;
  - `proposing` and `unchecked`: the patterns, each with its flags, of a line proposing an improvement
    instead of making it, and of a line leaving a check undone.
- **`job-prompt`:** `dev_tab`, the name of the tab a worker's dev server stands in.

## Loading

The policy loads once, from the settings, before anything uses it. A load that fails names the part and
one of these:

- the part's file is not TOML of its part's shape;
- the deployment gives words the part does not take, or words for a part that takes none or does not
  exist;
- a value the deployment gives is not of its key's type.
