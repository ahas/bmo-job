# Deploying the service

bmo-testbed deploys this repository, one commit at a time, by the declaration at the root, `testbed.toml`, in its
own format; the deployment's mechanics, the releases, the flip, the backups, the restarts and the rollback, are
bmo-testbed's. This document states what the declaration says of this service.

## What a release holds

- **A release is the repository at a commit,** its `bin/` holding the binary built for that commit, `bmo-job`,
  and the executables of the service's tools, `todo-tool`, `state-tool` and `launch-tool`.
- **The settings are the data root's,** never a release's: the settings file `[deploy] env_links` names under
  the data root, which each release links to as `settings.toml`, the file the service reads in its working
  directory (docs/settings.md, "Where the file is"). A deployment with no such file is refused.

## Where it stands

- **The service's own deploy tree** is the link `[deploy] root` and `link` name, onto the live release, and its
  data root is the one `[deploy] data_root` names.
- **The service runs as one user unit,** `deploy/bmo-job.service`, linked from the deploy tree: the release
  binary run as `bmo-job serve` in the release (docs/cli.md). It carries the `bot` mark, so it is the
  repository's one service, stopped, restarted and watched by every deployment, and it
  does not start where the data root holds no settings. A stop waits for it to end: on `SIGTERM` it ends
  its event streams, answers the requests it holds, and exits.
- **The MCP server is no unit.** A harness starts `bmo-job mcp` itself, from a release's `bin/`, for as long as
  its session lasts.

## Its tools

- **Each tool is placed before the flip,** by a `[[deploy.step]]` named `place-<tool>-tool`: the release's
  executable, the tool's manifest and its schemas are copied into `tools/<tool>/.placed`, the folder the tool
  host reads.
- **Each is registered after the service answers ready,** by a `[[deploy.step]]` named
  `register-<tool>-tool`: `bmo-pocket put --service bmo-job --placed`, allowing the capabilities the tool
  declares, so every release carries its tools without a hand on the registry (docs/tools.md).

## The unit command

- **The deployment links the release's binary into `~/.local/bin/bmo-job`,** by the `[[deploy.step]]` named
  `link-command`, so the platform `bmo` finds the unit command and reads the nouns it declares.
- **`bmo` hands it `job` and `todo`:** `bmo job close` runs `bmo-job job close`, and `bmo todo --done`
  runs `bmo-job todo --done` (docs/cli.md).

## A job closes

- **A close ends at the merge.** `bmo job close` checks what the job changed, pushes its commits onto the
  remote's main and leaves the item waiting for this repository's batch deployment (docs/cli.md, 「`bmo-job job
  close`」). It runs no suite and waits for no deployment.
- **The batch deployment.** Every commit a close leaves on main waits there, its item labelled `close-phase`
  `waiting-batch` with the commit in `close-commit`, until one deployment of this repository puts main live, the
  waiting commits all at once. When that deployment is made, and who makes it, is the deployer's; the service
  keeps no backlog and reads none, and the labels on the items are the whole record of what waits.
- **The deployment record.** Each deployment writes `deployed.json` under the deploy root `[deploy] root` names
  (`~/.local/share/bmo-job`), holding the commit it put live, and appends a line to `deployed.log` beside it,
  its last line naming that commit too. A checkout with no `[deploy] root` reads the platform's own
  `deployed.json` under the data root.
- **What a close reads of the record.** A commit the record already names as live runs nothing and ends clean.
  Where the job's commits already stand on main, so nothing is left to push, the record's commit is where the
  job's own commits begin: those past it are read for their language and checked.
- **A record commit the repository does not hold.** Where the record's commit is absent from the repository the
  close runs in or is no ancestor of its `HEAD`, the close takes the job's own base commit instead: the last
  commit `HEAD` shares with the branch the checkout was made from, read from the branch its git configuration
  names for it, else `main`, else `master`, else the remote branch it follows. The close says in one line
  which range it used. Where no such base can be read, it is refused with a reason naming the record's commit
  and the repository.

## What gates a deployment

- **The static check and the suite** run before a deployment, once for every deployment, as bmo-testbed's gate
  does for every repository: the suite is the deployment's business and no close runs it, so a batch carrying
  many commits runs it once over all of them. The suite is the Cargo workspace's tests and the TypeScript
  package's `check` script (`[test] languages`, its dependencies installed by `[test] setup`), and the quick
  checks are the layout and the dependencies (`[check]`), beside this repository's own checks in `tests/`
  (docs/checks.md).
