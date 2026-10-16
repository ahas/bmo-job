# Working in bmo-job

These rules hold every change to this repository.

## The documents come first

- `docs/` is the canonical statement of what the service does; the code is one realisation of it.
  Before changing code, read the document covering that area.
- A change that would make a document wrong or incomplete repairs the document first, stating what is
  true now, and the code is written to match.
- A document states what is true now: no change history, no rationale told through an earlier
  design, no pasted source, signature or config fragment, and no file-and-line pins.

## Boundaries

- **Every other unit is reached through its API,** never through its code, its tables or its files.
  The routes this service serves and every call it makes to another unit are listed in its API
  document.
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
- No personal data: no user names, home directories, machine names or names of any agent. Names the
  service needs at runtime come from its deployment settings, never from the code.

## Layout

- Only Cargo manifests at the root, and no directory named after its parent; the layout check in
  `tests/` holds both.
- Tests live in `tests/`: the root package's in the root `tests/`, a crate's in its own `tests/`.

## The stages of a change

1. **Writing:** the only check is `cargo check --workspace --all-targets`, on the narrowest crate that
   is cheaper.
2. **Review:** read the whole diff against the document governing it, then `cargo fmt --all --check`
   and `cargo clippy --workspace --all-targets -- -D warnings`.
3. **Verification:** the tests of the crates touched, then `cargo test --workspace` once, output
   written to a file and the file read.

A change is not done before stage 3 has passed.
