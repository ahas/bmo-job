---
name: bmo-job
description: The bmo-job MCP tools of a job service's queue, each with its required arguments and its refusals: reading the queue and an item, changing an item (approve, hold, drop, merge, reorder, switch its notices off), notes, reports, leases, questions and their replies, reviews and subscriptions.
---

# bmo-job

A job service keeps work as items in queues. It does no work itself: a holder takes an item with a lease, does the
work, reports on it and closes it. The `bmo-job` MCP server calls one service, one tool a route, as the caller its
token names; the deployment starts it, so never start or address it yourself.

## How a call answers

- A call answers the route's JSON. A refusal comes back as a tool error (`isError` true) whose text is the
  service's status, `code` and sentence. Read the sentence: it names what is wrong and, where there are any, the
  values to choose from. Correct the one thing it names and call again; never call the same thing unchanged.
- `not-found`: nothing has that key; read the list again. `conflict`: the item's state does not allow the move,
  or another holder took it first; read the item and choose again. `gone`: the lease or subscription ended; take a
  new one. `queue-not-open`: the queue is paused or under a hold; say so and leave it. `not-the-addressee`: your
  token may not settle or move that question. `label_value_unknown`: a label value off its key's list; the
  sentence names the values. `reason_required`: a hold with no reason. `item-frozen`: a lease holds that field
  fixed; the sentence says why.

## Reading the queue

- `job_items`: items in queue order, filtered by `state` (one, or several comma-separated), `queueName`,
  `assigneeName`, `scope`, `label` (`key` or `key=value`), `ids`, `raisedBy` or words (`q`); `limit` default 50.
  Example: `job_items {"state": "draft,open", "label": "series=release"}`.
- The order is `priority` from highest, then `position` from lowest, then `id`. An item filed with no priority
  takes 0.
- `job_item {"itemId": 4100}`: one item whole: title, brief, `requesterName`, `labels`, state, lease, approvals.
- `job_work_order {"itemId": 4100}`: its purpose, directive and steps; 404 means it has none.
  `job_tasks {"itemId": 4100, "current": true}`: the tasks of the stage under way.
- `job_queues`: every queue, whether it is paused or held, and how many items are open and leased in it.

## States and moves

- `draft` waits for an approval; `open` waits for a lease; `leased` is a holder's; `held` is set aside; `done`,
  `dropped` and `merged` are final.
- An approval moves a draft to open. A lease moves an open item to leased; giving it back or letting it run out
  moves it to open. `held` sets an open or leased item aside, and `open` takes it back. `done` closes a leased
  item. Any unfinished item can be dropped, or merged into another. A move the state does not allow answers
  `conflict` with the state it stands in.

## Changing an item

`job_item_update` is a merge patch: `itemId` and only the fields you change. A field left out stays; a nullable
field set to null is cleared. `labels` is the exception: it replaces the item's labels whole, so send every label
the item keeps.

- Approve a draft: `job_item_update {"itemId": 4100, "approval": {}}`. The service records the approval under
  your token's caller; it opens the draft only where the deployment marks that caller as an approver.
- Hold, with the reason in the same change (a hold without one is refused `reason_required`):
  `job_item_update {"itemId": 4100, "state": "held", "reason": "waits for the database move"}`. A standing lease
  ends. Open it again with `"state": "open"`.
- Drop a request that should never have been filed: `"state": "dropped"`.
- Merge into the item that carries the work: `{"itemId": 4100, "state": "merged", "mergedInto": 4090}`.
- Reorder: `priority` (higher first) or `position`. The service never renumbers the others.
- Switch the item's standing notices off with a reason: `{"itemId": 4100, "noticesOff": {"reason": "waits for
  the card upgrade"}}`; on again with `"noticesOff": null`. A switch-off without a reason is refused.
- Change title, brief, `assigneeName`, `requesterName`, `queueName` or `scope` the same way.
- Set or take off a few labels on one or many items without sending the rest: `job_items_relabel {"ids":
  [4100, 4101], "labels": {"model": "model-b", "follows": null}}`. A key given null is taken off; every other key
  stays. An item a lease holds fixed comes back under `refused` with why.

