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

The testbed's `personal-data` check, which the declaration enables, reads every file git tracks or
would track, before anything is built, and fails on:

- a home directory written as a path, outside the neutral role names;
- an email address other than the allowed one and those in the domains kept for documents;
- a telephone number written in groups;
- a credential in the shape a vendor gives its keys, a private key's opening line, or a bearer value;
- a private or link-local network address.

The check keeps no list of personal terms. It matches shapes, which are the testbed's policy (its
`docs/checks.md`, 「The quick checks」), and allows only the address the declaration names. A failure
gives the file, the line and the kind, never the value. The rules the check cannot read, such as a
commit message or the name of a machine, are held by review.
