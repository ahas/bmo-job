# Support workers and their worktrees

How one repository runs several jobs at once: the base worker, the support workers beside it, the
worktree each works in, the branch each job works on, how a job's work lands on main, and what a job may
write outside its own checkout. The worker rows and their rules are docs/queue.md, "Workers"; the runner's
side of a checkout is docs/runs.md, "The checkout before and after".

## Two kinds of worker per repository

- **The base worker** is the worker row the owner added, or the built-in one. It works in the checkout its
  row names. Between jobs that checkout stands on main; while a job of a repository with support workers is
  at work in it, it stands on that job's branch (「A job in the base checkout」).
- **Support workers** are further workers of the same repository, each a row of its own marked as the base's
  support and numbered from the policy's first support slot. They let the queue run more than one item of
  one repository at a time.
- **Two settings are the base's:** how many support workers it keeps, from nought to the policy's most
  (`workers.support_most`; nought runs one job at a time), and how many of them are kept warm, never more
  than that count. Keeping a worker warm is bmo-agency's.
- **Where they are changed:** the worker's settings through the server's `POST /worker` (docs/api.md,
  "Workers"), from the console or `worker update <base> --support-count <n> --support-keepalive <n>`. A change
  settles the rows at once: missing support rows are written, and rows past the count are removed once no
  job holds them.
- **A support row is the base's, not the owner's to edit on its own.** Its name, label, directory and dev
  command follow from the base; it is counted up and down through the base alone. A TODO item is never
  assigned to one: an item's worker is the base, and the base's line is its supports' line.

## Names and places

- **Name:** `<base>-w<n>`, `n` counting from the first support slot — the base is the first of its repository.
- **Label:** the base's label followed by `#<n>`, rewritten to it whenever the base is settled.
- **Directory:** a git worktree of the base's repository at the base's own path followed by `-w<n>`.
- **The path is fixed for the life of the support worker.** The directory is part of every session's prefix,
  and a session is kept under its directory, so a path that moved per job would start every session cold.
- **The worktree is made the first time a job lands on it,** from the base's main, and set up the way its
  repository sets a checkout up: the submodules its commit pins brought to their pins, then the repository's
  own setup script where it names one (`checkout setup`, docs/verbs.md, "A checkout set up").
- **The setup runs again whenever a checkout's files may have moved under its installs:** whenever the runner,
  a work order or a borrowing puts a checkout on a job's branch, and whenever a close has rebased a branch
  onto main. A setup that fails stops the job from starting, or fails the close, and says why.
- **Each support worker has its own workbench** under its own name, which bmo-agency keeps.

## A job on a support worker

- **Every job works on a branch of its item,** made from main in that worktree: `work/<item>`, or
  `work/job-<job>` for a job with no item. The worktree never has main checked out.
- **Main is the branch the base checkout stands on,** read off the base checkout the base's row names, never
  off the support worktree. Where the base checkout itself stands on a `work/` branch, main is the branch that
  branch was put on from, written down beside it in the repository's git configuration; where nothing is
  written down, the remote's own head, failing that `main`, then `master`.
- **Making a job's branch writes down the branch it was made from,** once, as it is made; the landing checks
  the branch against it (「Landing on main」).
- **Before the job's first turn the runner puts the worktree on that branch:** what the last job left open is
  committed onto the branch that job was on, which is how a job is parked — where the worktree is already on
  the job's own branch, what is open is that item's own and stays open; a branch already merged into main is
  deleted once the worktree has moved off it; the item's branch is taken up where it exists, so a rerun takes
  up where the last run stopped, and made from main where it does not; and the setup runs.
- **The job commits on its branch and does not push it.** The close lands it.

## A job in the base checkout

- **A repository with support workers keeps main out of every job's hands.** An ordinary job the base worker
  runs in the base checkout works on its item's branch there too, and lands the same way. A repository with no
  support workers has nobody to land beside it, and its base job works on main as it stands.
