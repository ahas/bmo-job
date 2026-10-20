# Personal data

The repository holds nothing that belongs to a person, a machine or a network. A value that differs
by deployment belongs to that deployment's settings, and the repository keeps only a neutral
example of it.

## What the repository may not hold

- A person's name, handle, account id or display name.
- An email address.
- A telephone number.
- A host name, a domain name or a machine name.
- A home directory path.
- A device, a network address or another detail of a network.
- A company's name.
- A room member's display name or id.
- A line of a real conversation.

## Where the rule applies

- Code, comments and strings.
- Documents and the README.
- Test fixtures and settings examples.
- Commit messages, branch names and tags.
- Anything published from the repository.

## What goes in its place

- **A deployment's value lives in its settings file.** The repository holds the setting's shape and a
  neutral example, never a working value.
- **An example uses a neutral name:** `tester`, `qa-admin`, or a name reserved for documentation such
  as `example.org`.
- **A person, a machine or a room is named by its role,** not by the name it carries.
- **A conversation is written as its shape,** not quoted line by line.

## Allowed values

These two values name this repository and its owner, and may stand as they are:

- `ahas`
- `dev@ahas.io`

Every other value of the kinds above is personal data.

## The check

The personal-data check in `tests/` reads every file in the repository and fails on:

- a home directory written as a path;
- an email address other than the allowed one;
- a telephone number written in groups;
- a credential, as a long value assigned to a token, key, secret or password name, or as the shape a
  vendor gives its keys;
- a private or link-local network address;
- the name of the machine the check runs on.

The check keeps no list of personal terms. It matches shapes, reads the machine's name where it
stands, and allows only the values this document names. A failure gives the file and the line, and
what it found there. The rules the check cannot read, such as a commit message, are held by review.
