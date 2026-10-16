# bmo-job

The TODO, work orders and the job queue as one service with its own API. It owns the rows of the TODO,
the work orders and the jobs that carry them out, the queue that decides whose turn comes next, and
the worker definitions those jobs run as. Every other unit is reached through its API: agent sessions
through bmo-agency, rooms through bmo-receiver, suites and deploys through bmo-testbed, users and their
grants through bmo-web, translations, the sanitiser and a piece of work's knowledge through bmo-gate,
and memories and brain holds through bmo-brain.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service's own package: the library and, under `src/bin/`, the server and the unit's command line |
| `crates/` | `agency`, the port to bmo-agency: every call the service makes there, as a trait and an HTTP client; `base`, the library base every crate stands on: refusals, the database, settings and the clock; `brain`, the port to bmo-brain: every call the service makes there, as a trait and an HTTP client; `codes`, every refusal, answer and fact the service gives as codes with structured fields; `gate`, the port to bmo-gate: every call the service makes there, as a trait and an HTTP client; `policy`, which reads the files of `policy/`; `queue-read`, which reads the queue as the database holds it; `queue-rows`, which writes the TODO's rows and work orders; `receiver`, the port to bmo-receiver: every call the service makes there, as a trait and an HTTP client; `testbed`, the port to bmo-testbed: every call the service makes there, as a trait and a client running its binary; and `web`, the port to bmo-web: every call the service makes there, as a trait and an HTTP client |
| `migration/` | The schema of the tables the service owns, as migrations that run up and down |
| `policy/` | The service's fixed numbers and words, as TOML files read by its policy loader |
| `tests/` | The root package's tests: the repository checks, and the agency, brain, gate, receiver, testbed and web ports' contract tests, each against a stand-in for its unit: an in-process server, or for bmo-testbed a stand-in program |
| `docs/` | The canonical statements of what the service does |

At the root stand the Cargo workspace manifest, the version, `testbed.toml`, `rustfmt.toml`,
`CLAUDE.md` and `settings.example.toml`, the example of the settings file a deployment gives.

## Documents

| Document | States |
|---|---|
| `docs/api.md` | The API: how a refused request is answered, every code the service gives, and every call it makes on another unit |
| `docs/settings.md` | The settings file: the database, the deployment's names and its words |
| `docs/policy.md` | The policy: the parts under `policy/`, their marks, patterns and words, and how they load |
| `docs/schema.md` | The schema: the tables the service owns, the rules they keep, and the migrations |
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