- **Why:** a landing moves main, and a checkout standing on main with a job's open files in it is one main
  cannot be moved in without refusing or overwriting that job's work. With the base job on a branch, main
  stands checked out nowhere while it works, and every landing moves the ref alone.
- **As the job starts,** the base checkout goes on the branch: from main, what is already open there — a
  person's own edit — is carried across as it stands, somebody else's to the job; from a `work/` branch an
  earlier job was left on, what is open is parked on that branch first; then the setup runs.
- **As the job ends, before its report goes out,** the base checkout goes back on main: what the job left open
  is committed onto its branch, main is checked out as the landings left it, the branch is deleted where main
  holds all of it, and what was open before the job began is carried back as it stands. A switch that cannot
  be made is named in the report (docs/runs.md, "The checkout before and after").
- **A work order in the base checkout works on its item's branch too,** put on it as the order takes the
  checkout and back on main as it lets go (docs/work-orders.md, "One checkout, one work order").
- **A base checkout another repository's job borrows** is put on that job's branch and back the same way,
  where its repository has support workers.

## A worker of its own in a worktree

- **A worker row of its own may name a linked worktree of another checkout's repository** as its directory. It
  is no support worker: it keeps its own name, label, pin and line, and no base's settings count it.
- **Its checkout is its own:** its jobs hold that worktree's lock alone.
- **It works the way a support worker does:** main is read off the repository's main worktree; every job works
  on its item's branch, parked as on a support worker, and the worktree is never put back on main; everything
  a job leaves open there is its own; and the close lands the branch on that main.
- **The worktree is made once, by hand,** and the row pointed at it; nothing makes or removes it for the row.

## Landing on main

- **A job's close lands its branch,** run in a checkout standing on a `work/` branch, and `job land` lands one
  with no check of its own (docs/verbs.md, "Landing a branch" and "A job's close"). Every landing:
  1. rebases the branch onto main as it stands now; a conflict aborts the rebase and is refused with the files
     it stopped on, and resolving it is the job's own work; the setup then runs on the rebased tree;
  2. where it is the close's, checks the rebased commit, and checks it again where the rebase took in commits
     its check did not cover;
  3. refuses a commit of the job's own whose message is not English, past the names of address, naming each by
     its short hash and subject;
  4. pushes each submodule commit the landed commits pin anew to that submodule's own repository's main, then
     the commit to the remote's main as a plain fast-forward, and only once the remote has taken it moves main
     here and writes down what the job landed;
  5. where it is the close's, records the landed commit as waiting to go live through bmo-testbed.
- **The remote's main is what a landing wins.** A push the remote refuses moves nothing here and writes
  nothing down, and the landing takes another round: the remote's main brought in where the close follows it,
  the branch rebased again. A refusal on every round refuses the landing with what the remote said.
- **Where main is checked out, moving it moves that checkout:** the base checkout between jobs, fast-forwarded
  in place, refused before the push where the move would overwrite a file open there. While a base job is at
  work main is checked out nowhere, and the landing moves the ref alone.
- **A checkout moved by a landing is set up again where its installs moved,** and its submodules are brought
  to the pins main now holds. A setup that fails there does not undo the landing; the close says it failed.
- **The units linked into the checkout main stands in follow its files** through bmo-testbed's unit links
  (docs/runs.md, "Landing on main"): a move bmo-testbed holds — a link with no file of its name in the landed
  commit, or a unit running in a folder the commit no longer has — is refused with nothing moved, and the
  links it plans are relinked once main has moved.
- **A branch lands only on the branch it was made from.** Where the branch it was made from shares a commit
  with the job's branch that the branch being landed on lacks, the landing is refused and names both, since
  the rebase would carry that branch's commits across.
- **Two landings do not race.** A landing holds the repository's landing lock in `[host] locks`, and the
  closes of a machine land one after another through the close line besides; a close on a work node lands
  under the operations machine's lock (「Checkouts on the nodes」).
