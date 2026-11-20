---
name: bmo-job
description: Take work from a job service and do it, through the bmo-job MCP server. Use when work is kept as items in a job service's queues, when told of an item opened in a queue you serve, when picking up the next piece of work, or when reporting on, asking about or closing an item you hold.
---

# bmo-job

A job service keeps work as items waiting in queues. It never does the work itself: whoever does it is told
of an item, takes it with a lease, does it, reports on it and closes it. The `bmo-job` MCP server reaches one
service through its API, one tool for each step, and keeps the leases and subscriptions you take alive while
the server runs.

## Starting the server

Register the server with the harness as a stdio MCP server:

```
bmo-job mcp --service http://127.0.0.1:7841
```

The address is the service's base address. The bearer token goes in the `BMO_JOB_TOKEN` environment variable
of the server, or `--token`; a wrong one answers every tool with a 401 `unauthorized` error. The token is the
caller every call is made as: the shared token, or a caller's own, under whose name the service records what
the token's caller approves.

To be told of items from the start, name the queues you serve and the name you hold work under:

```
bmo-job mcp --service http://127.0.0.1:7841 --holder builder-1 --queue build --queue review
```

Where the harness takes messages pushed into its session on an experimental capability, give that capability
with `--channel`, and each event also arrives as such a message.

## Being told of work

`job_subscribe` with your `holderName`, the `queueNames` you serve and the `kinds` to hear of (`item.created`
and `item.changed` for new and reopened work, `note.created` for words added to an item, `reply.created` for
answers to your questions, `lease.expired` for a lease you lost). Leave `url` out: the server reads the
subscription itself and tells you of each event as a `notifications/resources/updated` on
`job://items/{itemId}` or `job://queues/{queueName}`, which you read as a resource. Give a `ttlMs`, such as
60000: the server renews it while it runs, and when the server stops nobody is told of work on your behalf.

A notification hands nothing over. Several harnesses may be told of the same item; the one that takes its
lease first has it.

## Taking an item

`job_lease_next` with `queueName`, `holderName` and `ttlMs` takes the next open item of the queue, or, with
`itemId`, the item a notification named. `lease` is null when nothing waits; give `waitMs` (at most 60000) to
wait for one. A 409 `conflict` means another holder took that item first: leave it and take the next. An item
assigned to a name is only taken by a lease asking with that same `assigneeName`.

The lease holds the item for `ttlMs` (at most 3600000). The server renews it at half its span while it runs;
`job_lease_renew` renews it yourself. A lease that runs out gives the item back to the queue for someone else.
A lease may hold fixed what of its item its holder read to start the work, `frozen` on `job_lease_next` or
`job_lease_renew`: item fields and label keys, with the holder's reason. While it stands, a change of the item
that would give any of them another value is refused 409 `item-frozen` with that reason.

## Doing it

1. `job_item`: the title, the brief, who asked (`requesterName`) and the caller's `labels`.
2. `job_work_order`: the purpose, the directive and the steps, where the item has a work order (a 404 means it
   has none). `job_tasks` with `current: true` lists the tasks of the stage under way.
3. `job_notes`: words added after the item was raised. Pass the last note's `id` as `after` and a `waitMs` to
   wait for the next one, or subscribe to `note.created` on the item.
4. `job_report` with `kind: "progress"` as you go, so whoever watches the item knows where it stands.
5. Blocked on a decision that is not yours: `job_question_ask` with the `itemId`, your name as `askerName`,
   the `text` and the `choices` (empty for an open question, which only the supervisor takes), then
   `job_replies` with a `waitMs` until a reply settles it. Write it for someone who knows nothing of the work, since
   it may reach a person: what the work is, what each choice does, how the choices differ and what follows under
   each, with no step number or internal term left unexplained. Keep the lease meanwhile. Give `recommendation` as `{answer, why}` only where you can make the
   case for that answer on the merits of the work and the rules; never the safe or timid answer by default.
   Leave it out where you cannot make the case. Where your token is not a caller the deployment marks as
   supervising the work, the question goes to the supervisor, which settles it or puts it to a person, and it
   waits for the supervisor however long that takes. A question asked of a person offers two choices or more, or
   is refused 422 `no-choices`. `waitMs` says how long a question asked of a person waits for him, a minute where
   left out, so name one he can meet: then its recommendation settles it, as a reply with `source`
   `recommendation`, or, with none, it is told of as due and waits on. Name a choice in `recommendation.answer`
   word for word, with no mark beside it.
