# Schema

The service keeps everything it holds in one SQLite database, the one `[database] url` names (docs/settings.md).
The service makes its tables itself the first time it opens a database with none, and records the schema's
version in the database's own user version; a database of an older version is brought up to it at start, and
one of a later version is refused. Bringing a database up to the version that keeps the item an item was raised
from fills it, for each item standing then, from the item's `raised-by` label where that label is the key of
another standing item, and leaves it null otherwise; a lease standing then is held by no named caller, and
holds nothing of its item fixed.

## The rules of every table

- **The database gives a member its key,** never handing out the same number twice, even after the member
  goes.
- **A time is epoch milliseconds,** and a yes-or-no is `0` or `1`.
- **Lists and maps are JSON text:** labels, approvals, steps, holds, choices, recommendation, what a
  subscription matches, and what a lease holds fixed.
- **No foreign keys.** A row names another by its key, and the code that writes it keeps the reference.
- **One connection writes.** Every change runs alone in one transaction, with the events it tells, so a lease
  raced for by several holders goes to one.

## The tables

| Table | One row is |
|---|---|
| `items` | An item: its words, state, queue and place there, the names and scope the caller gave, its labels, approvals, merge, the item it was raised from, and standing lease |
| `notes` | A note on an item |
| `reports` | A report on an item |
| `reviews` | An entry of an item's review: the run it examines, its stage and round, its kind, words and labels |
| `work_orders` | An item's work order, keyed by the item |
| `tasks` | A task of a work order: its stage, title, state and assignee |
| `queues` | A queue by its name: its state and holds |
| `leases` | A lease, standing or ended, the caller whose token took it, and what of its item it holds fixed; an ended one is kept so its key is answered `gone` |
| `questions` | A question: its words and choices, the recommendation it carries and why, its state, its due time, the time its clock had left when paused, whether its due time was told, whether its asker tells of it itself, and the last reply its asker was handed |
| `replies` | A reply to a question: its choice and words, who gave it, whether it settled, and where the recommendation gave it, that source and its reason |
| `subscriptions` | A subscription, standing or ended: what it matches, where it posts and its secret, its state, span, failures, and the events it starts after and has handed over |
| `events` | An event told, with the queue, scope and item it is narrowed by, kept for the window |
