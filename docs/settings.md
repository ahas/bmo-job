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

The file holds five sections: `[database]` and `[deployment]`, both required; `[server]` and `[units]`,
which the server requires and the libraries read without; and `[words]`, which may be left out. A key
the service does not know is refused, so a misspelt setting fails at start rather than standing
silently at a default.

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

### `[server]`

Where the server stands (docs/api.md, "Routes"):

| Key | What it names |
|---|---|
| `listen` | The socket address the server binds, such as `127.0.0.1:7841` |
| `address` | The `http:` address other units reach the server at, which it gives bmo-receiver as a holder's |
| `unit` | The service unit the server runs as, which it declares itself a holder under |

### `[units]`

The `http:` address of each unit the server calls on (docs/api.md, "Calls on other units"):

| Key | The unit |
|---|---|
| `agency` | bmo-agency, whose provider order a reading of an item's model is read against |
| `gate` | bmo-gate, whose kept English a worker's reading carries |
| `receiver` | bmo-receiver, which the server declares itself a holder to and hands an item's talk |

Every key of both sections is required where the section stands. An address that is not `http:` is
refused when the file is loaded.

### `[words]`

The words of the deployment's own language that the service's policy reads and writes by, one table to
a policy part: `[words.closing]`, for example, gives the closing part's words. Each part says which
words it takes and what its English ones are (docs/policy.md); a key given here takes the place of the
part's own. The settings file loads whatever this section holds, and the policy refuses at its own load
what no part takes. Where the section is left out, every part reads by its English words.

## Why a load fails

A load that fails names the file and one of these:

- the file cannot be read;
- the file is not TOML of this shape: a section or a key is missing, unknown or of the wrong type;
- the database address is of a kind the service does not keep rows in;
- a deployment name is empty;
- a listening address is not a socket address, or a unit's address or the server's own is not `http:`.

The server also refuses to start where `[server]` or `[units]` is left out.
