# Deploying the service

bmo-testbed deploys this repository, one commit at a time, by the declaration at the root, `testbed.toml`, in
its own format; the deployment's mechanics — the releases, the flip, the backups, the restarts and the
rollback — are bmo-testbed's. This document states what the declaration says of this service. A job never
deploys (docs/jobs.md, "A job closes").

## What a release holds

- **A release is the repository at a commit,** set up as any checkout of it is, its `bin/` holding the
  binaries built for that commit: the service's own, `bmo-job`, and each tool's executable, `todo-tool`,
  `launch-tool` and `state-tool`.
- **The settings are the data root's,** never a release's: the settings file `[deploy] env_links` names under
  the data root, which each release links to as `settings.toml`, the file the service reads in its working
  directory (docs/settings.md, "Where the file is"). A deployment with no such file is refused.
- **Each tool is placed** in its folder's `.placed/` inside the release before the release is flipped onto:
  the executable out of `bin/`, its manifest and the schemas beside it (docs/tools.md, "Where a tool is
  placed").

## Where it stands

- **The service's own deploy tree** is the link `[deploy] root` and `link` name, onto the live release, and
  its data root is the one `[deploy] data_root` names, which it shares with the services that read its files.
- **The service runs as one user unit,** `deploy/bmo-job.service`, linked from the deploy tree: the release
  binary run as `bmo-job serve` in the release (docs/cli.md, "The nouns"). It carries the `bot` mark, so it is
  the repository's one service, stopped, restarted and watched staying up by every deployment, and it does not
  start where the data root holds no settings.
- **A job's unit runs the program `[host] runner` names** (docs/runs.md, "What the runner is handed"), which
  the deployment's settings name as the deploy tree's `bin/bmo-job`; a job keeps the release its unit was
  started on to its end.

## What gates a deployment

- **The static check and the suite** run before a deployment, as bmo-testbed's gate does for every repository.
  The suite is the Cargo workspace's tests (`[test] languages`), and the quick checks are the layout and the
  dependencies (`[check]`), beside this repository's own checks in `tests/` (docs/checks.md).
- **Each tool is checked** by the command its manifest names (`[tools]`).
