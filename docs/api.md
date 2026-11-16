# The API

**Fixed at 0.1.0-rev.8.** This document and the OpenAPI document generated beside it, `docs/openapi.json`, are
the service's contract; the version they carry is the repository's (README.md, "Version"). The common wire format, the path and method rules
and the error table are the bmo conventions bmo-chat keeps; this document states what the service holds and
serves on them, and repeats none of it.

The service is a todo kept as middleware, in the way Redis is one: it stores items, their notes, reports,
reviews and work orders, the queues the items wait in, leases on items, questions and their replies, and subscriptions,
and serves them through this API and nothing else. It never executes: it launches no process or run, calls no
other unit and registers nothing anywhere. The one call it makes is a delivery to the address a subscriber
registered. Who a member belongs to is opaque to it: `scope`, `assigneeName`, `requesterName`, `holderName`,
`askerName` and `actorName` are the caller's strings, and `labels` the caller's metadata, none of which it reads
but for one thing: an item raised with a label the settings name unattended opens at once (Items, "Unattended
labels").
The names it does not take from a body are the caller's the request's token names: who gave an approval
(Items, "Approvals"), who changed an item as its events tell it (Events), and who holds a lease (Leases).

## Access

- **Every route takes a bearer token** the settings name (docs/settings.md), `/health` included: the shared
  token, or the token of one of the callers the settings list. A request without one of them is answered 401
  `unauthorized`.
- **The token names the caller.** A request with a caller's own token is that caller's, known to the service
  by the caller's name in the settings; a request with the shared token is no named caller's. Every route
  answers either alike but for four readings of who the caller is: the kind of an approval it gives (Items,
  "Approvals"), the `changedBy` the events of an item it writes carry (Events), the `raisedBy` an item it
  raises takes from the lease it holds (Items, "Raised from an item"), and whether it may settle a
  question put to the supervisor, ask a person, put a question to a person or take it back (Questions and
  replies, "The addressee").
- **A method and path no route answers** is answered 404 `no-route`.
- **A body or query not of the route's shape** is answered 400 `bad-request`, the sentence naming what is
  wrong.

## Lists

- **A list is a page,** `{"items": [...], "more": boolean}`, read with `after` and `limit`.
- **`limit`** is 50 where it is left out and at most 500; a larger one is taken as 500.
- **`after`** is the key of the last member the caller read, and the page continues past it in the list's own
  order. Items are in queue order, tasks by stage and then key, queues by name (their `after` is a name), and
  every other list in key order, oldest first. An item or task key that names nothing in its list is
  answered 400 `bad-request`.
- **A filter taking several values** takes them comma-separated (`state=open,leased`).

## Waiting for an answer

`GET /items/{itemId}/notes`, `GET /questions/{questionId}/replies` and `POST /leases` take `waitMs`. Where
nothing past the cursor stands yet, or nothing waits to be leased, the answer is held open until something
does or `waitMs` runs out, and then given as it stands then. `waitMs` is at most 60 000; a larger one is
taken as 60 000.

## Items

An item is one piece of work: a title, a brief, the queue it waits in and its place there, who asked for it
and who is to take it, the caller's scope and labels, its approvals and its state.

| Field | Holds |
|---|---|
| `id` | The item's key |
| `title`, `brief` | What the work is, as the caller wrote it |
| `state` | One of `draft`, `open`, `leased`, `held`, `done`, `dropped`, `merged` |
| `queueName` | The queue it waits in |
| `priority`, `position` | Its place in the queue (below) |
| `assigneeName` | Who alone may lease it, or null for anyone |
| `requesterName` | Who asked for it, or null |
| `scope`, `labels` | The caller's opaque scope, or null, and its metadata |
| `approvals` | Each approval given, `kind`, `callerName` and `at`, oldest first (below) |
| `rejections` | How many times its work was sent back: the `rejection` entries of its review (Reviews), read and never written |
| `mergedInto` | The item a merged item was folded into, or null |
| `raisedBy` | The item it was raised from, or null (below) |
| `leaseId` | The lease standing on it, or null |
| `createdAt`, `changedAt`, `endedAt` | When it was raised, last changed, and reached `done`, `dropped` or `merged` |

### Its place in a queue

- **A queue's order is `priority` from highest, then `position` from lowest, then `id`.** An item raised
  with no `priority` takes 0; its `position` is one past the highest in its queue, so it goes last among
  its priority.
- **A caller moves an item** by changing either number. The service never renumbers other items.
- **`GET /items` answers in this order,** across queues where no queue is named, and `after` continues past
  the named item's place in it.

### Approvals

- **An approval is given by a change carrying `approval`,** which holds at most `at`, the time it was given,
  the time of the change where left out. An `actorName` in it is read and has no effect.
- **Who gave it is the caller the request's token names** (Access), recorded as `callerName`; an approval
  given with the shared token, or recorded with no caller, has `callerName` null.
- **Its `kind` is fixed when it is given,** by the settings the service runs with: `approval` where the caller
  is one the settings mark as an approver (docs/settings.md, `[callers]`), and `recommendation` for any other
  caller and for the shared token.
- **Only an approval of the kind `approval` opens a draft.** A recommendation is recorded beside the others
  and moves nothing: the item stays a draft until an approver's approval reaches it.
- **An approval the store holds with no kind,** such as one imported, reads as `approval` with `callerName`
  null.

### Unattended labels

- **An item raised with an unattended label opens at once.** The settings name the labels whose items need no
  approval (docs/settings.md, `[unattended]`). An item `POST /items` raises with one of them in its `labels` is
  raised `open` rather than `draft`, with no approval recorded; a label named there is `key=value`, held where the
  item's labels hold that key at that value, as the `label` filter reads that shape.
- **It carries a note saying so,** written as it is raised under the actor name `bmo-job`, with the label
  `opened` at `unattended`: that the item opened by itself, the label it matched and the setting that names it.
  The note is told as `note.created` after the item's `item.created`.
- **Only the raise reads them.** Labels a later change gives a draft open nothing; that draft waits for an
  approver's approval as any other does.

### Raised from an item

- **`raisedBy` is the item an item was raised from.** `POST /items` takes it as the key of a standing item; one
  naming no item is answered 422 `unknown-item`. It is fixed when the item is raised: a change carrying it is
  answered 400 `bad-request`. Every read answers it, null where there is none.
- **A raise with none takes it from the caller's lease.** Where `POST /items` carries no `raisedBy` and the
  caller the request's token names holds exactly one standing lease (Leases), the item is raised with that
  lease's item as its `raisedBy`: a holder filing a follow-up out of the item it holds names that item without
  saying so. A caller holding no lease or several, and a request with the shared token, raise it with none.
- **A label is not it.** A label naming an item, such as `raised-by`, is the caller's metadata like any other;
  `raisedBy` is what a reader of the item reads.

### While leased

An item a lease stands on keeps what its holder took it with until that lease ends.

- **It stays in its queue.** A change moving a leased item to another queue is answered 409 `conflict`.
- **What the lease holds fixed stays as it stood.** A lease may hold fixed some of its item's fields and the
  values of some of its label keys (Leases, `frozen`): what its holder read to start the work it holds the
  item for. A change that would give one of them another value, a label key added, removed or given another
  value among them, is answered 409 `item-frozen`, the sentence naming the item, the lease and its holder,
  what the change would have touched, and the reason the holder gave. A change giving them the values they
  hold already is no change to them and goes through, and the rest of the item changes as any other's.
- **The check reads the lease as it stood before the change,** so a change that also moves the item and ends
  its lease is refused alike: what is held fixed is given another value only once the lease has ended, by the
  holder giving it back or by a change of its own.

### States and moves

| From | To | By |
|---|---|---|
| `draft` | `open` | an approval of the kind `approval`: a change carrying `approval` from an approver, with or without `state: "open"` |
| `open` | `leased` | a lease, `POST /leases`, and by nothing else |
| `leased` | `open` | the lease given back or run out |
| `open`, `leased` | `held` | `state: "held"`; a lease standing on the item ends |
| `held` | `open` | `state: "open"` |
| `leased` | `done` | `state: "done"`; the lease ends |
| any but `done`, `dropped`, `merged` | `dropped` | `state: "dropped"`; a standing lease ends |
| any but `done`, `dropped`, `merged` | `merged` | `state: "merged"` with `mergedInto`; a standing lease ends |

A move not in the table is answered 409 `conflict`, among them `state: "open"` on a draft with no approval of the
kind `approval`, a recommendation in the same change included, and `state: "done"` on an item no final report
stands on: a finished item always carries the report of how it finished, written before it closes. `done`,
`dropped` and `merged` are final. A merge
names a standing item other than itself, neither dropped nor merged; one naming no item is answered 422
`unknown-item`, and one naming an item that cannot take it 409 `conflict`. An approval or a recommendation given
to an item past `draft` is recorded beside the others and moves nothing.

### Routes

| Method and path | Does |
|---|---|
| `GET /items` | Items in queue order, filtered by `state`, `queueName`, `assigneeName`, `scope`, `label` (a key, or `key=value`), `ids`, `raisedBy` (the items raised from that item) and `q` (words found in the title or the brief, in any case) |
| `POST /items` | Raises an item as a `draft`, or `open` where it carries an unattended label (above): `title`, `brief` and `queueName` required, `priority`, `assigneeName`, `requesterName`, `scope`, `labels` and `raisedBy` optional, `raisedBy` taken from the caller's one standing lease where it is left out (above). A `raisedBy` naming no item is answered 422 `unknown-item`. Answers 201 and the item |
| `GET /items/{itemId}` | One item |
| `PATCH /items/{itemId}` | Changes an item as a merge patch: `state`, `approval` (`at`, which is the time of the change where left out; the caller is the token's), `mergedInto`, `title`, `brief`, `queueName`, `priority`, `position`, `assigneeName`, `requesterName`, `scope`, `labels`. A nullable field set to null is cleared. A leased item changes as "While leased" (above) says |

A queue comes to stand with the first item raised or moved into it. A queue name is 1 to 64 letters, digits,
dots, underscores and hyphens.

## Notes

A note is words added to an item after it was raised: `id`, `itemId`, `text`, `actorName`, `labels`,
`createdAt`.

| Method and path | Does |
|---|---|
| `GET /items/{itemId}/notes` | The item's notes in key order, past `after`; `waitMs` waits for one |
| `POST /items/{itemId}/notes` | Appends a note: `text` and `actorName` required, `labels` optional. Answers 201 |
| `DELETE /items/{itemId}/notes/{noteId}` | Takes a note back. Answers 204 |

## Reports

A report is what the one doing an item tells of it: `id`, `itemId`, `kind` (`progress`, `final` or `held`, why
the item waits), `body`, `actorName`, `labels`, `createdAt`. A report moves no state: the one reporting
changes the item itself. Its `body` is kept and answered exactly as it was written, in whatever language the one
reporting wrote it, whole: the service cuts nothing of it and reads none of its words, so a worker's account in
the requester's language stands as the worker wrote it, and a screen shows it as it stands.

| Method and path | Does |
|---|---|
| `GET /items/{itemId}/reports` | The item's reports, oldest first |
| `POST /items/{itemId}/reports` | Appends a report: `kind`, `body` and `actorName` required, `labels` optional. Answers 201 |

## Reviews

A review is the record of how the work done on an item was examined before it was taken, one entry for each move
of it, in the order they were made. An entry is `id`, `itemId`, `ref` (the caller's reference of the run whose
work is examined, or null), `stage` (the stage of the item's work order the entry is about, or null for the work
whole), `round` (1 upward: the turn of the examination it belongs to), `kind`, `body`, `actorName`, `labels` and
`createdAt`.

| Kind | The entry is |
|---|---|
| `report` | What the one doing the work tells of it |
| `question` | A question the one examining it puts to the one doing it |
| `answer` | The answer the one doing the work gives to a question |
| `rejection` | The work sent back: why, and what is to be done |
| `pass` | A part of the examination passed, the part its `labels` name |
| `escalation` | The work put to a person, and why |
| `ruling` | The person's word on it |
| `accepted` | The work taken whole: the review stands closed |

- **An entry moves nothing,** as a report moves nothing: whoever examines the work moves the item itself. Its
  `body` is kept and answered whole, exactly as written.
- **A review stands open** from an item's first entry until its last entry is `accepted`; an entry written after
  an `accepted` one opens it again, as a rerun of the item's work does.
- **The item answers how often its work was sent back,** `rejections`, counted over every entry of its review.
- **`stage` is 1 upward and `round` is 1 upward;** either out of that range is answered 400 `bad-request`, and a
  `round` left out is 1.

| Method and path | Does |
|---|---|
| `GET /items/{itemId}/reviews` | The item's review, oldest first, past `after`; `ref` narrows it to one run's |
| `POST /items/{itemId}/reviews` | Appends an entry: `kind`, `body` and `actorName` required, `ref`, `stage`, `round` and `labels` optional. Answers 201 |

## Work orders and tasks

An item has at most one work order: `itemId`, `purpose`, `directive`, `steps` (lines of the plan, in order),
`state` and `changedAt`. Each step is a task: `id`, `itemId`, `stage`, `title`, `state` (`waiting`,
`current`, `done` or `skipped`), `assigneeName` and `changedAt`.

- **Steps make the tasks.** When a work order is saved with steps, the step at place *n* is the task of stage
  *n* titled by the step. A step that repeats a title already in the list is taken once, at its first place,
  so a list written twice makes its tasks once. A task whose title stands among the new steps keeps its key,
  state and assignee and takes the new step's stage; a step with no such task makes a new `waiting` one; a
  task whose title is gone is removed.
- **The steps stand fixed while the item is leased.** The steps are the plan its job runs in, so a `PUT` or a
  `PATCH` that would change them while the item is `leased` is answered 409 `conflict`, nothing written; the
  same steps written again change nothing and are taken, as is every other field.
- **The stage under way** is the lowest stage holding a task that is `waiting` or `current`.
- **A work order's moves:** `draft` to `saved` and back, `saved` to `running` and back, `running` to `done`,
  and `done` to `running` when it is reopened. Any other is answered 409 `conflict`. A task's move is any
  state to any other, and to any stage, except while its work order is `done`, which is answered 409
  `conflict`.

| Method and path | Does |
|---|---|
| `GET /items/{itemId}/work-order` | The work order |
| `PUT /items/{itemId}/work-order` | Saves the work order whole: `purpose`, `directive` and `steps` required, `state` optional (the standing state, or `draft` for a new one). Answers 201 when it is new, 200 when it replaces one |
| `PATCH /items/{itemId}/work-order` | Changes it as a merge patch: `state`, `purpose`, `directive`, `steps` |
| `GET /items/{itemId}/work-order/tasks` | Its tasks, by stage and then key; `current=true` for the stage under way alone |
| `GET /items/{itemId}/work-order/tasks/{taskId}` | One task |
| `PATCH /items/{itemId}/work-order/tasks/{taskId}` | Moves a task: `state`, `stage`, `assigneeName` |

## Queues

A queue is `name`, `state` (`open` or `paused`), `holds`, `openCount` and `leasedCount` (its items in those
states), and `changedAt`. A hold is `reason`, `actorName` and `until`, a time after which it no longer
stands, or null for one that stands until it is released.

- **A queue that is paused or under a standing hold gives no lease.** Its next item is not taken, and a lease
  asked for one item of it is answered 422 `queue-not-open`.
- A hold whose `until` has passed is dropped from the queue, which is told as `queue.changed`.

| Method and path | Does |
|---|---|
| `GET /queues` | Every queue, by name |
| `GET /queues/{queueName}` | One queue |
| `PATCH /queues/{queueName}` | Pauses or opens it with `state`, and replaces its holds whole with `holds`: a hold added or released is the list with or without it |

## Leases

A lease is an item held by one holder for a span: `id`, `queueName`, `itemId`, `holderName`, `ttlMs`,
`expiresAt`, `createdAt`, `frozen` and `item`, the item as it stands. A lease runs out at `expiresAt` unless it is
renewed; one that runs out gives its item back to `open` and is told as `lease.expired`. An ended lease,
given back, run out or ended by its item's move, is answered 410 `gone` from then on.

- **A lease is held by the caller whose token took it** (Access): one taken with a caller's own token is that
  caller's, one taken with the shared token no named caller's. `holderName` is the caller's string and is not
  read for it. The service reads who holds a lease only to give an item a caller raises its `raisedBy` (Items,
  "Raised from an item").

- **`POST /leases` takes the next open item of `queueName`** for `holderName`, for `ttlMs`, which is 1 000 to
  3 600 000. An item with an `assigneeName` is taken only by a lease asking with that same `assigneeName`; one
  without, by any lease. Where nothing waits it answers 204, at once or once `waitMs` runs out.
- **With `itemId` it takes that item,** the one a push named. An `itemId` naming no item is answered 422
  `unknown-item`. The item must be in `queueName` (400 `bad-request` otherwise) and `open`: one that another
  holder took first, or that is in any other state, is answered 409 `conflict`, so harnesses told of the same
  item race for it and one wins.
- **`frozen` is what of its item the lease holds fixed** while it stands (Items, "While leased"), or null for
  nothing: `fields`, item fields among `title`, `brief`, `queueName`, `priority`, `position`, `assigneeName`,
  `requesterName` and `scope`; `labels`, label keys, a key the item does not carry held fixed as absent; and
  `reason`, the holder's own sentence for why, or null. The holder gives it as it takes the lease or as it
  renews it, and every reader of the lease reads it. A field outside that list, or a label key that is empty,
  is answered 400 `bad-request`.

| Method and path | Does |
|---|---|
| `POST /leases` | Takes an item as above, `frozen` optional. Answers 201 and the lease |
| `GET /leases` | The leases standing now, filtered by `holderName` and `queueName` |
| `GET /leases/{leaseId}` | One lease |
| `PATCH /leases/{leaseId}` | Renews it: `ttlMs` from now; `frozen`, where it is carried, set in place of what stood, null holding nothing fixed |
| `DELETE /leases/{leaseId}` | Gives it back; its item is `open` again. Answers 204 |

## Questions and replies

A question is asked about an item or on its own: `id`, `itemId`, `askerName`, `scope`, `addressee` (who it is
put to, below), `text`, `choices` (empty for an open question), `recommendation` (the answer the asker would take
by itself, and why, or null), `state`, `dueAt`, `toldByAsker` (whether the asker puts it before whoever answers it
itself), `toldAs` (the asker's own reference to the notice it put the question before whoever answers it with,
opaque to the service, or null), `heardThrough` (the last reply its asker has been handed, or null), `readdressedBy`
and `readdressedAt` (the caller that put it to a person in the supervisor's place, and when, or null) and
`createdAt`. A reply is `id`, `questionId`, `choice`, `text`, `actorName`, `settles`, `source` and `why` and
`createdAt`. A reply is written with `source` `supervisor` where the caller supervises the work and `person`
otherwise, and `why` null; the type also reads `recommendation` with that recommendation's reason, which a reply
kept in the store may carry. The source is the voice a reply is heard in, whatever `actorName` says.

- **States:** `open`, `paused`, `answered`, `withdrawn`, `unsent`. `open` and `paused` move to each other, to
  `withdrawn` and to `unsent` by a change; a settling reply moves `open` or `paused` to `answered`. Any other
  move is answered 409 `conflict`. `withdrawn` is a question its asker took back or stopped waiting on; `unsent`
  is one that never reached anybody, because putting it to them failed. Neither waits for an answer, and neither
  is ever answered.
- **The due time.** `waitMs` is how long the asker lets the question go unanswered by a person before it is told
  of as due, from 1 000 to 86 400 000 milliseconds, and 60 000 where left out. A question put to a person
  has its `dueAt` that far from when it was asked; one put to the supervisor has none while the supervisor holds
  it, and is never told of as due there, and once the supervisor puts it to a person its `dueAt` is that far from
  that moment. Pausing a question keeps the time it had left and reads `dueAt` as null; opening it again sets
  `dueAt` that far from then, or a whole `waitMs` from then where it had no time left, as a question paused past
  its due time has. A question is told of as due once each time it is put to a person: a pause and an opening
  again tell nothing of a question already told of, whose due time has passed, and the `dueAt` it reads after
  opening again passes untold. The due time paces a notice and nothing more: a question waits for its answer
  however long that takes.
- **The recommendation.** `recommendation` is `{answer, why}`: `answer` is one of the question's choices or a
  free answer where the choices do not cover it, and `why` is the one line saying why that is the answer. It is
  shown to whoever answers, and taken only where a person settles the question with it. The
  asker sets it only where it can make the case for that answer on the merits of the work and the rules; it is
  never the safe option by default, and it is left null where it cannot make the case. An answer and a reason
  are both required where it is given.
- **When the due time passes** on an open question, the service tells `question.due` once and the question stays
  open for a person, with or without a recommendation. Nothing settles it but a settling reply or a withdrawal.
- **One question, one row.** Every `POST /questions` opens a row of its own, so an asker that may ask the same
  thing again reads the item's questions first and stands by the one it finds. `bmo job ask` does so in every
  state: it waits on one `open` or `paused`, answers with the line one `answered` was settled with, and opens
  a new row only where every one it finds stands `withdrawn` or `unsent` (docs/cli.md, 「`bmo-job job ask`」).
- **A question's scope** is the one asked with, or the item's where it is left out.
- **The addressee.** A question is put to `person`, a person who settles it, where the ask names no other; or to
  `supervisor`, the caller that supervises the work under the question's scope, which judges it first: settles it
  itself with what it did or found, talks it over with the asker, or puts it to a person in its own words. Only the
  supervisor puts a question before a person. The service reads the addressee in six places and nowhere else:
  - **who asks a person:** a question is opened for a person only by a caller the settings mark as supervising;
    one any other caller asks for a person, by naming `person` or no addressee, is opened for the supervisor,
    with no due time, as one that caller had put to the supervisor;
  - **who settles it:** a settling reply to a question put to the supervisor is taken only with the token of a
    caller the settings mark as supervising (docs/settings.md, `[callers]`), and one to a question put to a person
    only from a caller that answers for a person: the shared token or a caller the settings do not mark as
    supervising. Any other is answered 403 `not-the-addressee`, so the supervisor never answers what it put to a
    person. A word that settles nothing is taken from anyone, as on every question, and is heard in its caller's
    voice: a supervisor's word on a question put to a person is the supervisor's, never the person's;
  - **who puts it to a person:** a change naming `addressee` `person` puts an `open` or `paused` question from the
    supervisor before a person, with that same token alone, any other caller's answered 403 `not-the-addressee`.
    It may carry the question in the supervisor's own words, choices and recommendation in the same change. The
    question then stands `open`, its due time counted from that moment, its `readdressedBy` the caller and its
    `readdressedAt` the moment, and it is told as `question.changed`;
  - **who takes it back:** a change naming `addressee` `supervisor` takes an `open` or `paused` question put to a
    person back to the supervisor while no person has settled it, whoever put it there, with that same token
    alone, any other caller's answered 403 `not-the-addressee`. The question then stands `open` for the
    supervisor with no due time, its `readdressedBy` and `readdressedAt` null, and it is told as
    `question.changed`. A settled question is put nowhere, and a change naming the addressee the question
    already has is answered 409 `conflict`;
  - **its clock:** a question the supervisor holds has no due time (「The due time」 above);
  - **the list's filter,** `addressee`, one or several comma-separated;
  - **the bmo-receiver port,** which reads only the questions put to a person (「The bmo-receiver port」).
- **Who tells of it.** A consumer that watches the events tells of a new question, except one asked with
  `toldByAsker`: that asker puts it before whoever answers it itself, so it can see the failure and close the
  question `unsent`.
- **A change while it waits.** `text`, `choices` and `recommendation` put an `open` or `paused` question again in
  other words, judged as an ask is, and answered 409 `conflict` on any other. `heardThrough` moves on the last
  reply the asker was handed, so a reply said after it is not lost between one wait and the next; it never moves
  back. `addressee` is stated under 「The addressee」 above. `toldAs` is set or replaced as the asker gives it.
  `ifAddressee` makes the whole change conditional: where the question is no longer put to the addressee it
  names, nothing is changed and the change is answered 409 `conflict`, so an asker never withdraws a question
  the supervisor has just put to a person. A change holding none of `state`, `text`, `choices`, `recommendation`,
  `heardThrough`, `addressee` and `toldAs` is `bad-request`.
- **A reply settles the question** when its `settles` says so; left out, a reply settles when it carries a
  `choice` or the question has no choices, and is a word on the question otherwise. A `choice` is one of the
  question's choices or is answered 422 `not-a-choice`. A settling reply to a question already answered or
  withdrawn is answered 409 `conflict`; a word may be added in any state.

| Method and path | Does |
|---|---|
| `GET /questions` | Questions filtered by `scope`, `itemId`, `addressee` and `state` |
| `POST /questions` | Asks: `askerName`, `text` and `choices` required, `itemId`, `scope`, `addressee`, `recommendation`, `waitMs` and `toldByAsker` optional; a caller that does not supervise asks the supervisor whatever addressee it names. An `itemId` naming no item is answered 422 `unknown-item`. Answers 201 |
| `GET /questions/{questionId}` | One question |
| `PATCH /questions/{questionId}` | Pauses it, opens it again, withdraws it or closes it unsent, with `state`; puts it again in other words with `text`, `choices` and `recommendation`; moves what its asker was handed with `heardThrough`; puts a question to the supervisor before a person, or takes one put to a person back to the supervisor, with `addressee` |
| `GET /questions/{questionId}/replies` | Its replies in key order, past `after`; `waitMs` waits for one |
| `POST /questions/{questionId}/replies` | Replies: `text` and `actorName` required, `choice` and `settles` optional. Answers 201 |

## Events

The service's one stream is `GET /events`, in the shape the conventions state. Each event is
`{"id", "kind", "at", "data"}`, `data` being the member as a GET answers it, or its key alone for a `deleted`
kind: `{"id"}`, and `{"id", "itemId"}` for a member nested under an item. A stream opened with no cursor starts
with the events still to come; a page asked with none starts at the oldest the window holds.

- **An item's events say what changed and who changed it.** The data of `item.created` and `item.changed` is the
  item as a GET answers it, with two fields beside its own: `previousState`, the state the item stood in before
  the change, or null where the change left its state where it was, and on `item.created`; and `changedBy`, the
  caller the request's token names (Access), null for the shared token, and `bmo-job` where the service's own
  sweep made the change.
- **Every write of an item's state is told so:** a move of the state table (Items, "States and moves"), an
  approval that opens a draft, a lease taken or given back, a lease the sweep finds run out, and a move that ends
  a standing lease. Each is one `item.changed` naming the state it left.

| Kind | When |
|---|---|
| `item.created`, `item.changed`, `item.deleted` | An item raised, changed in any field or state, or let go with its scope; the first two carry `previousState` and `changedBy` (above) |
| `note.created`, `note.deleted` | A note appended or taken back |
| `report.created` | A report appended |
| `review.created` | An entry appended to an item's review |
| `work-order.changed` | A work order saved or changed |
| `task.changed` | A task moved, made or removed by a save (a removed task is told with its key alone) |
| `queue.changed` | A queue paused, opened or its holds changed, by a change or by a hold running out |
| `lease.created`, `lease.expired`, `lease.deleted` | A lease taken, run out, or given back or ended by its item's move |
| `question.created`, `question.changed`, `question.due`, `question.deleted` | A question asked, moved, past its due time while open, or let go with its scope |
| `reply.created` | A reply given |
| `subscription.created`, `subscription.changed`, `subscription.deleted` | A subscription made, changed, moved to `failing` or back to `active`, or deleted or run out |

- **Narrowing:** `kinds` (comma-separated), `scope`, and `subscriptionId` for the events a subscription
  matches. An event of an item, its notes, reports, review, work order, tasks and leases carries the item's queue and
  scope; a question's carries its scope and item.
- **The window** is seven days: events older are dropped, and a resume from before the window is answered 410
  `gone`.
- **`POST /events`** publishes an event of a process of the service's own: `kind` and `data` required, `kind`
  of the shape `<resource>.<change>`. Its `data`'s `queueName`, `scope` and `itemId`, where it has them, are
  what it is narrowed by. Answers 201 and the event.

## Subscriptions

A subscription is how a caller is told of changes: `id`, `holderName`, `kinds`, `queueNames`, `scope`,
`itemIds`, `url`, `state` (`active`, `paused` or `failing`), `ttlMs`, `expiresAt`, `failureCount`,
`deliveredEventId`, `labels`, `createdAt` and `changedAt`.

- **What it matches:** events of the kinds listed, of the queues listed, under the scope given and of the
  items listed. An empty list or a null scope matches every event on that count; each count that is given
  must match. A kind not among the service's own is answered 400 `bad-request`.
- **Where it starts:** with the first event after it was made. `deliveredEventId` is the last event handed
  over, and delivery goes on past it.
- **Pushed to a `url`:** each matching event is posted there in event order, at least once, as the
  `subscription-event` webhook the OpenAPI document states: the event as the body, the subscription's key in
  the `Job-Subscription` header and, where a `secret` was given, `sha256=` and the lowercase hex HMAC-SHA256 of
  the body under that secret in `Job-Signature`. A 2xx acknowledges it. A post is given a minute for its
  answer; a connection that is refused or breaks, a subscriber gone silent under the TCP keep-alive probe, a
  post with no answer within the minute, or any other answer than a 2xx, is tried again after one second,
  doubling to a minute; ten failures in a row move the subscription to `failing`.
- **A failing subscription is still tried,** one post every five minutes, since a subscriber down for longer
  than the retries above comes back. The first post taken moves it back to `active`, told as
  `subscription.changed`, and delivery goes on at the pace above from the event after `deliveredEventId`;
  setting its `state` back to `active` tries it at once. The subscriber drops an event it has seen by its
  `id`. A subscription whose next event has left the window moves to `failing` at once and stays there, since
  nothing can be delivered in order any more; it is deleted and made again.
- **Read as a stream:** a subscription without a `url` is read on `GET /events?subscriptionId=`, which starts
  past `deliveredEventId` where no cursor is given, and records each event it hands over as delivered. Asking
  the stream of a subscription with a `url` is answered 409 `conflict`. A paused subscription hands nothing
  over until it is active again.
- **Its span:** with `ttlMs` (1 000 to 86 400 000) it runs out at `expiresAt` unless a change renews it: every
  change, an empty one included, sets `expiresAt` to `ttlMs` from then. With none it stands until it is
  deleted. One that runs out is told as
  `subscription.deleted`, and from then on, like one deleted, answered 410 `gone`.
- **Its moves:** `active` and `paused` to each other, and `failing` to either. Setting `failing` is answered
  409 `conflict`; only delivery moves a subscription there, and back to `active` once a post is taken.
- **The secret** is kept for signing and never answered.

| Method and path | Does |
|---|---|
| `GET /subscriptions` | Subscriptions filtered by `holderName` and `state` |
| `POST /subscriptions` | Subscribes: `holderName` required, `kinds`, `queueNames`, `scope`, `itemIds`, `url` (an absolute `http` or `https` address), `secret`, `ttlMs` and `labels` optional. Answers 201 |
| `GET /subscriptions/{subscriptionId}` | One subscription |
| `PATCH /subscriptions/{subscriptionId}` | Renews it for `ttlMs` (the standing span where left out), changes `kinds`, `queueNames`, `itemIds` or `scope`, or sets its `state` |
| `DELETE /subscriptions/{subscriptionId}` | Ends it. Answers 204 |

## Scopes

What the service holds under a caller's scope is the items raised under it with their notes, reports, reviews,
work orders, tasks and standing leases, the questions under it with their replies, and the subscriptions under it.

| Method and path | Does |
|---|---|
| `GET /scopes/{scope}` | `scope`, `counts` (`items`, `notes`, `reports`, `reviews`, `workOrders`, `tasks`, `leases`, `questions`, `replies`, `subscriptions`) and `state` null. Nothing held is answered 404 `not-found` |
| `DELETE /scopes/{scope}` | Lets all of it go, each member told as its `deleted` event, and answers the counts with `state` `deleted`; with `dryRun=true` it lets nothing go and answers them with `state` `preview` |

## The bmo-receiver port

bmo-receiver reads the work a room's jobs put to the owner, answers a question the owner settles in his chat,
and asks what a room holds before it removes the room. These routes answer it in the shapes it reads. A
room's number is a scope written as text: an item or a question filed for a room carries the room's number as
its `scope`. Every route takes the bearer token as the others do.

| Method and path | Does |
|---|---|
| `GET /rooms/{chat}/holdings` | What the service holds of the room: `{code, count}` for `room_open_items`, its items not `done`, `dropped` or `merged`, and for `room_waiting_questions`, its questions `open` or `paused`; each only where its count is not zero, in that order |
| `GET /questions/open?chat=` | The room's questions put to a person and still waiting, the one asked last first: `id`, `options` (its `choices`, in order), `recommendation` (the recommendation's answer, or null), `why` (its reason, or null) and `talks` |
| `GET /items?open=1` | The open items in the port's shape: `{"items": [...]}`, each `id`, `approved` (whether it has an approval of the kind `approval`), `status` (its state), `title`, and `worker` (its `assigneeName`, or null). The items are those not `done`, `dropped`, `merged` or `held`, oldest first |
| `POST /question` | Settles a question with one of its choices by its number counted from zero: `id` and `chosen`. Answers 204 |
| `POST /question-talk` | Writes a line of talk on an open question and stops its clock: `id` and `say`. Answers 204 |
| `POST /queue/moved` | Puts a `queue.moved` event into the stream, its data `detail?` where one is given. Answers 204 |

- **The open questions** are the room's questions put to a person in `open`, whether or not their due time has
  passed, and its `paused` ones, the one asked last first: every question still waiting for his answer. A
  question put to the supervisor is not his to answer, and the port neither lists it nor takes a settle or a
  talk on it: either answers `question_absent` (404). The holdings count it, since the room holds it all the same. The port reads `talks` as true: any question may be talked over, which is how far the service's own
  questions go.
- **A talk is a reply that does not settle,** from the actor `owner`, its `source` a person, and the asker hears
  it as a `reply.created` event. Stopping the clock is the question moving to `paused` with what it had left.
- **A settle is a settling reply** carrying the chosen option as its `choice` and its text, from the actor
  `owner`, its `source` a person, which moves the question to `answered` as any settling reply does. A question
  already `answered`, `withdrawn` or `unsent` answers 204 and takes no reply.
- **A refused talk** answers `question_talk_settled` (409) on a question already settled,
  `question_absent` (404) for a number naming no question, and `question_talk_say` (400) for a talk with no
  words. **A refused settle** answers `question_absent` (404) for a number naming no question and
  `question_answer_wrong` (400) for a number that names no choice of it. A request of the wrong shape is
  `bad-request`, an `open` read as `false` or `0` among them.
- **The queue moved** is the port's own kind, `queue.moved`, with `detail?` as its data. It is read on the raw
  event stream; a subscription names only the service's member kinds and takes none of it.
- **These routes are not in the OpenAPI document or the clients.** They answer a caller that reads a room's
  work in its own words; this document states them and nothing generates them.

## Health

`GET /health` answers `ready`, `reason` (null when ready) and `version`, the repository's version.

## The client

The crate `bmo-job-client` in `crates/client` is the contract in Rust: every body above as a type, every route
as a call of `JobClient`, the event stream read as events, the webhook's signature checked, and the OpenAPI
document generated from those types. The package `bmo-job-client` in `package/` is the same contract in
TypeScript: the types `openapi-typescript` generates from `docs/openapi.json`, at the entry point and at the
package's `./schema` subpath, and `jobClient(baseUrl, token)`, whose calls are the document's routes, each
answer typed by the route and each refusal read in the error shape. A caller uses one of them and writes no
client of its own.

- **Installing it.** `bun pm pack` in `package/` writes `bmo-job-client-<version>.tgz`, and a downstream
  checkout names that file in its dependencies as a `file:` address. The pack carries the manifest, the entry
  point and the generated types; `openapi-fetch` comes with it as the package's one dependency.
- **A failure says whether the service answered.** A call of `JobClient` that fails tells a service that gave
  no answer, nothing at its address, a connection that broke before the answer was whole, or a 5xx, from one
  that answered with a refusal or an answer of another shape. A caller makes the first kind again, since a
  restart is no answer.
- **Version and types.** The package carries the repository's version, and its own test fails where the
  committed types differ from what `docs/openapi.json` generates.

## The harness plugin

The plugin is `bmo-job`: the MCP server the service's own binary starts and the skill beside it in
`crates/plugin/skills/bmo-job`. The server is started with the service's address and, from the environment,
the token it calls with (docs/cli.md). Every call it makes is the caller's that token names, so an approval
given through `job_item_update` is recorded under that caller and is of the kind `approval` only where the
settings mark that caller as an approver (Items, "Approvals"); the tool takes no name of who approves.

### The tools

A launch line may leave tools out (docs/cli.md, `--without`): the harness that starts the server is then
offered the rest alone, as where another tool of its own does that work. A tool left out is not called either:
the server answers a call of one as it answers a name it does not serve.

Each tool is one route called through the client, its input the route's path, query and body together and its
output the route's answer. A route's refusal comes back as a tool error carrying its status, `code` and
sentence. The OpenAPI document states each under `x-tools`.

| Tool | Route |
|---|---|
| `job_items` | `GET /items` |
| `job_item_create` | `POST /items` |
| `job_item` | `GET /items/{itemId}` |
| `job_item_update` | `PATCH /items/{itemId}` |
| `job_notes` | `GET /items/{itemId}/notes` |
| `job_note_add` | `POST /items/{itemId}/notes` |
| `job_report` | `POST /items/{itemId}/reports` |
| `job_reviews` | `GET /items/{itemId}/reviews` |
| `job_work_order` | `GET /items/{itemId}/work-order` |
| `job_tasks` | `GET /items/{itemId}/work-order/tasks` |
| `job_queues` | `GET /queues` |
| `job_lease_next` | `POST /leases`; nothing waiting is the output `{"lease": null}` |
| `job_lease_renew` | `PATCH /leases/{leaseId}` |
| `job_lease_release` | `DELETE /leases/{leaseId}` |
| `job_question_ask` | `POST /questions` |
| `job_question` | `GET /questions/{questionId}` |
| `job_replies` | `GET /questions/{questionId}/replies` |
| `job_reply` | `POST /questions/{questionId}/replies` |
| `job_question_change` | `PATCH /questions/{questionId}` |
| `job_subscribe` | `POST /subscriptions` |
| `job_unsubscribe` | `DELETE /subscriptions/{subscriptionId}` |

### What the server keeps for its harness

- **Leases and subscriptions it took.** A lease `job_lease_next` took and a subscription `job_subscribe` made
  are renewed by the server at half their span while the harness runs, until they are given back, ended or
  answered 410. When the harness ends the session, the server gives each back and deletes each.
- **Queues named at the start.** Started with queues, the server subscribes to `item.created` and
  `item.changed` on them under the holder name it was given, as if the harness had called `job_subscribe`.

### Notifications

Every subscription the server made without a `url` is read in the stream form, and each event is sent to the
harness:

- **as `notifications/resources/updated`** on `job://items/{itemId}` where the event belongs to an item, and on
  `job://queues/{queueName}` where it belongs to a queue. The server serves those two resource templates, so a
  harness reads the item or the queue the notification names;
- **and as a pushed message,** where the server was started with a channel: it declares that experimental
  capability and sends each event as the notification `notifications/` followed by the channel's name, with
  `content` the event as JSON text and `meta` its `kind`, `eventId`, and `itemId` and `queueName` where it has
  them, each a string. A harness that takes messages pushed into its session reads them there.

## What the service calls

Nothing but the `url` of each subscription that names one, as stated under Subscriptions. `bmo-job job ask` is a
verb in the asker's own process, and it is that process that reaches the receiver (docs/cli.md).
