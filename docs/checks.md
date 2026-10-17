# Repository checks

Four checks hold the repository's own rules. They are tests of the root package, so the workspace's
test run carries them and a change that breaks a rule fails like any other test.

## No Hangul in code or tests

- **What it reads:** every file under `src/`, `crates/`, `migration/`, `policy/`, `tools/` and `tests/`, skipping
  build output.
- **What fails:** any character of the Hangul syllables block, U+AC00 to U+D7A3. A failure names each
  file and line.
- **Why it holds:** the service answers in English sentences and status codes. Words a person reads in
  another language are the web console's language packs, never this repository's.
- The check itself names the range by code point, so it holds no Hangul of its own.

## No personal data

- **What it reads:** every file in the repository, at any depth, skipping build output, git's directory
  and the settings file it takes the names from.
- **What fails:**
  - a home directory: `/home/` followed by a name, as a user's home is written. A pattern that names no
    one, such as `/home/` followed by a bracket, stands;
  - the name of the machine the check runs on, and of the work node the settings name in `[node]`,
    as a whole word in any case;
  - the deployment's names in `[deployment]`: the owner's, the agent's and the sender's, each as a
    whole word as it is spelled.
  A failure names each file and line, and what it found there.
- **Where the names come from:** the deployment's own settings file, which is gitignored: the path
  `BMO_JOB_SETTINGS` names, otherwise `settings.toml` at the root. A file that is there and does not
  load fails the check, since it could vouch for nothing. Where there is none, only home directories and
  the machine's names are looked for.
- **What it does not look for:** the agent's key and the receiver's unit, which name the platform's own
  parts. The check writes down no name of its own: every name it looks for is read where it stands.
- **Why it holds:** no user name, home directory, machine name or agent's name belongs in the code.
  Names the service needs at runtime come from its deployment settings.

## The layout

- **Only Cargo manifests at the root.** A manifest of any other build tool at the root fails.
- **No directory named after its parent,** at any depth, the root's own name included. Build output
  and the git directory are skipped.
- **One version.** `VERSION` at the root holds the semver string, and the workspace's version equals
  it.

## Dependencies

- **What it reads:** every `Cargo.toml` in the repository, through a TOML library, and every
  dependency table in it: the normal, development and build tables, their per-target forms, the
  workspace's shared table and each patch source's table.
- **A path dependency stays inside the repository.** One whose path resolves outside the root fails.
- **No other unit's crate.** A dependency named `bmo-` something fails unless it is one of the base
  libraries `bmo-chat`, `bmo-chat-client` and `bmo-ui`, or a crate of this repository reached by a
  path inside it. The name read is the package's own where a dependency renames it.
- Every other unit is reached through its API, never through its code.
