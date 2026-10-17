# A job opening a real Claude session, and the hook in front of a job's shell

The shell hook this service runs before every command and every file edit a job makes (`job guard`,
docs/verbs.md, "The shell hook") holds the line on a job spending a real Claude session without saying so,
and on the other things a job's shell must not do: change the structure document, remove through an empty
variable, stall on its own habits, run a heavy command outside the testbed, write past the brain's editors,
and write in another worker's checkout. What a job's file writes inside its own checkout are held to is
docs/job-tools.md.

## What it costs and who may open one

- **The cost:** opening a new Claude session, copying a session's record to carry it on as a new session, or
  putting words into a program in a pane makes that session write its uncached prefix and conversation from
  the start, which on a long conversation is over a hundred thousand tokens of cache writes.
- **The service's own sessions** — every job's turns, carried by bmo-agency — are opened knowing that cost.
- **A job's own sessions,** for a measurement or an experiment, are opened only once the job has said what it
  is opening them for. The owner is not asked: the job opens the guard itself, and what it ran, and how many
  times, is kept and carried into the item's report.
- **Code holds the line, not a sentence.** It stops a job from spending without knowing it, not a job that
  knows.

## Which processes are a job's

- **A job's process carries two marks,** which every process its program starts as a tool inherits: the
  session mark `CLAUDECODE`, which the program itself sets, and `BMO_TASK_LOCKED`, set where the program runs
  a job's turn; and the run's name in `BMO_RUN_NAME`, which is the job's unit (docs/verbs.md, "Where a verb
  stands"). A process carrying both marks is a job's, and its job is the open job whose unit is the run's
  name.
- **A tool run another unit starts** — a search, a browse, speech — receives only the variables picked for it
  by name, inherits neither mark, and is not what this guard is about.

## The hook

- **Where:** the worker's settings, which bmo-agency keeps with the worker's definition, run the hook before
  every shell command and every file edit of a job, its input on standard input. A refusal comes back with the
  reason and the piece of the command it was read in — the program word, a redirect's target, the text handed
  to `eval` or a shell — so a job reads what was blocked and why instead of guessing at the rule.
- **How it reads a command:** as the shell splits it (docs/runs.md, "A command the runner allows itself",
  reads commands the same way): each word's quoting as written, a heredoc's body as the text it carries; a
  variable the command assigned itself read as its value; the text handed to `eval`, `source`, `.`, a shell
  reading standard input or a command substitution read as commands too; a backquote a substitution only where
  the shell opens one, so nothing quoted in a report or a commit message is read as a program. A line the
  reading cannot parse reads as no command at all.
- **When the hook cannot run** — no input, the binary missing or failing — nothing is blocked: one hook
  failing must not stop every shell command of every worker.
- **A command let through runs with its standard input closed:** the command is handed to the shell with its
  input on the null device, so a reader given nothing to read ends at once instead of waiting with no end,
  while a pipe, a redirect, a heredoc and a here-string still feed the command they stand on.

## What the live-Claude guard refuses until it is opened

1. **A command that writes into the program's session records:** every login's records directory, as
   bmo-agency names the logins' homes, spelled absolute, from `~`, from `$HOME` or from `${HOME}`, and the
   program's own configuration directory word. Among commands that mention them: those that copy, move, link,
   delete or create, edit in place, or redirect anywhere but the null device or a literal path outside every
   login's home. A target written as a variable the command did not assign could be anywhere, and counts as a
   write there. Reads pass.
2. **Starting the program itself,** however it is written: as a stage of a pipeline, behind an assignment or a
   runner (`env`, `command`, `exec`, `timeout`, `nohup`, `xargs`), as a path, inside a shell's `-c` string, or
   as one slot of a command line written in code. A path holding the configuration folder, or the word as an
   argument, is no start.
3. **Words put into a pane's session:** code that calls the pane's prompt, and driving an agent's pane with
   the pane multiplexer's command.
4. **The flag that forks a session.**

- **Print mode is never let through,** whatever the job opened: no agent of this system runs the program that
  way.

## A job opens it by saying what for

- **The way in:** the refusal names it. The job runs `job live-claude --purpose '<what it measures and why>'`
  and goes on; nobody is asked and nothing waits (docs/verbs.md, "The live-Claude guard").
- **What it lets through:** the interactive program, a fork, a write into the records and words put into a
  pane.
- **Scope:** the opening stands for the one job whose run name it was made under. The stages of a work order
  are separate jobs, so each opens it for itself. A process outside a job has nothing to open, and the verb
  refuses it.
- **Every run it lets through is recorded** against the opening, as a row of `live_claude_runs` under the
  opening's row of `live_claude_openings`: the command and when.
- **The count reaches the report.** `job live-claude --list` prints what the job opened and ran, and closing
  the item adds one line under its report for each opening any of the item's jobs made — its purpose and how
  many runs it let through — so the record says what was spent even when the job's own words leave it out
  (docs/rows.md, "The closing's readings").
