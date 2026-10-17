# bmo-job

The TODO, work orders and the job queue as one service with its own API. It owns the rows of the TODO,
the work orders and the jobs that carry them out, the queue that decides whose turn comes next, and
the worker definitions those jobs run as. Every other unit is reached through its API: agent sessions
through bmo-agency, rooms through bmo-receiver, suites and deploys through bmo-testbed, users and their
grants through bmo-web, translations, the sanitiser and a piece of work's knowledge through bmo-gate,
memories and brain holds through bmo-brain, and what a checkout's code graph names through bmo-graph.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service's own package: the server's routes, the verbs' traits it is built on, the job queue, the work-order runner and the pieces of a job's run it stands on, the item talk and the event stream, and under `src/bin/` the binary `bmo-job` that serves them |
| `crates/` | `agency`, the port to bmo-agency: every call the service makes there, as a trait and an HTTP client; `base`, the library base every crate stands on: refusals, the database, settings and the clock; `brain`, the port to bmo-brain: every call the service makes there, as a trait and an HTTP client; `codes`, every refusal, answer and fact the service gives as codes with structured fields; `gate`, the port to bmo-gate: every call the service makes there, as a trait and an HTTP client; `graph`, the port to bmo-graph: every call the service makes there, as a trait and an HTTP client; `policy`, which reads the files of `policy/`; `queue-read`, which reads the queue as the database holds it; `queue-rows`, which writes the TODO's rows and work orders; `receiver`, the port to bmo-receiver: every call the service makes there, as a trait and an HTTP client; `testbed`, the port to bmo-testbed: every call the service makes there, as a trait and a client running its binary; and `web`, the port to bmo-web: every call the service makes there, as a trait and an HTTP client |
| `migration/` | The schema of the tables the service owns, as migrations that run up and down |
| `policy/` | The service's fixed numbers and words, as TOML files read by its policy loader |
| `tests/` | The root package's tests: the repository checks; the agency, brain, gate, graph, receiver, testbed and web ports' contract tests, each against a stand-in for its unit: an in-process server, or for bmo-testbed a stand-in program; and the server's request tests, every route through the real server and the binary as a unit runs it |
| `docs/` | The canonical statements of what the service does |

At the root stand the Cargo workspace manifest, the version, `testbed.toml`, `rustfmt.toml`,
`CLAUDE.md` and `settings.example.toml`, the example of the settings file a deployment gives.

## Documents

| Document | States |
|---|---|
| `docs/api.md` | The API: how a refused request is answered, every code the service gives, every route it serves, and every call it makes on another unit |
| `docs/settings.md` | The settings file: the database, the deployment's names and its words |
| `docs/policy.md` | The policy: the parts under `policy/`, their marks, patterns and words, and how they load |
| `docs/schema.md` | The schema: the tables the service owns, the rules they keep, and the migrations |
| `docs/rows.md` | Writing the queue: an item filed, added to, approved, weighed, merged and closed, the closing's readings of its unfinished note, and a work order's lock |
| `docs/queue.md` | Running the queue: workers, routes and turns, a launch judged and started, what is running, how a job ends, its questions and clocks, a work order's verbs, what the agent and the rooms are told, and the work-order runner |
| `docs/runs.md` | Running a job: its locks, its checkout before and after, its prompt and turns, the judgement of its last answer, its ending, landing on main, and the commands it allows itself |
| `docs/reading.md` | Reading the queue: an item as a person and as a worker reads it, the time it took, a room's jobs and a work order |
| `docs/checks.md` | The repository checks: no Hangul in code or tests, the layout, the dependencies |

## Version

- **One version for the whole repository,** the workspace's, in semver. `VERSION` holds it and the
  workspace's version equals it.
- **A major change is a change to the API contract;** anything else is a minor or a patch.

## Commands

```sh
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The server reads the settings file `BMO_JOB_SETTINGS` names, `settings.toml` otherwise
(docs/settings.md), migrates its database up and serves:

```sh
cargo run --bin bmo-job
```
