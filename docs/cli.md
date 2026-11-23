# The command line

The binary `bmo-job` carries these commands.

| Command | Does |
|---|---|
| `bmo-job nouns` | Every noun this unit carries (`job`, `job ask` and `todo`), one a line, `job ask` marked `home` after a tab, which `bmo` reads to find the commands it hands this unit |
| `bmo-job serve` | Serves the API by the settings file (docs/settings.md) until it is told to stop by an interrupt or a termination signal; every open event stream ends then, so the stop is not held |
| `bmo-job mcp --service <url>` | Serves the harness plugin's MCP server on standard input and output, reaching the service at the address (docs/api.md, "The harness plugin") |
| `bmo-job job close` | Checks what the job changed, merges its commits onto the remote's main and leaves the item waiting for its repository's batch deployment (「`bmo-job job close`」) |
| `bmo-job job ask` | Puts a question to the job's supervisor in the middle of a job, which settles it or puts it to the owner, and waits for it to be settled (「`bmo-job job ask`」) |
| `bmo-job todo` | Closes an item, holds it with the run's account on it, or raises a new one, each through the service's own routes (「`bmo-job todo`」) |
| `bmo-job openapi` | Prints the OpenAPI document, the one committed as `docs/openapi.json` |

## `bmo-job job close`

`bmo job close --then '<why this goes live, in English>' [--todo <item>] [--skip-suite '<why, in English>'] [--checkout <path>]…`
runs this, through the unit command the deployment links into `~/.local/bin` (docs/deploy.md, "The unit command").

| Option | What it does |
|---|---|
| `--then <why>` | Why this goes live, in English; required. A reason holding Hangul is refused |
| `--skip-suite <why>` | Read as a reason, English like `--then`, and nothing more: the close runs no suite, so it changes nothing, and the close says so in one line |
| `--todo <item>` | The item the job works on; the close writes where it stands onto that item (below) |
| `--checkout <path>` | A checkout the close closes, read from the directory it runs in, given once for each; with none, the close closes the checkout it runs in (「Several checkouts in one close」) |

