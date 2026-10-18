# Research runs

A search or a browse is a research run: started once, ended once. What a search or a browse looks for,
how it reads the web and what document it leaves are the research tools' own, which another unit
carries; this document settles what this service's research verbs hold of a run: its keys and depths,
its profile, its run in bmo-agency's ledger, its branches' places in the co-worker pool, its clock, and
how it ends. The verbs are docs/verbs.md, "Research runs".

## The keys and the depths

- **Four research keys,** each a work key: `search:basic`, `search:thorough`, `search:exhaustive` and
  `browse` (docs/policy.md, `research.toml`).
- **A ladder of three search depths.** `basic` runs with no branches at all; `thorough` and `exhaustive`
  split the question into the angles that decide it and send branches over them. The depths differ in how
  widely the answer is looked for and how hard it is checked, never in how long they may take.
- **Which depth a search runs at is the caller's,** never this service's: the chat session or the console
  hands it the depth and whether the person insisted on it (「A named depth, and a full pool」).

## A run's profile

- **A profile per research key** sets what one run of it uses, as the launcher took it and handed it to the
  runner whole: its branch role and how many branches one run may open (none for `basic`), the longest an
  insisted depth waits for a pool place, how pages are fetched, the pause between two requests to one host
  and to one search engine, and for `browse` alone its total time and the wrap-up time kept inside it.
- **The checks are the policy's research part:** a clocked key carries both times and the wrap-up time
  leaves at least the research the policy asks; a key with no clock carries neither; `basic` opens no
  branch; a strategy fits its key; and every field stands inside the policy's limits. A profile read back
  for another key, or one that breaks a check, is refused.
- **A run keeps the profile it started with;** a change applies to the next run.
- **The tier** is the one the launcher took for the key and handed to the run beside its profile, or,
  where it hands none, the work key's own profile's (docs/policy.md, `work-keys.toml`); the model and
  effort are that tier's on the provider the search line starts on, resolved through bmo-agency's routes.
  A profile holds neither, so one work's model is decided in one place.
- **What a run's settings open:** the policy names the run variable that opens each of a run's research
  capabilities — the picture viewer, the branches, the rulebooks — and a run holds a capability where its
  variable names something other than `0`. The run's web server is told the run's settings in the turn's
  variables.

## The run

- **A run is opened in bmo-agency's run ledger** as it starts, with its agent, room, unit, depth and clock,
  and closed there once, with how it ended and the context it ended carrying; the first ending reached is
  the one that closes it.
- **Its model is carried as a work turn on bmo-agency** on the search line's work key, with no job behind
  it, on a session thrown away (docs/api.md, "Sessions"). The prompt names the moment it was asked in UTC,
  and the knowledge its words call for is handed as a reference block naming the deployment's agent.
- **A question its program stops on** is left standing for a person in its pane: nobody answers one for a
  research run.
- **A stop from outside** interrupts the turn's session and, once the turn has ended, keeps what bmo-agency
  reads the turn had written so far, so the last result the run wrote is what is handed back; nothing
  read is kept as nothing written (docs/api.md, "Work turns").
- **How it ends:** done, unanswered, stopped by hand, cut at a browse's ceiling, failed, or refused for
  leaving its own work running — a result produced with a background task still live describes work that
  can never finish.

## A search has no clock

- **No ceiling, no research deadline, no depth raised by time.** One question, one run, which goes on until
  it has written its answer. A clock would close in the middle of writing the result and leave the room
  nothing; a slow search is better than a cut one.
- **A search stops in three places only:** its done condition met; a branch asked for past its allowance,
  refused for good; or a person stopping it, which keeps what it wrote.

## The browse clock

- **A browse's clock starts when its program starts:** its research closes at its total time less its
  wrap-up time, and its ceiling is its total time. After the close the web tools refuse new searches and
  pages and say to write the result from what is at hand.
- **At the ceiling the agent is asked** whether to keep the run or cut it, and the run goes on until it
  answers (docs/jobs.md, "A clock asks before it cuts"). A cut ends the run with what it last wrote.

## A named depth, and a full pool

- **The caller says whether the depth is required:** `explicit` where the person named it, `optional`
  otherwise, absent reading as optional.
- **A deeper run asks the pool first** whether its branch role's tier has a place on the branch provider:
  the limit less what is active, reserved and asked for.
- **An optional run with no place** goes at the shallowest depth instead, and says why it was lowered.
- **An explicit run is never lowered:** it reserves its branches' seats and waits for its first seat up to its
  profile's wait, and past that gives the reservation back and fails, saying the pool was full.

## Branches take places in the pool

- **A run's branches' places are seats reserved in bmo-agency's co-worker pool,** held from before the run's
  clock starts until it ends, under the owner `<source>-run:<run>`. Every seat and request of the run's own is
  given back however it ends, and a sweep reclaims what a run left behind whose ledger entry is closed and
  whose unit no longer runs. Time alone reclaims nothing.
- **One branch,** handed over by the run's branch tool, takes a place for the profile's role — its model and
  effort that role's tier's, regardless of the parent — waits for one, and runs its angle on the place's
  model. A branch is refused past its run's allowance, after a browse's research close, and from inside a
  branch.
- **A branch's lifetime is its parent's:** it is stopped at its run's research close or when the run ends, and
  hands back what it had written. A search's branch, like its parent, has no clock.
- **A branch's place is given back once** however it ends, and its notes come back framed as working notes the
  parent has not checked: the parent verifies what it carries into its result.
- **A branch's place is the prefix of the branch line:** one per provider, model and effort, whichever run
  opened it, so branches on the same model share a warm prefix. Keeping that prefix warm while a reservation
  stands is bmo-agency's pool's.

## The verbs a research tool calls

A research tool hands this service a run's admission, the run and its branches on the command line
(docs/cli.md, "`research`"). Each stands on the stand (docs/verbs.md, "Where a verb stands"), and a stop
from outside — an interrupt, a termination or a hang-up — stops the model it carries, which keeps what it
had written.

- **The admission comes before the prompt:** a lowered depth asks for another prompt, so the tool asks
  which depth a run goes at — the one asked, or the shallowest with the sentence saying why (「A named
  depth, and a full pool」) — before it writes one.
- **The run is carried whole:** what ended runs left in the pool is reclaimed; the run's entry is opened
  in the ledger where its launcher opened none; its branches' seats are reserved, an explicit run waiting
  for its first; its model is carried on the prompt it is handed, closed with a line naming the moment it
  was asked and the knowledge its words call for; and the entry is closed once. It answers with the run,
  how it ended, the context it ended carrying and the text it wrote.
- **What fails a run:** an answer with its own work still running, closed in the ledger as `live-tasks`;
  and a turn that gives no answer, a route that cannot be resolved or an explicit run that got no seat,
  closed as `failed`. Each is said on standard error.
- **An answer closes the entry as `done`,** unless the run is asked to leave it open, so that a tool whose
  own reading of the answer is that it answered nothing closes it as `unanswered` itself. A stop closes it
  as stopped either way, and every place the run held is given back however it ends.
- **A branch is carried for the run's branch tool,** its angle on standard input, and answers with the
  branch's notes or the one sentence saying why it was refused or failed, which the parent reads as the
  branch's answer.
- **A browse's ceiling is not this verb's to cut:** the run is asked about at its ceiling (「The browse
  clock」), and a cut stops the run from outside.