## Notes and reports

- `job_note_add {"itemId": 4100, "text": "...", "actorName": "harness"}`: one more condition or a record on an
  item that stands. `job_notes {"itemId": 4100}` reads them; pass the last note's `id` as `after` and a `waitMs`
  to wait for the next.
- `job_report {"itemId": 4100, "kind": "progress", "body": "...", "actorName": "harness"}`: where the work
  stands. `kind` `final` carries the account of finished work. `body` and `actorName` are required.
- `job_reviews {"itemId": 4100}`: the item's whole review in order: every report, question, answer, rejection and
  pass; `ref` narrows it to one job.

## Leases

- `job_lease_next {"queueName": "todo", "holderName": "harness", "ttlMs": 900000, "itemId": 4100}` takes the one
  item named; without `itemId` it takes the next open item. `ttlMs` is 1000 to 3600000.
- An item with an assignee is taken only by a lease that names the same `assigneeName`; leave `assigneeName` out
  for an item assigned to no one.
- `{"lease": null}` means nothing waits: it is no failure. `waitMs` (at most 60000) waits for one.
- `conflict` on a named item: another holder took it first. Leave it; launch nothing.
- `job_lease_renew {"leaseId": ..., "ttlMs": 900000}` renews; the server also renews at half the span while it
  runs. `job_lease_release {"leaseId": ...}` gives the item back, open for the next taker.
- One lease stands on an item at a time: an item already leased is never taken again.
- `frozen` on a lease holds item fields or label keys fixed while it stands, with your reason.

## Questions

- Ask a person: `job_question_ask {"itemId": 4100, "askerName": "harness", "text": "...", "choices": ["...",
  "..."], "recommendation": {"answer": "<one choice word for word>", "why": "..."}}`. Give two to four choices;
  fewer are refused `no-choices`. Leave `waitMs` out: the deployment's question wait, a minute unless its settings
  name another, applies, and a longer one is cut to it. Once it passes unanswered, the recommendation settles the
  question as a reply with `source` `recommendation`.
- Write it for someone who knows nothing of the work: what the work is and why it stopped, what each choice does,
  how they differ and what follows under each, with no step number or internal name.
- `job_question {"questionId": ...}` reads one; `job_replies {"questionId": ..., "waitMs": 60000}` reads and
  waits for its replies.
- Holding a question put to the supervisor, you have the question wait from the moment it reached you, and
  nothing you write lengthens it. Within it, do one of three:
  - settle it: `job_reply {"questionId": ..., "text": "<what you did and what came of it, in English>",
    "actorName": "harness", "settles": true}`;
  - talk it over with its asker: the same call with `"settles": false`;
  - put it to a person: `job_question_change {"questionId": ..., "addressee": "person", "text": "...",
    "choices": [...], "recommendation": {...}}`, with no `waitMs`. Fewer than two choices are refused
    `no-choices`, and no recommendation `no-recommendation`.
- Once the wait passes with the question still yours, the service puts it to a person as its asker wrote it.
- A question with a person that nobody settled you take back with `job_question_change {"questionId": ...,
  "addressee": "supervisor"}`; settle it in the same turn, since taking it back buys no time.

## Raising work

- `job_item_create {"title": "...", "brief": "...", "queueName": "todo"}` raises a draft, taken only once it is
  approved. Where the deployment files items through a tool of its own, use that one instead.

## Subscriptions

- `job_subscribe {"holderName": "harness", "queueNames": ["todo"], "kinds": ["item.created", "item.changed"],
  "ttlMs": 60000}` tells you of each event as a resource update on `job://items/{itemId}` or
  `job://queues/{queueName}`; leave `url` out. Kinds: `item.created`, `item.changed`, `note.created`,
  `reply.created`, `lease.expired`. `job_unsubscribe {"subscriptionId": ...}` ends it. A notice hands nothing
  over: whoever takes the lease first has the item.
