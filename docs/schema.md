# Schema

The service keeps its rows in the database `[database] url` names (docs/settings.md), on SQLite or on
PostgreSQL. The `migration` crate makes the tables, and nothing else writes the schema.

## The rules of every table

- **The same shape on both backends.** Each table has the same columns in the same order, the same
  nullability, the same indexes and the same triggers on SQLite and on PostgreSQL. Only the spelling of
  a type or an expression differs between the two.
- **Numbers are 64-bit integers.** A time is milliseconds since the epoch, a yes-or-no is `0` or `1`,
  and the one fractional number, a span's seconds, is a double.
- **JSON is kept as text.** A work order's body is one JSON text, read by expression where a question
  filters on it.
- **The database gives a row its number.** A table keyed by `id` numbers a row the writer leaves
  unnumbered, and never hands out the same number twice.
- **Every table has a `rowid`,** which follows the row's insertion order: SQLite's own, and on
  PostgreSQL a column of that name made by the database. On a table keyed by `id` it equals the `id`.
- **No foreign keys.** A row names another by its key, and the code that writes it keeps the
  reference. The one rule the database itself holds is the merge of TODO items, below.

## The tables

| Table | One row is |
|---|---|
| `todos` | A TODO item: its title, instruction, requester and the agent that filed it, approval and how it came, status, priority, hold and resume conditions, merge, brief, impact grounds and report |
| `todo_notes` | Words added to an item after it was filed, or a change made to its brief, with who added it and the job it was handed to |
| `todo_sources` | An item raised from another: the item and its source |
| `todo_approvals` | One change of an item's approval, given or taken back, with who made it, by which road and when |
| `work_orders` | An item's work order, its body as one JSON text, keyed by the item |
| `work_order_locks` | Who holds a work order's lock: the host, the process and its start, and the token it took the lock with |
| `jobs` | A job: its unit, launch, prompt and its English and Korean readings, the road it ran on, its worker and item, its ending, outcome and report |
| `job_spans` | Where a job's time and tokens went, by kind and name |
| `job_questions` | A job's question to the owner: its options and recommendation, its deadline, and how it was settled |
| `question_talk` | One thing said about a question while it was open |
| `checkout_leases` | The job that holds a worker's checkout |
| `branch_agents` | A co-work branch agent standing in a pool slot on one road, and when it last answered |
| `session_work_items` | The item a worker session was last served |
| `session_work_results` | The version of an earlier co-work result a worker session has been sent in full |
| `live_claude_openings` | A job's opening of a live Claude session, and what for |
| `live_claude_runs` | One thing run in a live Claude opening |
| `clock_holds` | A run that reached its clock: the question asked before cutting it, the verdict and how it closed |
| `workers` | A job worker's definition: its label, checkout, icon, preamble, development command, support workers, the node it stands on, its role and whether it rests |
| `settings` | One of the service's own flags, by key |

### The merge of TODO items

An item merged into another names it in `merged_into`. The database refuses, on insert and on update of
that column, a merge:

- of an item into itself;
- into an item that does not exist;
- into an item that is itself merged;
- of an item other items are merged into.

It also refuses to delete an item other items are merged into. So a merge link is always one step long
and lands on an item that carries the work itself. Each refusal is an error whose message starts
`todo merge:`.

### The keys of `settings`

`settings` is a table of keys and text values, and it holds only the keys of this service. The database
refuses any other key. A key is either one of these fixed keys:

- `queue_paused`, `deploy_hold`, `deploy_waits_on`, `machine_launch_held`, `failure_orders`,
  `job_queue_told`, `work_retry_model`;

or one of these prefixes followed by the id it is about, one row per id:

- `job_stop:`, `job_route_attempts:`, `job_route_block:`, `job_report_waiting:`,
  `job_ending_waiting:`, `job_report_roads:`, `job_wait:`, `job_command:`, `job_quiet_told:`,
  `job_spin_told:`, `job_queue_waiting:`, `job_queue_waiting_told:`, `todo_repair_ceiling_told:`.

A flag another unit owns, such as an execution's cancel mark or a brain hold, is read through that
unit's API and is never a row here.

## What is not here

Rows another unit owns are reached through that unit's API (docs/api.md), never kept as a copy:

- **bmo-agency:** an agent's session and runtime state, the agent log, executions, runs, worker
  sessions and their resets, the co-worker pool, nodes and Claude accounts.
- **bmo-receiver:** instructions, messages and room answerers.
- **bmo-brain:** memories.
- **bmo-gate:** document translations.
- **bmo-testbed:** pending deploys.

## Migrations

- The tables are made by migrations run in order, each recorded in the table `seaql_migrations` once it
  has run. Running the migrations again runs only those not yet recorded.
- Every migration runs up and down on both backends. Down takes away exactly what up made, so a
  database taken all the way down holds only the record table.
- A change to a table is a new migration; a migration that has run is never edited.
