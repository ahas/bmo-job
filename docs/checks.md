# Repository checks

These checks hold the repository's own rules. They are tests of the root package, so the workspace's test run
carries them and a change that breaks a rule fails like any other test.

## No Hangul in code or tests

- **What it reads:** every file under `src/`, `crates/`, `tests/` and `package/`, skipping build output and
  installed packages.
- **What fails:** any character of the Hangul syllables block, U+AC00 to U+D7A3. A failure names each file and
  line.
- **Why it holds:** the service answers in English sentences and codes. Words a person reads in another
  language belong to whoever shows them.
- The check itself names the range by code point, so it holds no Hangul of its own.

## No personal data

- **The rule:** `docs/PERSONAL-DATA.md` states what the repository may not hold, where a value goes instead,
  and the values it allows.
- **What it reads:** every file in the repository, at any depth, skipping build output, git's directory,
  installed packages and a deployment's `settings.toml`.
- **What fails:** each of these, with every line named by file, line and what was found there:
  - a home directory: a Unix-like `/home/`, `/Users/` or `/root/` followed by a name, or a Windows path's
    `Users` folder followed by one;
  - an email address, other than the address the rule allows;
  - a telephone number written in groups, with an optional country or area code;
  - a credential: a long value assigned to a token, key, secret or password name, and the shape a vendor
    gives its keys;
  - a private network address in the RFC 1918 ranges or the link-local range; a loopback address is
    neither and passes;
  - the name of the machine the check runs on, as a whole word in any case.
- **The check keeps no list of personal terms:** it matches shapes, reads the machine's name where it stands,
  and allows only the values the rule names.

## The contract names nothing of its callers

- **What it reads:** the OpenAPI document the code generates, its tools among it, and the `bmo-job` skill.
- **What fails:** a word that names a unit's caller or a concept only a caller holds: an agent, the agency, a
  worker, a room, the owner, a persona, the receiver, the console. Callers are the caller's own
  opaque `scope`, `<role>Name` and `labels`.

## The committed contract is the generated one

- **`docs/openapi.json`** is compared with what the code generates; a difference fails, and `bmo-job openapi`
  writes it again.
- **The tools** are exactly the draft's twenty-one, in its order, each calling a route the document states.
- **Every stated route is served:** a request on each answers with anything but `no-route`.
- **The TypeScript types** in `package/` are compared with what openapi-typescript generates from the committed
  document, by the package's own test; `bun run generate` in `package/` writes them again.

## The layout

- **Only Cargo manifests at the root.** A manifest of any other build tool at the root fails.
- **No directory named after its parent,** at any depth, the root's own name included. Build output, installed
  packages and the git directory are skipped.

## Dependencies

- **What it reads:** every `Cargo.toml` in the repository, through a TOML library, and every dependency table
  in it: the normal, development and build tables, their per-target forms, the workspace's shared table and
  each patch source's table.
- **A path dependency stays inside the repository.** One whose path resolves outside the root fails. A
  dependency taken from the workspace is checked where the workspace's shared table names it.
- **No other unit's crate.** A dependency named `bmo-` something fails unless it is one of the base libraries
  `bmo-chat`, `bmo-chat-client` and `bmo-ui`, or a crate of this repository reached by a path inside it. The
  name read is the package's own where a dependency renames it.
