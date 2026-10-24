# A base library the work needs

The service stores work and never executes. This document states the road a job takes when the work it holds
needs something a base library does not serve, and the rule its pull request keeps.

## The road

- **The part stops; the rest goes on.** A job whose item needs a base library's part that is not there stops
  only that part, records it, and carries the rest of the item to its end. It does not hold the whole item for
  the base library, and it never copies the missing part into its own repository.
- **A branch in the base library, never its main.** The change is made on a branch of the base library's own
  checkout, made from its main, and the branch is pushed to the base library's GitHub repository. The job
  never commits to that main: the base library's main takes the change only through the pull request's merge.
- **A pull request on GitHub.** The branch is opened against the base library's main as a pull request. The
  pull request states the general capability, not the item that asked for it.
- **The change is the general case.** It is written for any consumer of the base library, with the base
  library's own tests. What is specific to the repository that asked stays there, or is reshaped into the
  general form the base library can serve.
- **The base library's worker decides.** The worker holding the base library's work reads the pull request
  and merges it or sends it back with the reason on the pull request. `bmo-review` may assist the reading.
- **A merge raises the base library's version.** The merge raises the base library's semver, and the version
  that serves the change is the one the asking repository pins.
- **The asking repository pins and resumes.** It takes the new version, pins it, and resumes the part it
  stopped. Until then that part waits; the rest of the item does not.

## The rule its pull request keeps

The rule is checked before the pull request is opened and again when it is reviewed. The change:

1. names no repository that asked for it;
2. names no provider or unit of one of them;
3. carries no branch for one unit;
4. carries no data or test fixture that only the asking repository uses;
5. is written as the general case, and comes with the base library's own tests.

A pull request that fails any of these is sent back rather than merged.

## When the review sends it back

The branch stays. The asking job fixes it on the same branch, answers the reason on the pull request, and asks
for the review again. The rest of its item goes on while it waits. Where the review is still open when the item
ends, the item's report names the pull request and says which part waits on it; that part is not reported done.

## The phase prompt

A work order's step that opens this road is a plain-text phase prompt, written like this one and filled in for
the item:

```text
A part of this item needs a change in <base library> that it does not serve.
Stop that part only; the rest of the item goes on.
Make a branch in <base library> from its main, and open a pull request against its GitHub repository.
Write the general case, with the base library's own tests. The change names no repository that asked for it,
no provider or unit of one, no branch for one unit, and no data or test fixture only this repository uses.
Never commit to the base library's main, and keep no copy here.
When its worker merges it and raises its version, pin that version here and resume the stopped part.
```

## What this service does not do

The service holds the item, its notes, its report and its questions; it opens no pull request, runs no
command and calls no other unit. The road is the job's, and this document states it for the job.
