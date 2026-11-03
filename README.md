# bmo-job

A todo kept as middleware, in the way Redis is one: items, their notes, reports and work orders, the queues the
items wait in, leases on items, questions and their replies, and subscriptions, stored and served through one
API. The service never executes and calls no other unit. Work reaches whoever does it two ways side by side:
pushed, as the events of a subscription the one doing it registered, delivered by webhook or read as a stream
and turned into MCP notifications; and pulled, by a lease. A coding harness takes it as its first entry point
through the `bmo-job` plugin: the MCP server `bmo-job mcp` and its skill.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service's own package: the store, the HTTP server and its event stream, webhook delivery, the settings, and under `src/bin/` the binary `bmo-job` |
| `crates/` | `client`, the crate `bmo-job-client`: the contract in Rust, every body as a type, every route as a call, the event stream, the webhook's signature and the OpenAPI document generated from those types; and `plugin`, the crate `bmo-job-plugin`: the harness plugin, the MCP server and its tools over the client, the whole OpenAPI document with the tools, and the `bmo-job` skill under `skills/` |
| `package/` | The TypeScript package `bmo-job-client`, packed for a downstream checkout: types generated from `docs/openapi.json` and a client over them |
| `deploy/` | The service's user unit, which runs `bmo-job serve` out of the deploy tree (docs/deploy.md) |
| `tools/` | The service's tools for the tool host: `todo`, `state` and `launch`, each a manifest, an executable and its schemas (docs/tools.md) |
| `tests/` | The root package's tests: the repository checks, the contract's checks, every route through the real server, and the binary as a unit and a harness run it |
| `docs/` | The canonical statements of what the service does, and `openapi.json`, the contract as the code generates it |
| `.graph/` | The code dependency graph: the repository's settings, one node per feature with its files and the features it needs, and a boundary node for each API the service exposes. Its area files are generated in the checkout and untracked, so a fresh checkout builds them again, while `settings.toml` is the repository's configuration and stays tracked |

At the root stand the Cargo workspace manifest, the version, `testbed.toml` (bmo-testbed's declaration of how
this repository is tested, checked, built and deployed, docs/deploy.md), `rustfmt.toml`, `CLAUDE.md` and
`settings.example.toml`, the example of the settings file a deployment gives.

## Documents

| Document | States |
|---|---|
| `docs/api.md` | The API: every resource, its states and moves, its routes, the events, subscriptions and webhook delivery, the client and the harness plugin |
| `docs/openapi.json` | The contract as the code generates it, with the tools under `x-tools` and the harness entry under `x-harness` |
| `docs/tools.md` | The tools the service places: the queue's entries, the chat agent's state readings and its launching entries |
| `docs/cli.md` | The command line: `serve`, `mcp` and `openapi` |
| `docs/settings.md` | The settings file: where the service listens, its tokens and the callers they name, the labels whose items open by themselves, and its database |
| `docs/schema.md` | The schema: the tables and the rules they keep |
| `docs/deploy.md` | Deploying the service: what a release holds, where the service stands, and what gates a deployment |
| `docs/base-libraries.md` | The road a job takes when a base library it imports lacks a part: the pull request, the rule it keeps, the review, and the version pin |
| `docs/checks.md` | The repository checks |
| `docs/PERSONAL-DATA.md` | The personal data the repository may not hold, where a value goes instead, and the values it allows |

## Version

- **One version for the whole repository,** the workspace's. `VERSION` holds it and the workspace's version
  equals it, as do the OpenAPI document and the TypeScript package.
- **It moves only at a release the owner declares,** as bmo-chat's `docs/conventions.md` 「Versions」 states; a
  commit, a deploy, a fix or a contract change moves none, and a deployment is told apart by its commit.
- **Until the first release the version is `0.1.0` with a revision suffix,** `0.1.0-rev.N`: `0.1.0` is the
  base, and each release the owner declares raises the revision by one. The major, minor and patch numbers do
  not move before the first release.

## Commands

```sh
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd package && bun install --frozen-lockfile && bun run check)
```

The server reads the settings file `BMO_JOB_SETTINGS` names, `settings.toml` otherwise (docs/settings.md), and
serves; the MCP server reaches a running service at an address:

```sh
cargo run --bin bmo-job -- serve
BMO_JOB_TOKEN=... cargo run --bin bmo-job -- mcp --service http://127.0.0.1:7841
```

A change to the contract writes the document and the TypeScript types again:

```sh
cargo run --bin bmo-job -- openapi > docs/openapi.json
(cd package && bun run generate)
```

`bun pm pack` in `package/` writes the tarball a downstream checkout installs (docs/api.md, "The client").

Deployed, the service runs as the user unit `deploy/bmo-job.service`, `bmo-job serve` out of the deploy tree;
bmo-testbed puts a commit live by `testbed.toml` (docs/deploy.md).
