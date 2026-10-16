# Settings

The service reads one TOML file of settings when it starts. Nothing a deployment chooses stands in the
code: the database it keeps its rows in and the names it says and records come from this file.

## Where the file is

- **The path in `BMO_JOB_SETTINGS`,** where that variable is set and not empty.
- **Otherwise `settings.toml` in the working directory.** That file is the deployment's own and git
  ignores it.
- `settings.example.toml` at the root is the checked-in example. It names a database and neutral
  placeholder names, and loads as it stands.

## What the file holds

The file holds two sections, both required. A key the service does not know is refused, so a misspelt
setting fails at start rather than standing silently at a default.

### `[database]`

- **`url`:** the database the service keeps its rows in, as a connection address.
  - An address starting `sqlite:` is a SQLite database, for example `sqlite://job.db?mode=rwc`.
  - An address starting `postgres:` or `postgresql:` is a PostgreSQL database. Options the driver reads
    in the address, such as a schema's search path, are passed on.
  - Any other address is refused when the file is loaded.
- Both kinds take the same statements. A statement binds its values by number, `$1` onwards, and every
  number is bound at its widest width, so a statement prepared once takes later values of any width.
- On PostgreSQL, a statement met as prepared before the tables' shape moved under it runs once more on
  a fresh connection. The pool never hands out a connection opened before that moment.

### `[deployment]`

The names the deployment goes by, which the service says in what it writes and matches in what it
reads:

| Key | What it names |
|---|---|
| `owner_name` | The deployment's owner, where nothing else gives the owner a name |
| `agent_name` | The deployment's agent, as a room calls it |
| `agent_key` | The deployment's agent, as the records name it |
| `sender_name` | The sender the agent's own messages are stored under |
| `receiver_unit` | The service unit the chat receiver runs as |

Every key is required and none may be empty.

## Why a load fails

A load that fails names the file and one of these:

- the file cannot be read;
- the file is not TOML of this shape: a section or a key is missing, unknown or of the wrong type;
- the database address is of a kind the service does not keep rows in;
- a deployment name is empty.
