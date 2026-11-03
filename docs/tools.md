# The tools

## What they are

- **The service provides three tools to the tool host,** each a folder under `tools/` with its `tool.toml`,
  the executable it runs and the schemas its entries name: `todo`, the queue's readings, its filing, the words
  added to a row and its approval; `state`, the chat agent's readings of what runs, a room's job runs, one
  item's work order and a room's archived messages; and `launch`, a job filed on a worker and a research sent
  out.
- **Each is declared in the tool host's manifest format,** protocol 1, and placed and registered by this
  repository's deployment (docs/deploy.md). The host reads every manifest alike and knows no entry by name.
- **The owner of an entry is the capability a session must hold:** the worker's two queue readings ask for
  `queue.read`, and every entry of the chat agent's `bmo` namespace asks for `room.tools`. No entry declares a
  grant: the job service's own caller tokens decide what a filing and an approval are, and the agency's route
  decides whether the turn may file an item.

## The todo tool

- **`worker/todo_list`** and **`bmo/todo_list`** read the queue in its own order. By default what is open: not
  finished, not set aside, not merged away. `all` reads every state, `pending` the drafts that await a
  person's approval, `held` what is set aside, `merged` what was merged away, and `chat` one room, by the
  item's scope.
- **`worker/todo_show`** and **`bmo/todo_show`** read one item whole: its number, or the title of one that is
  not finished. The answer carries the item, its notes and its reports.
- **`bmo/todo_add`** files a row through `POST /items`: its title, its brief from what is to be done and what
  was concluded, the room it came from as its scope, its requester and, where named, its model. A row filed
  this way is a draft until an approver approves it.
- **`bmo/todo_append`** adds words to an item as a note, so a running job reads them before it reports.
- **`bmo/todo_approve`** gives the chat caller's approval through `PATCH /items/{itemId}`. The job service
  records an approval and has no route that takes one away, so a call asking to unapprove is refused with that
  sentence.

## The state tool

- **`running`** is the agency's jobs that are starting or running on this machine, newest first; a room's own
  runs are the `jobs` reading.
- **`jobs`** is the agency's jobs for the room the call names or the turn's own, newest first, each with its
  state, its item, its worker, its role and its room, and its report where `full` asks for it.
- **`work_order`** is the job service's work order of one item: its state, its purpose, its directive and its
  tasks in stage order.
- **`archive_scan`** is the receiver's messages of one room, newest first: one message by its id, or a page
  narrowed by the text it holds, the message it carries on before and its sender.

## The launch tool

- **`task`** files a job for the turn through the agency's `POST /item-filings`, which reads the turn's room
  and whether the owner asked for it, so an item the owner asked for opens at once and any other waits as a
  draft. Its title is the first line of the prompt and its brief the prompt whole. A call naming an item that
  already stands is refused: the item is leased from its queue and no second one is filed for it.
- **`search`** asks the `research/run` entry the session holds, on the call's own socket back into the host,
  with the depth the call names (`search:basic`, `search:thorough` or `search:exhaustive`) and the room the
  result is told to. A page named by `import` is asked for whole. The research's result comes back to the room
  when it ends; the call answers at once with the run's id and state.

## What is not here

- **The word-chain game and the waits are other units' tools.** The word-chain service places
  `bmo/word_chain_start` and `bmo/word_chain_stop`, and the receiver places `bmo/wait`, so this repository
  declares none of them.
- **A web search at a depth is the research unit's,** reached through the entry above; a page fetched directly
  is the explorer's `web/web_fetch`. This tool launches the research and does not fetch a page itself.
