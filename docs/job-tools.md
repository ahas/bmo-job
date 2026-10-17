# What a job's shell is held to

The shell hook this service runs in front of every command a job's program runs (`job guard`,
docs/verbs.md, "The shell hook") holds three of a job's habits: how it writes a file of its own
checkout, what its commits may carry, and how a graph file is written. The verbs it hands back in their
place are the platform's own command line: `bmo edit`, the file-editing verb, and `bmo graph`, the code
graph's, which other units carry. This document also says how this service's own verbs answer.

## What a job's shell may no longer write with

- **The hook refuses three ways of writing a file that lies inside the job's checkout:**
  - an interpreter handed an inline script — by a flag or on standard input — that writes a file;
  - an in-place editor: `sed -i`, `perl -i`, `ruby -i`, anything carrying `--in-place`;
  - a redirect into a file from one of the text tools — `cat`, `printf`, `echo`, `tee`, `sed`, `awk`, `jq`,
    `tr`, `cut` — where the text put there is the command's own: a heredoc, or the words of an `echo` or a
    `printf`. A tool reading one file and writing another is a transform, which no verb does, so it stands.
- **A relative path is one of the checkout's files,** because a job's shell stands in its checkout. A `cd`
  out of it is followed, and after one a relative path is somewhere else and nothing about it is refused; a
  path spelled with a variable, a substitution or a `..` could be anywhere, and what could be anywhere is
  left alone. The job's own checkout is the one its worker's row names, and the ones it borrowed.
- **A file under the graph's directory hands back the graph's own call** rather than the edit verb,
  whatever was being done to it, since a graph file is written through the graph tool alone ("A graph
  file" below).
- **The refusal names the verb and the whole call.** It says in one line what the command was doing and
  hands back the one line that does it: a phrase swap as `bmo edit <file> --replace '<old>' '<new>'`, a
  block as its `--stdin` form, a substitution as `--replace-regex`, a slice between anchors as `--between`,
  lines named by number as `--replace-lines` or `--delete-lines`, text put beside an anchor as
  `--insert-after`, an append as `--insert-at`, a whole file as the program's own file-writing tool. The
  mapping is read off the command, never a fixed sentence.
- **A refusal that cannot name a verb is not made.** Where the reading cannot say which verb does the
  work, the command runs: a job stopped with nowhere to go is a worse fault than a habit left standing.
- **A write outside the checkout is never refused,** neither is reading, nor a pipeline that writes
  nothing, nor a test run redirected into a log. A committed script is not an inline script, and the
  program's own file tools are untouched.

## What a job's commits may carry

- **The hook refuses Hangul going into git** where the command line shows it: a `git commit` or `git tag`
  whose `-m` message, or the heredoc fed to `-F -`, holds it, and a new tag, a `git branch`, `git switch
  -c` or `git checkout -b` whose name does. The names of address the close's policy sets aside pass
  (docs/policy.md, `close.toml`).
- **The refusal says the rule,** that what goes into git is English, and the command is written again in
  English, the owner's quoted words translated.
- **A message read from a file is left to the close,** which refuses the commit by name before it lands
  (docs/jobs.md, "A job closes").

## A graph file

- **A node of the code graph is not a phrase in a file:** the graph's files are written through the graph
  tool, which validates and serialises them, and `bmo graph update <node> --replace '<before>' '<after>'`
  is the swap there. bmo-graph owns the graph and says what that verb reaches and refuses.
- **Never write a graph file with the edit verb** or with anything else: a file written past the graph tool
  is one whose faults only a later check finds.

## What this service's verbs answer with

- **In English,** whoever runs them: their lines, refusals and usage. What a verb quotes back stands as it
  was written — a phrase handed in, a row out of the database, a domain term in another language.
- **A coded answer where its area has codes** (docs/verbs.md, "What a verb answers"): the command line
  prints a code as its JSON on one line and a refusal as its sentence (docs/cli.md, "What it prints").
- **Thin by default, thick when asked.** A verb answers with the fewest lines that answer the question; a
  caller that wants the structure asks for it with `--json`, and no verb carries a third width between the
  two.
- **A wrong word is named, not the whole usage:** a verb handed the wrong thing names that one thing and
  points at its usage.
- **A reading too long for its lines says so out loud,** and how to ask for the rest: a work order's lines
  past their ceiling are cut, with the line saying to ask for it whole with `--json` (docs/verbs.md, "A work
  order read and changed"). A short answer read as the whole answer is the one failure this must not have.