A close ends at the merge. It checks what the job changed, puts the job's commits on the remote's main, marks
the item waiting for its repository's batch deployment and ends; it runs no suite, waits for no deployment and
confirms nothing live. The suite, the deployment and the live check are the batch deployment's, which puts every
commit waiting on main live at once, its gate running the suite once for all of them (docs/deploy.md, "What gates
a deployment").

```mermaid
flowchart TD
  A[Reasons read] --> B[Remote fetched]
  B --> C{Commits not on main?}
  C -- yes --> D[Commit messages read]
  C -- no --> E[Range from the record]
  E --> F[Commit messages read]
  D --> L[Close line taken]
  L --> R[Remote fetched, HEAD rebased where main moved]
  R --> G[Check since the base]
  F --> G
  G --> H[HEAD pushed where main lacks it]
  H --> I[Close line let go, item waits for the batch]
```

- **The main it lands on.** The remote is the one the checkout's branch follows, else `origin`; its main is the
  remote's `main`, else its `master` where the remote holds no `main`. The close fetches that remote first, and
  a remote that cannot be read refuses the close with what git said.
- **The job's own commits.** Where `HEAD` holds commits the remote's main does not, those are the job's own: the
  close has commits to push, and the base is the remote's main as the close finds it inside its close line.
- **The close line.** A close with commits to push stands in its repository's close line, bmo-testbed's
  (bmo-testbed's `docs/testbed.md`, 「The close line」), from before its rebase to the end of its push, so no
  other close of that repository on this machine lands while it checks. It takes the line through bmo-testbed's
  `close-line` verb, started with its checkout, which waits its turn, says every thirty seconds how many closes
  stand ahead and which holds the line, and holds the line until the close lets it go; the close passes those
  lines on as its standard output. The line is held by that verb's process alone, so a close that ends in any
  way lets it go. A close with nothing to push takes no line.
- **The wait for the line has no clock.** It ends once the line is the close's, once the harness the close runs
  in is gone, which the testbed answers with exit 75 and the close is refused with what it said, or once the
  close is told to stop.
- **The rebase.** Inside its line the close fetches the remote again. Where the remote's main no longer stands
  under `HEAD`, another close having landed while this one waited, the job's commits are rebased onto it, so
  the check runs on the commit that lands, and the close says in one line the commit `HEAD` moved from and the
  one it stands on now. A checkout holding uncommitted changes to tracked files is not rebased: the close is
  refused naming them, to be committed or put aside before closing again. A rebase that stops on a conflict is
  aborted, which leaves the checkout as the close found it, and the close is refused naming the paths in
  conflict and the command that rebases by hand.
- **A commit already on main.** Where the remote's main already holds `HEAD`, nothing is pushed, and the range is
  read from the deployment record (docs/deploy.md, "A job closes"): the job's commits are those past the commit
  the record names as live. Where the record's commit is not in the repository or is no ancestor of `HEAD`, the
  range is taken from the job's own base commit instead, and the close says in one line which range it used. A
  commit the record already names as live runs nothing and ends clean.
- **What is refused before the check.** A commit of the range whose message holds Hangul, each such commit named
  with the command that rewords it; nothing is checked, pushed or written on the item.
- **The check.** `bmo-testbed check --since <base>`, which `bmo check --since` runs, runs in the checkout: the
  check a change owes over every path changed since the base (bmo-testbed's `docs/checks.md`). A check run inside
  the close line is handed `BMO_TESTBED_NEXT`, so its seats stand next after the front (bmo-testbed's
  `docs/testbed.md`, 「Next after the front」). A check that does not pass refuses the close with exit 1, and
  nothing is pushed.
- **The merge.** Once the check passes, the job's commits are pushed onto the remote's main as `HEAD` stands, and
  the close line is let go once the push has ended. A push the remote refuses, its main moved by a push that
  stood in no close line on this machine, refuses the close with what git said and the way to close again;
  nothing is written as waiting.
- **Where the close stands is written on the item `--todo` names,** as labels of its own repository, the rest of
  the item's labels kept: `close-phase:<repository>` is `testing` from the moment the check starts and
  `waiting-batch` once the commit stands on main, and `close-commit:<repository>` is that commit, written with
  `waiting-batch`. A reader of the queue tells from them an item whose work is being checked from one whose
  commit waits on main for its repository's batch deployment.
- **The repository is the name its testbed declaration gives,** the name its deployment goes by; a checkout
  whose declaration names none goes by the last part of the address of the remote its main is fetched from,
  without `.git`, and else by its directory's name.
- **An item whose work spans repositories carries a pair for each.** A close in one repository writes and takes
  off that repository's pair alone, so the commit another repository's close left waiting stays on the item.
  The item's work is live once every commit its pairs name as waiting is live. Telling that, taking a pair off
  once its commit is live, and the batch deployment itself are the deployer's.
- **`close-phase` and `close-commit`, with no repository, sum the pairs up** for a reader that reads one commit:
  `close-phase` is `testing` while any repository's close checks, else `waiting-batch` while any repository's
  commit waits, and is absent otherwise; `close-commit` stands beside `waiting-batch` alone and names one waiting
  commit, the one the close writing it has just left, else the one it already named while that still waits,
  else the waiting commit of the repository first in name order. Each close writes them again from the pairs
  the item then carries.
- **A repository's pair is taken off** where its close is refused, where its check does not pass, where its push
  is refused, where the close is told to stop, where the commit is already what is deployed, and where the
  repository deploys nothing, so an item carries it only while that repository's check runs or its commit
  waits.
- **The item is reached as `bmo-job todo` reaches it** (「`bmo-job todo`」), and a restart is no answer. While
  the service does not answer, nothing at its address, a connection cut, a 5xx or a request left unanswered for
  ten seconds, its labels are written again, after a quarter of a second at first and every five seconds at
  most. The close goes on once they are written, once two minutes have passed with no answer, or once the run
  is told to stop; the last two are said in one line. An item the service refuses, one it does not hold or a
  change it will not take, stops no close: the close says so in one line and goes on.
- **A close with no `--todo` writes on no item.**
- **The check is watched.** It is started in the checkout, in a process group of its own, its standard output
  and error read through pipes and written on as they come. It is handed `BMO_TESTBED_HEARD`, naming a file of
  the close's own, which the testbed touches while a command it watches writes but holds its output back; a
  touch counts as output.
- **A check that falls silent ends the close.** A check that writes nothing and touches nothing for the close's
  silence limit is hung. The limit always stands five minutes above bmo-testbed's own bound on a silent unit,
  so the testbed ends a hung unit first and names it, and the close's limit catches what the testbed does not
  watch. Where the checkout's `testbed.toml` declares `[test] silence_seconds`, the close's limit is that many
  seconds and five minutes more; where it declares none, or its declaration does not read, the testbed keeps
  its ten-minute default and the close's limit is fifteen minutes. The close names every process still running under the check, in its process
  group or below it, sends each a TERM, and a KILL to whatever stands ten seconds later. It then reads the
  checkout's `git status` against what it read before the check, and refuses the close with the check's last
  line, what was still running, and whether the checkout stands as it found it or which paths differ:
  `job-close: refused — the static check wrote nothing for 900s and was ended`, the seconds being the limit
  the close kept to. Besides its rebase, the close writes nothing in the checkout.
- **A running close says where it stands.** Every minute while the check runs, one line on standard output
  names it, how long it has run, how long ago its output last moved and its last line, so a reader tells a slow
  check, whose output moves, from a wedged one, whose does not. Whatever ends the close, its last line on
  standard output is `job-close: ended, exit <code>`, so a caller reading its log needs no process listing to
  know it has ended.
- **Every wait inside a close has a clock the code names.** Each request writing the item's labels waits ten
  seconds and the writing as a whole two minutes; each local `git` call one minute, the rebase among them, and
  the fetch and the push five minutes each, after which it is ended and read as failed; the check's silence
  fifteen minutes, or five more than the checkout's declared testbed silence, and the pause before the KILL ten
  seconds; the running check is looked at every second. The wait for the close line alone has none (above).
- **Told to stop, a close ends the check it runs first,** every process under it as for a hung check, and lets
  its close line go, so nothing it started goes on after it.
- **What the close says is the account of its close.** A close that ends at the merge prints, before its last
  line, the commits it put on main, each as its short commit and subject, and the check that passed with the
  base it ran from. That is what a job's report says of its close: what changed and what was checked, and that
  the commit waits for its repository's batch deployment.
- **How it ends:**

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | The check passed and the commit stands on main, its repository deploying | `merged:` with the commits, `checked:` with the check, and `waiting for the batch deployment: <commit> stands on <remote>/<main>` | | 0 |
  | The same in a repository whose `testbed.toml` reads and declares no `[deploy]` (a base library, released and never put live) | `merged:`, `checked:`, and `on main: <commit> stands on <remote>/<main>, and this repository deploys nothing` | | 0 |
  | The record already names the commit as live | `this commit is already what is deployed` | | 0 |
  | A reason, a commit message, the remote, the close line, the rebase, the check or the push refused it | | `job-close: refused —` and why | 1 |
  | The run was told to stop | | one line saying so | 130 |

### Several checkouts in one close

A job whose work spans repositories closes every checkout it changed in one close, each named by a `--checkout`.
Each checkout is closed by the rules above, its main, its range, its record, its repository's name and its pair
its own, except where this section says otherwise: their close lines are held together, one check runs for them
all, and their pushes follow an order.

```mermaid
flowchart TD
  A[Reasons read] --> B[Every checkout read: remote fetched, messages, push order]
  B -- any refused --> X[Refused, nothing checked]
  B --> L[Every close line taken at once]
  L --> R[Each fetched and rebased where its main moved]
  R -- a rebase refused --> W[Refused, nothing checked]
  R --> C[One check over the checkouts that owe one]
  C -- failed --> Y[Refused, nothing pushed]
  C --> D[Pushed one by one, base libraries first]
  D -- a push refused --> Z[Stops there, the pushed ones stay]
  D --> E[Each repository's pair written after its push]
  Z --> G[Every close line let go]
  E --> G
```

- **Every checkout is read before anything is checked.** Each path names a checkout by its top level as git names
  it; a path that names none, and two paths naming one checkout or two checkouts of one repository, are refused.
  Then each checkout, in the order given, is read as the close of one checkout reads it: a commit its record
  already names as live is left out and said so; its remote is fetched; and no message of its range holds Hangul.
  The push order is read then too (below). Every refusal of every checkout is said at once, each line opening
  with its repository and naming the command that mends it, and nothing is checked, pushed or written on the item.
- **A checkout already closed is not checked again.** Where a checkout's `HEAD` already stands on its main and the
  item `--todo` names carries that repository's pair as `waiting-batch` with that very commit, an earlier close
  checked it and pushed it: it is neither checked nor pushed again, and its pair stays. Any other checkout whose
  `HEAD` stands on main, one the job pushed itself or one closed with no item, is checked over the range its
  record gives, as the close of one checkout checks it. Neither takes a close line, having nothing to push.
- **Every close line is held before any is used.** Each checkout with commits to push stands in its repository's
  close line, all of them asked for through one `close-line` of bmo-testbed's, its checkouts each named, and held
  from before the first rebase to the end of the last push. The testbed takes several lines in the one order it
  keeps for every holder of several, so two closes asking for lines they share, whatever order each names them
  in, never each hold a line the other waits for. Nothing is rebased, checked or pushed until every line is the
  close's, and the wait for them has no clock, as for one checkout's line.
- **Each checkout is rebased inside the lines,** in push order, as the close of one checkout rebases it: fetched
  again, and its commits rebased onto its main where that main no longer stands under `HEAD`, each rebase said in
  one line opening with its repository. A rebase refused, by uncommitted changes or a conflict, refuses the close
  naming that repository, and nothing is checked or pushed; a checkout rebased before it stays as rebased, on
  top of its main.
- **One check runs for every checkout that owes one:** bmo-testbed's change check over several checkouts,
  `bmo-testbed check --checkout <path>@<base>` once for each, every checkout's own base given as its commit, its
  main as it stands after the rebase where it has commits to push, run in the directory the close runs in
  (bmo-testbed's `docs/checks.md`, 「The change check over several checkouts」). It is watched as the check of one checkout is, its minute line naming it `the combined check`, and
  is handed `BMO_TESTBED_NEXT` where the close holds any line.
  Its silence limit is the largest any named checkout's declaration gives, `[test] silence_seconds` or the
  testbed's ten minutes where one declares none, and five minutes more. Where no checkout owes a check, none
  runs.
- **A check that does not pass pushes nothing.** The close is refused with the lines the check named its failed
  repositories and steps with (`check FAILED: <repository>: <steps>`), else its last line, and every pair the
  close wrote `testing` is taken off. A check ended for falling silent says, for each checkout, whether it stands
  as the close found it.
- **The pushes follow the base libraries.** A checkout whose repository another checkout's `testbed.toml` lists in
  `[check] base_libraries` is pushed before that checkout; the rest go in the order given. Declarations naming one
  another in a ring are refused before anything is checked.
- **Each repository's pair is written once its push lands:** `waiting-batch` with its commit, or taken off where
  its repository deploys nothing, and the summing pair written again from every pair the item then carries. While
  the check runs, each checked repository's pair is `testing`, all written at once.
- **A push the remote refuses stops the close there.** Every repository pushed before it stays on its main with
  its pair; the refused one's pair, and the pairs of those after it, are taken off; the close is refused naming
  the repository, what git said and that the close is run again with the same checkouts. Run again, every
  checkout the first run pushed is already closed (above), so only the rest are rebased, checked and pushed.
- **Every close line is let go once the close ends,** however it ends: after its last push, at a refusal, or told
  to stop.
- **Told to stop,** the close ends its check as above, lets go of its close lines, takes off every pair it wrote
  `testing`, and writes once every other pair whose writing the stop cut short.
- **What it says** is the account of each checkout, every line opening with its repository and `: `: `merged:`
  with its commits, `checked:` with the check and its base, and what it waits on, the lines the close of one
  checkout ends with, or, for a checkout already closed, `already waits on <remote>/<main>` with its commit. Its
  last line is `job-close: ended, exit <code>`, and the codes are those above.

## `bmo-job job ask`

`bmo job ask` runs this. It is what a job's own run waits on: nothing ends the wait but what is listed here. The
only clocks are the service's: the supervisor's question wait, after which the service puts the question to the
owner, and the owner's, after which its recommendation settles it (docs/api.md, 「The due time」).

A job asks its supervisor, never the owner directly. The question is opened in the item's room as the
supervisor's (docs/api.md, 「The addressee」) and handed to that room's supervisor at once, which judges what it
is: it does or finds what is asked and settles the question with the result, talks it over with the job, or puts
it to the owner in its own words, with two choices or more and its recommendation, and from then on it is the
owner's question as any other. The supervisor's move puts a job's question before the owner, and so does the
service once the supervisor has held it for the question wait, a minute unless the service's settings name
another, with neither settling it nor putting it to him: nothing the verb does puts it there.

| Form | What it does |
|---|---|
| `--todo <item> --question <words> --why <words> [--option <words> --option <words> --recommend <n>]` | Opens the question for the supervisor of the item's room (below), hands it to that supervisor and waits |
| `--talk <question> --say <words>` | Puts the job's answer to the supervisor's or the owner's talk in the same room, then waits on the same question |
| `--talk <question> --say <words> --option <words> --option <words> --recommend <n> [--question <words>] [--why <words>]` | The same, with the question put again in other words |
| `--talk <question> --say <words> --ack` | Writes words that ask nothing on the question, an acknowledgement such as that the job waits, hands them to nobody and waits on the same question |
| `--show <question>` | Reads one question by its number and waits on nothing |
| `--todo <item> --show` | Reads every question asked about the item and waits on nothing |

- **How it ends:**

  | What happened | Standard output | Standard error | Exit |
  |---|---|---|---|
  | The supervisor settled it | Its answer, as it wrote it | | 0 |
  | A person settled it, once it was put to him | One line: the option's words, or his own | | 0 |
  | The owner's due time passed and the question was settled on its recommendation | One line: the recommendation's answer | `ask: question #<n> was settled on its recommendation, the owner's time having run out` | 0 |
  | Somebody settled the same question before this run | The same answer | `ask: question #<n> was settled before; its answer stands` | 0 |
  | The supervisor or the owner talked instead of settling; the question stays open | Their words, each under the voice that said it, and the call that answers them | | 3 |
  | The question could not be put, the receiver refusing it; it is closed `unsent` | Nothing | Why | 1 |
  | The question was withdrawn, or the run was told to stop, which withdraws a question already opened | Nothing | Why | 1 |
  | The words were refused; no question opened | Nothing | Why, and the usage | 1 |

- **A question asks for what the job cannot settle or do itself,** said in words read at a glance: a decision
  that is not the job's, or something only the supervisor's reach grants, such as a command on a node the job's
  account does not reach or a deployment the work cannot go on without. A choice puts two options or more with
  the one the job would take; a request the supervisor answers in its own words puts none. Either way it says
  why the job cannot settle it itself.
- **It is written for someone who knows nothing of the work,** since the supervisor may put it before the owner:
  what the work is, what each option does and how the options differ, and what follows under each, with no step
  number, internal name or term left unexplained. An option says what it does in words of its own, so the owner
  can take it without reading anything else.
- **What is refused.** Its words are refused, and no question opened, where any of them carries a fenced block of
  code or a token of more than 80 characters with no space — a script, an encoded payload, a hash or a long
  address — or where the question runs past 500 characters or 5 lines, an option past 160 characters, or the
  reason past 240. An option is refused where it is a bare label or a step number, said in fewer than two words
  of two letters or more: `1`, `B`, `Option 2` and `Step 12` say nothing to someone who has not read the rest. A
  long reading goes into the archive and its address into the question. The job's answer to a talk is refused
  on code the same way, its length its own.
- **An answer comes back as it was given,** read by the voice the service heard it in (docs/api.md, "Questions
  and replies"). The supervisor's is printed as it wrote it, a result of several lines among it; a person's is one
  line, the option he took or his own words. A recommendation settles the question where the owner's due time
  passes with no answer from him, and comes back as his would, one line, with a line on the standard error saying
  it was the recommendation.
- **Putting the question.** The verb finds the room the question goes to, opens the question there for the
  supervisor with `toldByAsker` (docs/api.md, "Questions and replies"), and hands it to the room's supervisor
  through the receiver's `POST /instructions`, an order under the sort `job` of the kind
  `question-for-supervisor`. An answer other than a queued instruction is a failed send. A question that is
  settled, put to the owner or stopped has its instruction withdrawn where it has not reached the supervisor yet.
- **The question's room is the item's.** It is the item's own `scope` where that reads as a room's number, an
  integer other than zero; else the scope of the item it was raised from, read the same way, following
  `raisedBy` up from item to item, at most 16 items, never past an item met twice nor one the service refuses
  to read; else the owner's private room, which the receiver's `GET /owner` names. The room is the question's
  `scope`, so every notice of it, a wake, a talk and a withdrawal, goes to that same room.
- **What a turn carries.** An order carries the notice's kind and number, the item's number and title, and the data
  it is about: the question, its options numbered with the recommended one marked or the word that it is open, and
  why it is asked; or the job's words in a talk, with the question's words and options again where the talk changed
  it. An order for the supervisor also names the tools that settle the question, talk on it and put it to the
  owner (docs/api.md, 「The tools」). It carries no other instruction, since how a notice is read stands in the
  receiving session's own instructions.
- **While the supervisor holds it,** the question's due time is the service's question wait from the moment the
  supervisor was given it, and no word the supervisor says stops it; the supervisor's notice is read through the
  receiver's `GET /instructions/{id}` as the verb waits. The receiver keeps a notice with no expiry and holds it
  while the supervisor's session cannot be reached. Where the wait passes with the question still the
  supervisor's, the service puts it to the owner as it stands, with the job's recommendation or none, and the
  verb waits on his answer as below.
- **A supervisor that did not take the question up is woken again,** never passed over. It did not take it up in
  two cases, both events the receiver states:
  - the notice was let go untaken — its round stopped, its session reset, no turn able to take it — which the
    receiver answers as gone;
  - the notice reads `delivered`, which the receiver writes only once the round that carried it ended after its
    turn ended, and since the notice the supervisor said no word on the question: it did not settle it, talk on
    it or put it to the owner.

  The verb then rests, and where the supervisor still has not taken it up it hands the question to the room's
  supervisor anew as `question-for-supervisor`, with the count of the wake, and watches that notice as the first.
  The first rest is a minute, or the milliseconds the run's `BMO_JOB_ASK_WAKE_MS` names, and each rest after it is
  twice the one before. Past the sixth wake it wakes nobody: it says so on the standard error once and waits on the
  question as it stands. A question the service has put to the owner wakes the supervisor no more.
- **A supervisor's word is taking it up.** A word that settles nothing — a talk, or a word that it settles later,
  as while a deployment it called runs — keeps the question with the supervisor until its question wait passes,
  and nothing wakes it again until the job talks again. The job's acknowledgement (`--ack`) is written on the question and handed to nobody: it
  asks nothing, so it wakes nobody and leaves the watch where it stood. It changes no words of the question, so it
  takes no `--option`, `--question` or `--why`.
- **The notice is kept on the question,** as its `toldAs`: the notice's number, the last reply before it and how
  many times the supervisor was woken again. An ask that stands on a question asked before reads the notice kept
  there, so a rerun job watches it, and counts its wakes, as the first run did. The job's answer to a talk on a
  question the supervisor holds is a notice of its own, `question-talk`, kept the same way, and the rules above
  hold for the turn it wakes.
- **Once it is the owner's,** put to him by the supervisor or by the service, the verb says so on the standard
  error once, naming which, and waits on his answer: the due time the service counts from that moment, the
  question wait or a shorter one the supervisor's move named, settles the question on its recommendation where
  nobody has answered by then
  (docs/api.md, 「The due time」 and 「When the due time passes」). The job's talk on it goes to the room's
  supervisor as `question-talk`, which puts it before him; an acknowledgement goes nowhere.
- **Taken back by the supervisor** while the owner has not settled it (docs/api.md, 「The addressee」), it is the
  supervisor's again, for a new question wait: the verb says so on the standard error once and waits on it with
  no notice to watch, since the supervisor took it itself.
- **A restart is no answer.** Every call the verb makes, reading the item's questions and the answer one was
  settled with, reading the item and the items it was raised from, reading the owner's room, opening the
  question, reading the item's title, handing the order or the talk over, reading the order, keeping the job's
  words and where it heard through, closing the question, and every read while it waits, is made again while the
  service or the receiver does not answer: nothing at its address, a connection cut, or a 5xx. It is made again
  after a quarter of a second at first and every five seconds at most. The run told to stop ends it as above.
  Only an answer ends a call: while the question is put, a refusal closes it `unsent`; while it is waited on, only
  a question the service no longer holds ends the wait, with 1.
- **One question, one row, told once.** Before anything is opened, the item's questions are read, and the last
  one of the same asker, words and options that is not `withdrawn` or `unsent` is the question this ask
  stands as. One `open` or `paused` is waited on as it stands, told to nobody again, and the standard error
  says so with its number. One `answered` comes back with the answer it was settled with, its number on the
  standard error, and nothing is opened or told. Only where every such question stands `withdrawn` or
  `unsent`, or none was asked, is a new question opened and handed to the supervisor.
- **At least once.** A call cut after its peer took it is made again, so an order or the job's words in a talk
  can reach the room twice. A question is opened once: where opening it was cut, the item's open questions
  are read before it is opened again, and one of the same asker, words and options is taken as the one opened.
- **Reading a question.** `--show` prints each question as JSON, one a line: the question as the service keeps
  it (docs/api.md, "Questions and replies") with `answer`, the answer it was settled with, or null. It reads
  the service once, opens nothing and tells nobody; a question or an item the service does not hold ends it 1
  with why on the standard error.
- **Pages hosted elsewhere are refused:** an address of a claude.ai artifact, a published Claude page, a gist or a
  paste in the question, an option, the reason or the words of a talk.
- **Where it reaches the receiver:** `BMO_RECEIVER_URL` with `BMO_RECEIVER_TOKEN` or `BMO_RECEIVER_TOKEN_FILE` in
  the run's environment, else `RECEIVER_ADDRESS` and `RECEIVER_TOKEN` in `env/receiver.env` under the data root.

## `bmo-job todo`

The worker's run already names the job service in `BMO_JOB_URL` and its token in `BMO_JOB_TOKEN` or `BMO_JOB_TOKEN_FILE`; a shell with neither reads the deployment's `settings.toml` beside the binary, or the file `BMO_JOB_SETTINGS` names.

| Form | What it does |
|---|---|
| `--done <item> [--report <text>] [--unfinished <text>]` | Leaves the run's account of the finished work on the item: `--report` kept as a `final` report and `--unfinished` as a `held` one through `POST /items/{itemId}/reports`, the `held` one with the label `account` at `unfinished`, which tells the unfinished note from a hold's reason and a `--stop-here` account, both `held` reports with no labels. It moves no state: the item is done once whoever ordered the work takes it — its review, or, for an item whose commit waits on main for its repository's batch deployment (a label `close-phase:<repository>` reading `waiting-batch`, 「`bmo-job job close`」), whoever puts every such commit live. The verb prints one line saying what the item waits on: `deployment` with `commit`, the commit its label `close-commit` names, and `commits`, each waiting repository with its commit, `review`, or `orderer` |
| `--stop-here <item> --report <text>` | Leaves the run's account on the item as a `held` report, for the run that picks the work up: what is finished and where the next run goes on. It moves no state; holding the item is its orderer's |
| `--add --title <words> --judgement <words> --raised-by <item> [--repository <name>[,<name>]]… [--from-todo <id>:<strand>[,<id>:<strand>]]` | Raises a draft in the queue and scope of the item it was raised from, the judgement as its brief, the first `--repository` as its assignee (the repository its work is in), else the assignee of the item it was raised from, and `--from-todo` as its label `from-todo`, and prints it. Where its work spans two repositories or more, named by `--repository` given again or by names joined with commas, the whole list, in the order given and each name once, is its label `repositories`, joined with commas |

- **`--by <name>`** names who writes it: the run's `BMO_TURN_AGENT` where left out, then `USER`.
- **Pages hosted elsewhere are refused** in a `--report` and an `--unfinished`, as in a question.
- **Each answer is printed as JSON,** one a line. The verb reads no database of its own.
- **A run never moves its own item's state.** Whether the item is done, held or open again is decided by whoever
  ordered the work, through the service's routes; the verb leaves the run's words on the item and nothing else.

## `bmo-job mcp`

| Option | What it gives |
|---|---|
| `--service <url>` | The base address the service's routes stand at; required |
| `--token <token>` | The bearer token every call is made with: the shared token, or a caller's own, under whose name the service records an approval the server gives (docs/settings.md, `[callers]`); read from `BMO_JOB_TOKEN` where it is left out |
| `--holder <name>` and `--queue <name>` | A holder name and the queues it is told of from the start, `--queue` given once for each; each needs the other |
| `--channel <capability>` | The experimental capability each event is also pushed on as a message |
| `--without <tools>` | Tools the server leaves out, their names comma-separated, the option given once or more; read from `BMO_JOB_MCP_WITHOUT` where it is left out. A tool left out is neither listed nor called; a name that is no tool of the plugin refuses the start |

The server ends when the harness closes its input, and then gives back the leases and deletes the
subscriptions it kept, waiting for the service to answer each.
