# Working in bmo-job

These rules hold every change to this repository.

## The documents come first

- `docs/` is the canonical statement of what the service does; the code is one realisation of it.
  Before changing code, read the document covering that area.
- A change that would make a document wrong or incomplete repairs the document first, stating what is
  true now, and the code is written to match.
- A document states what is true now: no change history, no rationale told through an earlier
  design, no pasted source, signature or config fragment, and no file-and-line pins.
- A document names no file outside this repository. Another unit is named by its name, and what it
  keeps is its own documents' to state.
- Every document a comment or a document cites, and every section it cites by name, exists.

## Boundaries

- **The service is middleware:** it stores and serves, and never executes. It calls no other unit; its
  one outgoing call is a delivery to the address a subscriber registered. The routes it serves are listed in
  its API document, and the OpenAPI document is generated from the client's types.
- **Dependencies:** crates from the registry pinned in the workspace manifest, crates of this
  repository by a path inside it, and among other repositories only the base libraries `bmo-chat`,
  `bmo-chat-client` and `bmo-ui`. The dependency check in `tests/` holds this.
- **No lookup table standing in for polymorphism.** A value that differs per member belongs to that
  member.
- **No hand-written parser for a general format.** TOML, JSON, markdown, command lines and their kin
  are read by a maintained library.

## Language

- Everything in this repository is English: code, comments, strings, tests, documents, commit messages
  and branch names. No Korean in code or tests; the Hangul check in `tests/` holds this.
- What the service answers is an English sentence for a person or a status code and a snake_case code
  for a machine. Words a person reads in another language are the web console's language packs.
- No personal data: no user names, home directories, machine names or names of any agent. The personal-data
  check in `tests/` holds this over the whole tree, and the contract check holds the API, its tools and the
  skill free of every caller's concepts. `docs/PERSONAL-DATA.md` states the rule and the values it allows.

## Layout

- Only Cargo manifests at the root, and no directory named after its parent; the layout check in
  `tests/` holds both.
- Tests live in `tests/`: the root package's in the root `tests/`, a crate's in its own `tests/`.

## The stages of a change

1. **Writing:** the only check is `cargo check --workspace --all-targets`, on the narrowest crate that
   is cheaper.
2. **Review:** read the whole diff against the document governing it, then `cargo fmt --all --check`
   and `cargo clippy --workspace --all-targets -- -D warnings`.
3. **Verification:** the tests of the crates touched, then `cargo test --workspace` and the package's
   `bun run check` once, output written to a file and the file read.

A change is not done before stage 3 has passed. A change to the contract writes `docs/openapi.json` and the
package's generated types again (README.md, "Commands"), and the checks fail until it does.

## Closing and deploying

- A job commits its own steps to this repository, which has no remote, and never deploys.
- bmo-testbed deploys this repository by `testbed.toml`; the service runs as `deploy/bmo-job.service`,
  `bmo-job serve` out of the deploy tree (docs/deploy.md). A change to what a release holds, the service's
  unit or its start line changes `testbed.toml`, the unit and docs/deploy.md together.