- **Behind the hook:** a script file the hook cannot see into is held by the guard in the code that starts the
  program, which is bmo-agency's.

## The hook holds the structure document

- **What it guards:** `STRUCTURE.md`, the owner's statement of the system, which a job changes only on his own
  request.
- **What it refuses:** a file edit whose target is a file of that name; and a shell command that writes to it
  — a redirect into it, a writer program or an in-place editor given it, `git rm`, `mv`, `checkout`, `restore`
  or `apply` on it, and an interpreter's inline script whose write names it or that names it and writes
  somewhere the reading cannot resolve — inside `bash -c`, `zsh -c` and `eval` too. Reading it passes.
- **Who may change it:** a job whose item the owner requested himself and whose title or instruction names it,
  found by its run name. Nothing else opens it.

## The hook refuses a removal the program would stop on

- **Why:** the program asks a person before some removals even in bypass mode, and a job's turn has nobody to
  answer; a removal through an empty variable is also a real danger, reaching the root.
- **What it refuses:** an `rm` or `rmdir` — behind assignments and runners, inside `bash -c`, `zsh -c` and
  `eval`, on any line of the command — one of whose targets holds a command substitution, or a parameter
  expansion that does not stop on an empty value (anything but `${NAME:?…}` or `${NAME?…}`). Text in single
  quotes is literal.
- **What passes:** a literal target, a guarded one, and a removal whose targets come from elsewhere (`xargs
  rm`, `find … -delete`).
- **Nothing opens it.** The refusal says how to write the removal instead.

## The hook refuses the shell habits that stall a job

- **Why:** a job's shell is a fresh `zsh -c` for every call, its input closed, and zsh refuses a glob that
  matches nothing. Each habit below is a command that runs for ever, runs past the call that started it,
  answers nothing, or never runs at all, and the refusal comes the moment it is written, with the way to write
  it.
- **What it refuses,** read as the shell splits the command and never inside a heredoc body:
  - **a known long command with no wait asked for** — a close, a whole suite, a testbed run, `job ask`, `job
    land`, `git push` — in a call whose wait is under five minutes and which does not run in the background;
  - **a reader named nothing to read** — `ls` with no path, `cat`, `head`, `tail`, `wc`, `sort`, `grep`,
    `sed`, `jq`, `tr` and the like with nothing feeding them — which on the closed input answers nothing, and
    an empty answer reads as one that found nothing;
  - **a `-f` match that finds its own shell** — `pkill -f`, and `pgrep -f` other than a listing — since the
    pattern stands in the shell's own command line;
  - **a pattern the command expands for itself, left unquoted** — an `--option=value` word, or the value after
    `-name`, `--include`, `--glob` and their kin, holding `*`, `?` or `[` outside quotes;
  - **a sleeping wait with no bound** — a `while` or `until` loop that sleeps, unless it runs under `timeout`.
- **Nothing opens it.**

## The hook sends a heavy command to the testbed

- **Why:** one heavy test or build command holds the machine at a time, and a job's shell is where such a
  command starts, whatever checkout the job works in.
- **What it refuses:** a command one of bmo-testbed's languages claims as heavy, as bmo-testbed's `classify`
  answers it (docs/api.md, "Heavy commands"), that is not itself a run of the testbed. It answers with the
  line that runs it there.
- **What passes:** a light command, the testbed's own run, and the close, which takes its turn itself.
- **Nothing opens it.**

## The hook sets the brain hold

- **What it does:** a job's first write into the brain's directory (`[host] brain`, docs/settings.md) holds
  the brain's editors off through bmo-brain, so no editor starts a batch until the job's run ends
  (docs/api.md, "Brain holds").
- **What counts as a write there:** the same reading as the structure document's, a git command that changes
  files among them, and relative paths after a `cd` into the brain.
- **What it refuses:** that write, only while an editor holds its lock, with the way to wait for it.
- **When it cannot tell** — a process with no run name, a brain bmo-brain does not answer for — it sets
  nothing and refuses nothing.

## The hook keeps a job in its own checkout

- **What it guards:** every registered worker's checkout — each base and each support worker — but the job's
  own worker's and the ones the job borrowed (docs/worker-slots.md, "A job writes only in its own checkout").
- **What counts as a write there:** the same reading as the brain's. A path is matched at a boundary, so a
  support worktree whose path begins with its base's is never read as the base, and `~` and `$HOME` spellings
  count.
- **What it refuses with:** whose checkout it is, and `job borrow <worker>`, which lends the job a free
  checkout of that repository.
- **When it cannot tell** — a job whose row or worker it cannot read — it refuses nothing. The job's own
  checkout is also what its file-write refusals read as the checkout (docs/job-tools.md).

## What the guard is not

- **A wall against spending without knowing, not a sandbox.** It cannot stop code written to get round it, and
  getting round it is spending without the record the opening keeps.