- **What the runner counts as the job's own commits** on a branch is what the landing recorded it landed and
  what is still on the branch past main.

## Checkouts on the nodes

- **A base may also have one checkout on each work node,** a support row named `<base>-<node>` with no number,
  added and removed one by one through the server's `POST /worker/node` and never by the support count
  (docs/cli.md, "`worker`"). Its directory is the base's own folder name followed by the node's, beside it,
  unless a path is given.
- **Its jobs work on their item's branch as every support worker's do,** made from the remote's main, which
  the runner brings up in that checkout first, every git question going over one SSH login to the node.
- **Its landing keeps the rules above:** the node's close or `job land` holds the repository's landing lock on
  the operations machine through `node-land`, takes the remote's main in before each round and pushes it, and
  the base checkout here follows (docs/verbs.md, "Landing a branch").

## Parking

- **Parking is leaving the branch.** A job that ends without landing leaves its branch where it is, and the
  next job on that worktree commits what it left open onto it before moving off.
- **A stopped work order on a support worker** commits everything open in its worktree onto its item's branch
  and lets the worktree go; nothing else writes in that worktree, so all of it is the order's. On resume it
  takes the worktree again once it is free.
- **A base checkout parks by time windows:** a stopped work order sorts out its own change there by its turns'
  windows (docs/work-orders.md, "One checkout, one work order").

## The queue with support workers

- **A repository takes as many jobs as it has free checkouts** — the base's, and each support worker's within
  the count — and the queue's standing counts the free checkouts of each base.
- **The launch picks the checkout:** a free support worktree already on the item's branch, then the base
  checkout, then a free support worktree by number (docs/queue.md, "Launching a job"). A launch meeting a held
  lock tries the next free checkout before it refuses.
- **A stopped work order at the head of the line is its runner's to resume** in the checkout it holds.
- **An item already at work is not launched a second time,** on any checkout of its repository.
- **Free checkouts are not room on the machine:** a launch with a free checkout is still held while bmo-agency
  says the machine is short of room.

## Dev servers

- **A worker's dev server stands only while a job of it runs and has asked for it.** The job starts, restarts
  and stops it with the platform's dev-server verbs, which the preamble names; the server itself is
  bmo-agency's.
- **A support worker's dev server is told its number:** it runs with `BMO_WORKER_SLOT=<n>` in its environment,
  so a dev server that binds ports moves them by that number.
- **The job's preamble says which worker it is:** a support worker's job opens with the base's preamble under
  a header saying it is support worker `n` of the base, in its own worktree and on its item's branch, with the
  lines that start, restart and stop its dev server. A base job on its item's branch is told the branch, and
  that the close lands it rather than a push (docs/runs.md, "The prompt").

## A job writes only in its own checkout

- **Every registered worker's checkout is someone's.** A job may write in its own worker's checkout and in a
  checkout it borrowed, and in no other worker's checkout — base or support — whoever holds it. A place no
  worker is registered at is not guarded by this.
- **The shell hook refuses the write,** from the file tools and from the shell alike, and the refusal names
  whose checkout it is and the verb that borrows one (docs/live-claude-guard.md, "The hook keeps a job in its
  own checkout").
- **Borrowing:** `job borrow <worker>`, run inside a job, takes a free checkout of that worker's repository
  for the rest of the job — a support worktree first, put on the job's branch, the base checkout where no
  support worker is free — and prints its path. While the job lives the borrowed checkout reads held
  everywhere a held checkout is read, and nothing is launched into it (docs/verbs.md, "Borrowing a checkout").
- **When the job ends the loan ends:** what it left open in a borrowed support worktree is committed onto its
  branch, and the job's report names it.
- **Why:** a stopped job parks everything open in its checkout as its own, so a write from any other job would
  leave with it. With every job in a checkout of its own, what is open in a checkout is its holder's.
