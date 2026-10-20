# Deploying the service

bmo-testbed deploys this repository, one commit at a time, by the declaration at the root, `testbed.toml`, in its
own format; the deployment's mechanics, the releases, the flip, the backups, the restarts and the rollback, are
bmo-testbed's. This document states what the declaration says of this service.

## What a release holds

- **A release is the repository at a commit,** its `bin/` holding the binary built for that commit, `bmo-job`.
- **The settings are the data root's,** never a release's: the settings file `[deploy] env_links` names under
  the data root, which each release links to as `settings.toml`, the file the service reads in its working
  directory (docs/settings.md, "Where the file is"). A deployment with no such file is refused.

## Where it stands

- **The service's own deploy tree** is the link `[deploy] root` and `link` name, onto the live release, and its
  data root is the one `[deploy] data_root` names.
- **The service runs as one user unit,** `deploy/bmo-job.service`, linked from the deploy tree: the release
  binary run as `bmo-job serve` in the release (docs/cli.md). It carries the `bot` mark, so it is the
  repository's one service, stopped, restarted and watched staying up by every deployment, and it does not
  start where the data root holds no settings.
- **The MCP server is no unit.** A harness starts `bmo-job mcp` itself, from a release's `bin/`, for as long as
  its session lasts.

## What gates a deployment

- **The static check and the suite** run before a deployment, as bmo-testbed's gate does for every repository.
  The suite is the Cargo workspace's tests and the TypeScript package's `check` script (`[test] languages`, its
  dependencies installed by `[test] setup`), and the quick checks are the layout and the dependencies
  (`[check]`), beside this repository's own checks in `tests/` (docs/checks.md).