6. Holding a question put to the supervisor (`addressee` `supervisor`), where your token is a caller the
   deployment marks as supervising the work: do or find what it asks and settle it with `job_reply`, what you did
   or found as its `text`; talk it over with its asker by a `job_reply` that settles nothing; or, where the
   choice is a person's to make, put it to a person with `job_question_change`, `addressee` `person`, and the
   `text`, two to four `choices` and a `recommendation` in your own words, written for someone who knows nothing
   of the work: what the work is, what each choice does, how they differ and what follows under each. A move with
   fewer than two choices is refused 422 `no-choices`, and one with no recommendation 422 `no-recommendation`.
   Give `waitMs` for how long the person has, a minute where left out, so name one he can meet: once it passes
   unanswered, the recommendation settles the question, as a reply with `source` `recommendation`. A question put
   to a person that nobody has settled, which you would rather settle yourself, you take back with
   `job_question_change`, `addressee` `supervisor`, and it is yours again.

## A base library missing a part

Where the item needs something a base library this repository imports does not serve, stop only that part
and go on with the rest. Do not copy the missing part here.

1. Make a branch in the base library's own checkout, from its main, and push it to the base library's
   GitHub repository. Never commit to its main.
2. Write the change as the general case: no name of this repository, no provider or unit name, no branch
   for one unit, and no data or test fixture only this repository uses. Bring the base library's own tests.
3. Open a pull request from that branch against the base library's main. Check the rule in step 2 again
   before opening it, and again when the review comes.
4. Whoever holds the base library's work reviews it, merges it or sends it back with the reason, and raises
   the base library's version on merge. Pin that version here and resume the stopped part.
5. Where the review is still open when the item ends, report the pull request and say which part waits on
   it; that part is not done.

## Ending it

- **Done:** `job_report` with `kind: "final"` and the account of the work, then `job_item_update` with
  `state: "done"`. The lease ends with it.
- **Cannot go on now:** `job_item_update` with `state: "held"` and `reason` saying why, in the one change. The
  item keeps the reason as its `hold`, and the same change writes it as the item's `held` report; a hold without
  a reason is refused 400 `reason_required`. Whoever can release it sets it back to `open`.
- **Giving it back undone:** `job_lease_release`; the item is open again for the next taker.

## Raising work

`job_item_create` raises an item as a `draft` in a queue; it is taken only once it is approved. `job_item_update`
with an `approval` (`{}`, or with `at`) gives an approval, and it takes no name of who approves: the service
records it under the caller whose token this server was started with. Only where the deployment marks that
caller as an approver is it of the kind `approval`, which opens the draft; otherwise it is a `recommendation`,
recorded on the item for an approver to see, and the item stays a draft. The item's `approvals` say which
each one is.

`job_items` lists items by `state`, `queueName`, `assigneeName`, `scope`, `label` or words (`q`), in queue order;
`job_queues` lists the queues, whether each is paused or held, and how many items are open and leased in it.

## Failures

A refused call comes back as a tool error whose text is the service's `status`, `code` and `error` sentence.
`not-found` names nothing at that key; `conflict` is a move the member's state does not allow; `gone` is a
lease or subscription that has ended; `queue-not-open` is a queue paused or under a hold; `not-the-addressee` is a
question whose addressee your token may not settle or move; `label_value_unknown` is a label value off the list
of values published for its key, the sentence naming the values it takes.
