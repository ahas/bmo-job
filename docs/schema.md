# Schema

The service keeps everything it holds in one SQLite database, the one `[database] url` names (docs/settings.md).
The schema starts at the 1.0.0 baseline, version 9, and each step since raises its version by one; the
current version is 10. The database's own user version records the version it holds. The first time the
service opens a database with no tables it makes every table of the baseline and takes every step after it.
A database at the baseline or a step past it takes the steps it lacks when the service opens it, each adding
what it adds and keeping every row; an install from before 1.0.0 at its last release holds the baseline
already. A database of a version older than the baseline is refused, with its version named: the last release
before 1.0.0 brings it up first. One of a later version than the current is refused too.

| Version | What its step adds |
|---|---|
| 10 | A task's `after`, the item and the repository it waits on, null on every task standing then |

## The rules of every table

- **The database gives a member its key,** never handing out the same number twice, even after the member
  goes.
- **A time is epoch milliseconds,** and a yes-or-no is `0` or `1`.
- **Lists and maps are JSON text:** labels, approvals, steps, holds, choices, recommendation, what a
  subscription matches, what a lease holds fixed, and a label key's values.
- **No foreign keys.** A row names another by its key, and the code that writes it keeps the reference.
- **One connection writes.** Every change runs alone in one transaction, with the events it tells, so a lease
  raced for by several holders goes to one.

## The tables

| Table | One row is |
|---|---|
| `items` | An item: its words, state, queue and place there, the names and scope the caller gave, its labels, approvals, merge, the item it was raised from, standing lease, the reason, caller and time of its hold while it is held, and the reason, caller and time of its notices switched off while they are |
| `notes` | A note on an item |
| `reports` | A report on an item |
| `reviews` | An entry of an item's review: the run it examines, its stage and round, its kind, words and labels |
| `work_orders` | An item's work order, keyed by the item |
| `tasks` | A task of a work order: its stage, title, state and assignee, and the item and repository its step waits on, both null where it waits on none |
| `queues` | A queue by its name: its state and holds |
| `leases` | A lease, standing or ended, the caller whose token took it, and what of its item it holds fixed; an ended one is kept so its key is answered `gone` |
| `questions` | A question: its words and choices, the recommendation it carries and why, who it is put to, its state, the wait its due time is counted with (the supervisor's question wait, the ask's, or the move's once it is put to a person), its due time, the time its clock had left when paused, whether its due time was told, whether its asker tells of it itself, the last reply its asker was handed, and who put it to a person in the supervisor's place, or `due time` where the service did, and when, and the asker's reference to the notice it told it with |
| `replies` | A reply to a question: its choice and words, who gave it, whether it settled, and where the recommendation gave it, that source and its reason |
| `subscriptions` | A subscription, standing or ended: what it matches, where it posts and its secret, its state, span, failures, and the events it starts after and has handed over |
| `label_values` | The values one label key may take, keyed by the key, and when the list last changed |
| `events` | An event told, with the queue, scope and item it is narrowed by, kept for the window |
