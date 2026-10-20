# Schema

The service keeps everything it holds in one SQLite database, the one `[database] url` names (docs/settings.md).
The service makes its tables itself the first time it opens a database with none, and records the schema's
version in the database's own user version; a database of another version is refused at start.

## The rules of every table

- **The database gives a member its key,** never handing out the same number twice, even after the member
  goes.
- **A time is epoch milliseconds,** and a yes-or-no is `0` or `1`.
- **Lists and maps are JSON text:** labels, approvals, steps, holds, choices, and what a subscription matches.
- **No foreign keys.** A row names another by its key, and the code that writes it keeps the reference.
- **One connection writes.** Every change runs alone in one transaction, with the events it tells, so a lease
  raced for by several holders goes to one.

## The tables

| Table | One row is |
|---|---|
| `items` | An item: its words, state, queue and place there, the names and scope the caller gave, its labels, approvals, merge and standing lease |
| `notes` | A note on an item |
| `reports` | A report on an item |
| `work_orders` | An item's work order, keyed by the item |
| `tasks` | A task of a work order: its stage, title, state and assignee |
| `queues` | A queue by its name: its state and holds |
| `leases` | A lease, standing or ended; an ended one is kept so its key is answered `gone` |
| `questions` | A question, its state, its due time and the time its clock had left when paused, and whether its due time was told |
| `replies` | A reply to a question |
| `subscriptions` | A subscription, standing or ended: what it matches, where it posts and its secret, its state, span, failures, and the events it starts after and has handed over |
| `events` | An event told, with the queue, scope and item it is narrowed by, kept for the window |
