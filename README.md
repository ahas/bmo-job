# bmo-job

The TODO, work orders and the job queue as one service with its own API. It owns the rows of the TODO,
the work orders and the jobs that carry them out, the queue that decides whose turn comes next, and
the worker definitions those jobs run as. Every other unit is reached through its API: agent sessions
through bmo-agency, rooms through bmo-receiver, suites and deploys through bmo-testbed.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service's own package: the library and, under `src/bin/`, the server and the unit's command line |
| `crates/` | `queue-read`, which reads the queue as the database holds it, and `queue-rows`, which writes the TODO's rows and work orders |
| `migration/` | The schema of the tables the service owns, as migrations that run up and down |
| `policy/` | The service's fixed numbers and words, as TOML files read by its policy loader |
| `tests/` | The root package's tests, the repository checks among them |
| `docs/` | The canonical statements of what the service does |

At the root stand the Cargo workspace manifest, the version, `testbed.toml`, `rustfmt.toml` and
`CLAUDE.md`.

## Documents

| Document | States |
|---|---|
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
