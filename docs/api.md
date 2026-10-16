# The API

What the service answers over HTTP, and what it calls on other units. Every other unit reaches the
service through this API and nothing else.

## A refused request

- **The status is a plain HTTP status code:** 400 for a request that is wrong as asked, 403 for one
  the asker may not make, 404 for something that does not exist, 409 for a request that conflicts
  with the present state, and 500 for a failure on the service's side, such as its database.
- **The body is a JSON object of exactly two keys:**
  - `error`: one English sentence for a person, naming why;
  - `code`: a stable snake_case code for a machine. A caller decides on the code and never on the
    sentence.
- A database failure is refused with status 500 and code `database`.

## Codes

Every refusal the service answers, every answer it gives and every fact it hands a room is a code with
structured fields. None carries a sentence in a person's language: the words a person reads are made
from the code and its fields where they are shown, by the web console's language packs or by the room's
receiver. The crate `job-codes` holds them, one Rust module to an area and in it one enum to a family.

- **Four families:**
  - **Refusals** are answered as "A refused request" above: the status, the code and one English
    sentence. `JobError` gathers every area's refusals, and each answers its own status.
  - **Answers** are what a request that succeeded answers with: a JSON object holding the code under
    the key `code` beside its fields.
  - **Facts** are what the service hands bmo-receiver to tell a room: a JSON object holding the code
    under the key `code` beside its fields. `Fact` gathers every area's facts.
  - **Kinds** are the values a field takes where it names a state or a sort of thing: a snake_case
    word, or, in a list where any kind carries fields of its own, an object holding the word under the
    key `kind` beside the fields.
- **A code is unique across refusals, answers and facts,** so a caller reading `code` needs nothing
  else to know what it is. A kind is unique within its own list.
- **What has no code:**
  - Layout: the joints of a list, a heading over a listing, a length of time or a count spelt out. The
    field carries the number or the list, and whoever shows it lays it out.
  - Grammar around a name. A name is carried as it stands; a language that attaches a particle to it
    does so where it is shown.
  - A command line's own usage, its reading of its flags and its failure to reach the service, which
    are the command line's.
  - A matter of another unit: a room's registration, a chat session's health and its fold, Claude
    accounts, provider keys, gates and Laya, documents and their translation, code review. The unit
    that owns it answers it.
- **Fields:** an item, a job, a question, a note, a memory and a task are named by their numbers or
  ids; a count is a whole number; a field written with a question mark may be null; a field written
  with brackets is a list.

### Refusals

#### Providers: `ProviderRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `provider_off` | 409 | | the run's provider is switched off |
| `single_model_provider` | 409 | | a co-worker is asked of a provider that serves one model alone |
| `job_model_wrong` | 400 | `selection` | the job model named is no tier, no provider and no model |
| `no_job_model` | 409 | | no provider that is on offers a job model |

#### Work order edits: `WorkOrderRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `plan_not_object` | 400 | | the plan is not a JSON object |
| `plan_part_empty` | 400 | `part` (a plan part) | a part of the plan is empty or too long |
| `plan_no_stages` | 400 | | the plan has no list of stages |
| `plan_stage_empty` | 400 | | a stage has no task |
| `plan_role_wrong` | 400 | `[roles]` | a task's role is none of the roles |
| `plan_model_form` | 400 | | a model is not named as provider and model |
| `plan_too_many_builds` | 400 | `most` | the synthesis builds more than the most it may |
| `task_absent` | 404 | `task` | the order has no such task |
| `control_turn_untouched` | 409 | | a step verb names the plan, synthesis or final turn |
| `order_finished` | 409 | | the order is finished and is not reopened |
| `order_draft` | 409 | | the order is a draft that has not run |
| `order_closing` | 409 | | the order is winding down and not yet finished |
| `instruction_size` | 400 | | a task's instruction is not 1 to 30000 characters |
| `model_tier_wrong` | 400 | `[tiers]` | a model tier is none of the tiers |
| `coworker_tier_wrong` | 400 | `[tiers]`, `[worker_only]` | a co-worker tier is none of the tiers a co-worker may be |
| `history_edit` | 409 | | a task of an earlier round is edited |
| `history_rerun` | 409 | | a task of an earlier round is run again |
| `history_restore` | 409 | | a task of an earlier round is restored |
| `running_task_edit` | 409 | `item` | a running task's stored instruction is changed |
| `done_task_edit` | 409 | `task` | a finished task is edited rather than run again |
| `nothing_to_edit` | 400 | | an edit names no instruction and no model |
| `skip_reason_size` | 400 | | a skip reason is not 1 to 2000 characters |
| `task_already_skipped` | 409 | `task` | the task is already skipped |
| `builder_running` | 409 | `item` | a running builder turn is to be skipped |
| `stage_not_yet` | 409 | `task` | the task's stage has not come yet |
| `stage_running` | 409 | | the present stage has a task running |
| `build_running` | 409 | | a build task is running |
| `research_over` | 409 | | a research task is run again after the research phase |
| `phase_wrong` | 409 | | the order's present phase does not run the task again |
| `task_running` | 409 | `task` | the task is running |
| `task_pending` | 409 | `task` | the task is already pending |
| `task_not_skipped` | 409 | `task` | the task is not skipped |
| `same_anchor` | 400 | | a task is moved relative to itself |
| `add_anchor` | 400 | | a new task is placed anywhere but beside a pending build task |
| `build_move` | 409 | | a build task is moved while it or its anchor is not pending |
| `task_move` | 400 | | a single task is moved instead of its whole stage |
| `stage_move_past` | 409 | | a stage at or before the present one is moved |
| `stage_move_pending` | 409 | | a stage with a task not pending is moved |
| `move_kinds` | 400 | | a task is moved among tasks of another sort |
| `stop_reason_size` | 400 | | a stop reason is not 1 to 2000 characters |
| `close_reason_size` | 400 | | a close reason is not 1 to 2000 characters |
| `close_builder_running` | 409 | `item` | the order is closed while a builder turn runs |
| `recover_not_done` | 409 | | an order not finished is recovered |
| `recover_no_jobs` | 404 | | the range holds no job of the item |
| `recover_job_known` | 409 | `job` | the order already records the job |
| `recover_job_running` | 409 | `job` | the job has not ended |
| `recover_not_step` | 400 | `job`, `unit` | the job is no step of the item's work order |
| `recover_prompt_unread` | 409 | `job`, `base` | the job's prompt does not give the role and instruction |
| `recover_task_twice` | 409 | `task` | the task is already in the order |
| `recover_no_success` | 404 | | the range holds no step that succeeded |
| `recover_no_builder` | 404 | | the range holds no builder turn to read the purpose and instruction from |
| `order_paused` | 409 | `item` | the order is stopped and waits for its turn in its worker's line |
| `field_not_text` | 400 | `name` | a body field is not a string |
| `field_missing` | 400 | `name` | a body field is missing |
| `runs_still_alive` | 409 | `[alive]` | runs asked to stop are not yet confirmed ended |
| `document_exists` | 409 | `path` | a result document already stands at the path |
| `reopen_not_done` | 409 | | an order not finished is reopened |
| `draft_only` | 409 | | an order past its draft is changed as a draft |
| `order_and_model` | 400 | | the order or its model does not read |
| `item_not_approved` | 409 | | the item is not approved and ready to run |
| `order_active` | 409 | `item` | the order's stored steps are already running |
| `order_done` | 409 | `item` | a finished order is run again |
| `chat_wrong` | 400 | | the chat id is not a room's |
| `checkout_not_git` | 409 | `cwd` | the checkout is not a git repository |
| `item_number_wrong` | 400 | | the item number is not a whole number |
| `item_absent` | 404 | | there is no such item |
| `resume_not_held` | 409 | | work resumed is not approved, or is held or merged |
| `resume_not_stopped` | 409 | | work resumed is not stopped |
| `before_and_after` | 400 | | a move names both before and after |
| `checkout_of_done` | 409 | | a finished order's checkout is changed |
| `job_range_form` | 400 | | a job range is not a first and a last job |
| `stop_done_item` | 409 | | a finished item is stopped |

#### Stopping runs: `RunStopRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `stop_target_wrong` | 400 | `target` | the target is not a kind and an id |
| `reason_language` | 400 | | a stop or hold reason is not in the deployment's language, which the console shows it in as written |

#### TODO verbs: `TodoRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `condition_both` | 400 | | a hold names both an item and a deployment to resume after |
| `after_not_open` | 400 | `id`, `why` (an after refusal) | the item a hold resumes after is not another unfinished item |
| `no_waiting_commits` | 409 | `id` | the item has no commit waiting for a deployment |
| `not_a_commit` | 400 | `selector` | the commit named is not this repository's |
| `commit_not_on_main` | 409 | `selector` | the commit is not on main, so no deployment makes it live |
| `commit_already_live` | 409 | `selector` | the commit is already live |
| `edit_nothing` | 400 | | an edit names no part to change |
| `no_deploy_hold` | 409 | | no deployment hold stands to release |
| `hold_takes_no_after` | 400 | | a plain hold names an item to resume after |
| `stop_needs_why` | 400 | | a stop gives no reason |
| `wind_down_needs_why` | 400 | | a wind-down gives no reason |
| `stop_here_needs_report` | 400 | | a stop-here gives no report |
| `stop_here_closed` | 409 | `id`, `title` | the item is finished or merged |
| `job_cannot_unhold` | 403 | | a job releases a hold |
| `merge_needs_items` | 400 | | a merge names no items or no item to merge into |
| `resolve_needs_why` | 400 | | a resolution gives no reason |
| `mark_needs_status` | 400 | | a mark gives no status line |
| `worker_needs_name` | 400 | | a worker assignment names no worker and does not clear one |
| `worker_is_support` | 400 | `worker`, `base` | the worker is a support worker of a base |
| `repairs_on_filing` | 400 | | a repair link is given with a filing |
| `repairs_needs_target` | 400 | | a repair link names no item and does not clear one |
| `cowork_needs_switch` | 400 | | co-work is set to neither on nor off |
| `impact_unread` | 400 | `why` | the impact grounds are not the one JSON object they are written as |
| `priority_needs_number` | 400 | | a priority is not a number |
| `model_needed` | 400 | | a model setting names neither auto nor a model |
| `judgement_needs_title_and_room` | 400 | | a row raised on BMO's judgement has no title or no room |
| `filing_parts_missing` | 400 | `[missing]` (filing parts) | a filing lacks a part an item is made of |
| `banned_terms` | 400 | `[terms]` | a title or summary uses terms the owner has set aside |
| `append_empty` | 400 | | words added to an item are empty |
| `brief_agent_only` | 403 | | anyone but chat BMO rewrites a brief |
| `brief_needs_change` | 400 | | a brief comes without what changed |
| `brief_empty` | 400 | | a brief is empty |
| `brief_not_english` | 400 | | a brief or its change is not in English |

#### A job's question: `JobAskRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `question_room` | 400 | | a question names a room; it goes to the owner's private room alone |
| `question_item` | 400 | | a question names no item |
| `question_text` | 400 | | a question does not say what is asked |
| `question_options` | 400 | | a question has fewer than two options |
| `question_option_empty` | 400 | | an option is not one line with something on it |
| `question_recommend` | 400 | `count` | the recommendation is not one of the options by number |
| `question_why` | 400 | | a question does not say why it is asked rather than decided |
| `question_wait` | 400 | | the wait is not a number of milliseconds |
| `question_talk_number` | 400 | | a talk names no question by number |
| `question_takes_no_talk` | 409 | `id` | the question takes no talk |
| `question_talk_say` | 400 | | a talk has no words of its own |
| `question_banned_terms` | 400 | `[terms]` | the words use terms the owner has set aside |

#### A job's wait: `JobWaitRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `wait_needs_what` | 400 | | a wait mark does not say what the job waits on |
| `wait_needs_target` | 400 | | a wait mark names no unit or process and runs no command |

#### Talk: `TalkRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `talk_busy` | 409 | | a second word comes while the first is still being answered |

#### Control requests: `ControlRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `body_not_json` | 400 | | the request's body is not JSON |
| `action_wrong` | 400 | | the action asked is none the route takes |

#### Branch records: `BranchSplitRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `no_workflow_records` | 404 | `project` | no workflow record is left under the project folder |
| `workflow_not_found` | 404 | `workflow`, `project` | the workflow has no record under the project folder |

#### Workers: `AgentRefusal`

| Code | Status | Fields | Refused when |
|---|---|---|---|
| `no_worker` | 404 | `worker` | there is no such worker |
| `not_restable` | 409 | `worker` | a worker the owner did not add is set to rest or woken |
| `back_here_needs_path` | 400 | | a worker brought back to this machine has no path on it |
| `nothing_to_change` | 400 | | a change names nothing to change |
| `not_a_base` | 400 | `base` | the worker is not a base |
| `base_on_node` | 409 | `base`, `node` | a base standing on a node is given a node checkout |
| `checkout_already_on_node` | 409 | `base`, `node` | the base already has its checkout on the node |
| `checkout_name_long` | 400 | `base`, `name` | the node checkout's name breaks the worker rule |
| `agent_name_taken` | 409 | `name` | an agent of the name already stands |
| `node_path_form` | 400 | | a node's path starts with neither `/` nor `~/` |
| `checkout_not_on_node` | 404 | `base`, `node` | the base has no checkout on the node |
| `checkout_busy` | 409 | `worker` | a job runs on the checkout's worker |
| `checkout_not_removed` | 500 | `worker` | the checkout's worker could not be taken away |
| `no_node` | 404 | `node` | there is no such node |
| `node_takes_no_jobs` | 409 | `node`, `why` | the node takes no job |
| `builtin_stays_here` | 409 | | the built-in worker is placed on a node |
| `support_follows_base` | 409 | `worker`, `base` | a support worker is placed apart from its base |
| `has_support` | 409 | `worker` | a worker with support workers or node checkouts is placed on a node |

### Answers

#### Stopping runs: `RunStopAnswer`

| Code | Fields | Answered when |
|---|---|---|
| `nothing_running` | | nothing is running, so nothing is listed or stopped |
| `nothing_nameable` | `count` | runs are running and none can yet be named as a target |
| `stop_confirmed` | `target`, `runs` | a stop reached its runs and each is confirmed ended |
| `stop_asked` | `target`, `runs` | a stop asked its runs to end and their end is not yet confirmed |
| `all_asked` | `asked`, `confirmed`, `[targets]` (each a target and its stop state) | everything running was asked to stop |

#### TODO verbs: `TodoAnswer`

| Code | Fields | Answered when |
|---|---|---|
| `note_removed` | `id`, `title`, `note` | words added to an item were taken back before any job carried them |
| `note_removed_after_delivery` | `id`, `title`, `note` | words were taken back after a running job was handed them, so that job keeps them |
| `item_edited` | `id`, `title`, `[changed]` (edited parts) | an item was rewritten |
| `queue_paused` | | the queue's automatic progress was paused |
| `queue_resumed` | | the queue's automatic progress was resumed |
| `deploy_hold_stands` | `hold` | a resume leaves a deployment's hold standing |
| `deploy_hold_released` | `hold` | a deployment's hold was released |
| `item_held` | `id`, `title`, `reason?` | an item was set aside |
| `item_stopped` | `id`, `title`, `runs`, `reason` | an item's runs were asked to stop and it is held as stopped |
| `item_wound_down` | `id`, `title`, `reason`, `condition?` (a resume condition), `job?` | an item will stop at the end of its stage, the running job told so |
| `stopped_here` | `id`, `title`, `note`, `held` (a held word), `reason?` | where an item's job stopped was left on it, the item kept open and held |
| `item_unheld` | `id`, `title`, `priority` | an item's hold was released |
| `items_merged` | `[ids]`, `into`, `title` | items were merged into another |
| `item_unmerged` | `id`, `title`, `priority` | an item was taken out of a merge |
| `item_closed` | `id`, `[written]` (closing parts), `[carried]` | an item was finished with the items merged into it |
| `strand_handed` | `memory`, `head` | a strand left to BMO was handed to it as a memory |
| `resume_condition_met` | `id`, `title`, `after` | the item an item waited on finished and BMO was told |
| `item_reopened` | `id`, `unmerged_from?`, `[merged_again]`, `[sources_reopened]` | an item was opened again |
| `item_approved` | `id`, `noted` | an item was approved, with the owner's word or without |
| `worker_cleared` | `id`, `title` | an item's worker was taken off, so the built-in worker takes it |
| `worker_set` | `id`, `title`, `worker` | an item was given a worker |
| `repairs_cleared` | `id`, `title` | an item's repair link was taken off |
| `repairs_set` | `id`, `title`, `target`, `root`, `round`, `rounds` | an item was linked as a repair of another |
| `cowork_set` | `id`, `title`, `on` | an item's co-work was switched |
| `impact_set` | `id`, `title`, `contracts`, `new_home`, `verdict` (a verdict), `cowork` | an item's impact grounds were written: how many nodes change their outward contract, whether a new home is to be chosen, the verdict and the co-work setting |
| `impact_check_kept` | `id` | a job's own impact grounds agree with the co-work setting and were kept quietly |
| `impact_check_told` | `id`, `difference` (an impact difference) | a job's own impact grounds were kept and told to BMO |
| `model_set` | `id`, `model` | an item's job model was set |
| `item_filed` | `id`, `title`, `cowork`, `clip` | a row was filed and waits for approval |
| `item_raised` | `id`, `title`, `item`, `[sources]` (each an item id and a strand), `cowork`, `[reopened]` | a row was raised on BMO's judgement out of an item's work |
| `words_appended_told` | `id`, `title` | words were added to an item and BMO was told |
| `words_appended_kept` | `id`, `title` | chat BMO's own words were added to an item as a record |
| `brief_rewritten` | `id`, `title`, `job?` | an item's brief was rewritten, the running job handed the change |

#### Rooms an item belongs to: `RoomHoldings`

| Code | Fields | Answered when |
|---|---|---|
| `room_open_items` | `count` | open items came from the room |
| `room_waiting_questions` | `count` | questions of jobs from the room wait for an answer |

#### Work orders: `WorkOrderAnswer`

| Code | Fields | Answered when |
|---|---|---|
| `resumed_held` | `item`, `hold` (a queue hold) | an order was resumed while the queue is held still, so its next step waits |

#### A job's question: `JobAskAnswer`

| Code | Fields | Answered when |
|---|---|---|
| `owner_talked` | `id`, `[heard]` | the owner talked on the question instead of choosing, and the job is handed what was said |

### Facts

#### Jobs: `JobFact`

| Code | Fields | Told when |
|---|---|---|
| `job_ended` | `reason` (a job ending), `job_id`, `checkout` | a job ended |

#### A job's question: `QuestionFact`

| Code | Fields | Told when |
|---|---|---|
| `question_asked` | `item`, `question_id`, `question`, `[options]`, `recommended`, `why`, `wait_seconds`, `talk`, `removed_room?` | a job put a question to the owner |
| `question_talk_held` | `item` | the owner talked, so the clock stopped while the job answers |
| `job_replied` | `item`, `said` | the job answered the owner's talk |
| `question_revised` | `item`, `[options]`, `recommended`, `wait_seconds` | the talk changed the options |
| `question_chosen` | `item`, `option`, `named` | the owner chose an option |
| `question_typed` | `item`, `answer` | the owner answered in words |
| `question_lapsed` | `item`, `lapsed` (a lapse), `option`, `named`, `wait_seconds` | the wait ran out and the recommendation was taken |
| `pane_question` | `item`, `mark`, `window` (a pane window), `[options]` (pane options) | a job's pane stopped at a confirmation the runner does not answer alone |
| `pane_question_withdrawn` | `item` | the confirmation closed before it was answered |
| `pane_command_allowed` | `item` | the runner allowed a command that stays inside the job's own workspace |

#### A run that failed: `FailureFact`

| Code | Fields | Told when |
|---|---|---|
| `run_failed` | `room?`, `cause` (a run death), `said?`, `last_tool?`, `tool_detail?`, `run_log` | a job's run died or held together and gave no answer |

#### Work order stops: `WorkOrderStop`

| Code | Fields | Told when |
|---|---|---|
| `final_not_closed` | `item` | the final turn ended done with its item neither finished nor held |

#### A work order's checkout: `CheckoutStop`

| Code | Fields | Told when |
|---|---|---|
| `checkout_not_moved` | `item`, `place` (a checkout place), `why` (a git failure) | the checkout could not be taken onto the order's branch |
| `parked_not_merged` | `item`, `why` (a git failure) | the change parked at the stop could not be taken back |

#### TODO words: `TodoFact`

| Code | Fields | Told when |
|---|---|---|
| `words_appended` | `id`, `title`, `by`, `job?` | someone added words to an item, with the job at work on it if one runs |

### Kinds

#### A plan part: `PlanPart`

| Kind | Fields | Names |
|---|---|---|
| `stage_title` | | a stage's title |
| `model` | | a model |
| `task_instruction` | | a task's own instruction |
| `task_title` | | a task's title |
| `build_instruction` | | a build task's instruction |

#### A stop state: `StopState`

| Kind | Fields | Names |
|---|---|---|
| `ended` | | the target ended |
| `stopping` | | the target is still stopping |
| `refused` | `why` | the target could not be stopped |

#### An after refusal: `AfterWrong`

| Kind | Fields | Names |
|---|---|---|
| `itself` | | the item named is the held item itself |
| `finished` | | the item named is already finished |

#### An edited part: `EditedPart`

| Kind | Fields | Names |
|---|---|---|
| `title` | | the title |
| `what` | | what the work is |
| `do` | | what has to be done |
| `conclusion` | | the conclusion |
| `judgement` | | BMO's judgement |
| `intent` | | the work order's purpose |
| `order` | | the work order's instruction |

#### A closing part: `ClosingPart`

| Kind | Fields | Names |
|---|---|---|
| `report` | | the report |
| `unfinished` | | the unfinished note |

#### A verdict: `Verdict`

| Kind | Fields | Names |
|---|---|---|
| `single_job` | | the change fits one job |
| `cowork` | | the change needs co-work |

#### An impact difference: `ImpactDifference`

| Kind | Fields | Names |
|---|---|---|
| `differs` | `verdict`, `cowork` | the job's grounds come to another verdict than the co-work setting |
| `unfiled` | | BMO filed no grounds on the item |

#### A filing part: `FilingPart`

| Kind | Fields | Names |
|---|---|---|
| `title` | | the title |
| `what` | | what the work is |
| `do` | | what has to be done |
| `conclusion` | | the conclusion |
| `instruction` | | the messages that asked for it |
| `reply` | | the reply, by message or by text |

#### A resume condition: `ResumeCondition`

| Kind | Fields | Names |
|---|---|---|
| `item_finished` | `id` | the item resumes once another item finishes |
| `deployed` | `what` | the item resumes once a commit or an item's commits go live |

#### A written record: `WrittenRecord`

| Kind | Fields | Names |
|---|---|---|
| `live_claude` | `at`, `purpose`, `runs` | one opening the live-Claude guard kept, in the section a closing puts on the report |
| `strand_settled` | `[rows]` | the resolution of a strand whose rows all finished |
| `merged_report` | `target` | the report of an item closed with the item it merged into |
| `stopped_here` | | the opening of the note a run leaves where it stopped |

#### A line mark: `LineMark`

| Kind | Fields | Names |
|---|---|---|
| `merged_into` | `id` | the item was merged into another |
| `merging_into` | `id` | the item merges into another when that one is closed |
| `done` | | the item is finished |
| `pending_approval` | `requester` | the item waits for its requester's approval |
| `cowork` | | the item runs as co-work |
| `owner_note` | | the approval carried the owner's word |
| `report` | | the item has a report |
| `unfinished` | | the item has an unfinished note |
| `unfinished_resolved` | | the item's unfinished note is resolved |

#### A held word: `HeldWord`

| Kind | Fields | Names |
|---|---|---|
| `stop` | | stopped by the owner |
| `stopping` | | stopping while its run still ends |
| `pause` | | wound down to resume |
| `pausing` | | winding down while its run still goes |
| `failure` | | held after failing |
| `plain` | | set aside |
| `pending` | | waiting for its run to end before the hold takes |

#### A linked row: `RowState`

| Kind | Fields | Names |
|---|---|---|
| `done` | | finished |
| `open` | | still open |
| `merged` | | merged |
| `held` | | held |
| `opened` | | opened again |

#### A question's state: `QuestionState`

| Kind | Fields | Names |
|---|---|---|
| `waiting` | | still waiting for an answer |
| `typed` | `answer` | the owner answered in words |
| `chosen` | | the owner chose |
| `defaulted` | | no answer came and the recommendation was taken |
| `answered_in_pane` | | the confirmation closed before it was answered |

#### A model mark: `ModelMark`

| Kind | Fields | Names |
|---|---|---|
| `fallback` | | the provider order fell to another road |
| `changed` | | the runs went out on more than one model |
| `requested` | `model` | the model requested, where it differs from the one used |

#### A lapse: `Lapse`

| Kind | Fields | Names |
|---|---|---|
| `waiting` | | no answer came |
| `talking` | | the talk went quiet |
| `stopped` | | the clock stood stopped |

#### A pane window: `PaneWindow`

| Kind | Fields | Names |
|---|---|---|
| `permission` | | a confirmation asking whether a command may run |
| `unknown` | | a window the reading does not know |

#### A pane option: `PaneOption`

| Kind | Fields | Names |
|---|---|---|
| `allow` | | run the command as shown |
| `refuse` | | do not run it and tell the job to do it safely |
| `close` | | close the window and tell the job |
| `leave` | | leave it, to be answered in the pane |

#### A queue hold: `QueueHold`

| Kind | Fields | Names |
|---|---|---|
| `owner_pause` | | the owner paused automatic progress |
| `deploy_hold` | | a deployment holds the queue while a job ends |

#### A work step: `WorkStep`

| Kind | Fields | Names |
|---|---|---|
| `plan` | | the plan turn |
| `investigate` | | a stage that finds things out, and a stage of no sort the order knows |
| `review` | | a stage that checks a finding or the plan |
| `synthesis` | | the synthesis turn |
| `build` | | a build step |
| `final` | | the final turn |

#### A job label: `JobLabel`

| Kind | Fields | Names |
|---|---|---|
| `item` | `id`, `attempt?` | a job by its item, with which run of it past the first |
| `itemless` | | a job with no item and no line of why it was launched |

#### An unanswered turn: `Unanswered`

| Kind | Fields | Names |
|---|---|---|
| `resting` | | BMO is resting |
| `limit` | | the usage limit was reached |
| `outage` | | the API did not answer |
| `silent` | | the turn said nothing |
| `other` | | any other reason |

#### A release wait: `ReleaseWait`

| Kind | Fields | Names |
|---|---|---|
| `open_job` | `worker`, `job` | a job of the worker is still open |
| `lock` | | the checkout's job lock is held |
| `ended_units` | `worker`, `[units]` | ended job units of the worker are still alive |
| `ended_record` | `worker`, `unit`, `id`, `label`, `more` | an ended unit's run record is not closed, with how many more such records |
| `own_turns` | `[live]` | the order's own turns from before its stop are still alive |
| `checking` | `count`, `starts` | the repository check runs over the changed files |
| `release` | | the order waits for its turn to let the checkout go |

#### A line place: `LinePlace`

| Kind | Fields | Names |
|---|---|---|
| `resume_turn` | | its turn to resume |
| `passing` | | passing its turn on |
| `held` | | held |
| `owner_stop` | `why` | stopped by the owner |

#### A checkout note: `CheckoutNote`

| Kind | Fields | Names |
|---|---|---|
| `unsettled` | `tries`, `item` | the change kept changing under the check, so it was put on a branch off main |
| `unpushed` | `commit`, `refused` | the change was committed and not pushed |
| `uncommitted` | `why?` | the change could not be taken into a commit and stays in the checkout |
| `left` | `[paths]` | changes that could not be parted from others' stay in the checkout |
| `parked` | `count`, `branch`, `commit`, `stopped` | the change was parked on a branch |
| `not_parked` | `why?` | the change could not be parked and stays in the work path |
| `not_on_main` | `main`, `said` | the base checkout could not be taken back to the main branch |

#### A checkout place: `CheckoutPlace`

| Kind | Fields | Names |
|---|---|---|
| `own_worktree` | `worker` | the worker's own work path |
| `base_checkout` | | the base checkout |

#### A git failure: `GitFailure`

| Kind | Fields | Names |
|---|---|---|
| `no_branch` | `branch` | the branch is not there |
| `not_fetched` | `branch`, `said` | the branch would not come back |
| `step_failed` | `step` (a git step), `said` | a git step went wrong |

#### A git step: `GitStep`

| Kind | Fields | Names |
|---|---|---|
| `add` | | taking the change into the index |
| `commit` | | committing |
| `read_commit` | | reading a commit |
| `read_head` | | reading HEAD |
| `write_tree` | | writing a tree |
| `write_commit` | | writing a commit |
| `move_branch` | | moving a branch |
| `take_back` | | taking the change back out |
| `read_branch` | | reading the branch's change |

#### A run death: `RunDeath`

| Kind | Fields | Names |
|---|---|---|
| `exit` | `exit_code` | the run exited abnormally |
| `signal` | `signal` | the run was cut from outside |
| `error_result` | | the run went to its end and handed back an error |
| `silent` | | the run held together and only said it would wait |
| `transport` | | the run never had a pane |
| `unknown` | | nothing could read how it ended |

#### A span name: `SpanName`

| Kind | Fields | Names |
|---|---|---|
| `tool` | `name` | a tool of the CLI, by the name the protocol calls it |
| `server` | `server`, `tool` | a tool called through an MCP server, by the name the server is registered under |
| `coworker` | `agent_type?` | a co-worker a run handed work to |

#### A token kind: `TokenKind`

| Kind | Fields | Names |
|---|---|---|
| `input` | | tokens read |
| `output` | | tokens written |
| `cache_read` | | tokens read from the cache |
| `cache_write` | | tokens written to the cache |

#### A block head: `BlockHead`

| Kind | Fields | Names |
|---|---|---|
| `note` | `note`, `author`, `at` | words added to an item while it stood |
| `judgement` | | BMO's own judgement on a row it raised |
| `source` | `id`, `title`, `strand` | the strand of an unfinished note a raised row came out of |
| `raised_by_item` | `id` | the item whose work raised the row |
| `filed_by` | `filer` | a block the filer wrote as it filed the row |
| `fast_track` | `tool` | a fast-track promotion's first block |
| `message` | `id` | the archived message a copied message came from |

#### A wait target: `WaitTarget`

| Kind | Fields | Names |
|---|---|---|
| `unit` | `name` | a unit by its name |
| `pid` | `pid` | a process by its id |

#### A job ending: `JobEnding`

| Kind | Fields | Names |
|---|---|---|
| `done` | | the job worked |
| `work_step` | | a work order's step finished |
| `failed` | | the agent answered with nothing where a report was due |
| `live_tasks` | | the agent answered and left its own background work running |
| `unclosed` | | the agent answered while its own work was still open |
| `limit` | | the run met a usage limit |
| `error` | | the run never reached the work |
| `died` | | the runner died |
| `stopped` | | a person stopped it |
| `unanswered` | | the agent reported it could not finish its answer |
| `blocked` | | the run stopped waiting on something a person answers |

## Calls on other units

### bmo-agency

Agent sessions, the executions and runs they make, the log they write, the workers' runtime, routes and
the co-worker pool, the nodes, Claude accounts, the machine's room and the places work stands in are
bmo-agency's. The service reaches all of them through the calls below and nothing else. The crate
`job-agency` holds them: the trait `Agency` declares every call, and `AgencyClient` makes them over
HTTP to the address it is given.

- **The address** is the base of every path below, a plain `http:` address of bmo-agency on the
  machine. An address of any other kind is refused when the client is made.
- **A body is JSON, both ways,** with snake_case keys. A time is epoch milliseconds, a count and a
  token count are whole numbers, and a field written with a question mark may be null.
- **A path segment is an id as it stands,** percent-encoded where it needs to be.
- **An answer with no body is 204.** An answer with a body is 200, or 201 where the call creates
  something.
- **A read of one thing by its id answers 404 where it does not exist,** and the client reads that as
  nothing found, not as a failure.
- **Any other refusal** carries the body of "A refused request": `error` and `code`. The client hands
  both on as they stand, with the status, and decides nothing on the sentence.
- **A call that does not reach bmo-agency,** or an answer that does not read as its shape, is a failure
  of the call naming which call it was.

#### Sessions

A session is one agent's conversation on a provider, started for the agent itself, for a room or for a
worker. bmo-agency keeps it; the service starts, feeds, reads and ends it.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Start a session | `POST /sessions` | `agent`, `scope` | 201, a session |
| Send a turn | `POST /sessions/{id}/turns` | `prompt` | `delivery`: `taken`, or `blocked` where the session cannot take a turn now |
| Read its state | `GET /sessions/{id}/state` | | a session's state |
| Read its output | `GET /sessions/{id}/output` | | `session`, `generation`, `text`: what the session has written in its latest turn so far |
| Interrupt it | `POST /sessions/{id}/interrupt` | | 204 |
| Fold it | `POST /sessions/{id}/fold` | | 204 |
| End it | `DELETE /sessions/{id}` | | 204 |
| List the sessions | `GET /sessions` | | a list of sessions |

- **A scope** is an object holding `kind`: `agent`, `worker`, or `room` with the field `room`.
- **A session:** `id`, `agent`, `scope`.
- **A session's state:** `session`, `provider`, `model`, `place`, `generation` (how many times it has
  started over), and `context?`, which holds `carried` and `window`: the tokens it carries and the most
  it may.
- A turn, an interrupt, a fold or an end on a session that does not exist is refused with 404, and a
  turn to a session busy with another is refused with 409.

#### Executions

An execution is one run of an agent's process that bmo-agency launched and records. The service reads
them, and asks one cancelled where a job it carries is stopped.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| List the executions | `GET /executions?open={true or false}` | | a list of executions: only those not yet ended where `open` is true |
| Read one | `GET /executions/{id}` | | an execution |
| Ask it cancelled | `POST /executions/{id}/cancel` | | 204 |
| Drop the cancels it covers | `DELETE /executions/{id}/cancel` | | `dropped`: the list of executions whose asked cancel was dropped |

- **An execution:** `id`, `parent?`, `root?`, `line?`, `shared_pane` (true where it runs in a pane it
  shares), `started_at?`, `ended_at?`, `outcome?` (the outcome it recorded as it ended, as an object),
  and `details`: an object of every other field bmo-agency records of it, handed on as it stands.
- **The executions one covers** are itself, every execution whose parent chain leads to it, and every
  one whose `root` is it. A cancel stays asked until it is dropped.

#### Runs

A run is one entry of the run ledger: an agent's unit started, how deep it was, and how it ended.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Open a run | `POST /runs` | a run's opening | 201, `id` |
| Close a run | `POST /runs/{id}/close` | `finished_at`, `outcome`, `context_tokens?` | 204 |
| Read a run | `GET /runs/{id}` | | a run |

- **A run's opening:** `agent`, `chat_id?`, `unit`, `depth`, `cap_ms?`, `started_at`, `started_by?`,
  `search_id?`.
- **A run:** `id`, `agent`, `chat_id?`, `unit`, `depth`, `started_at`, `finished_at?`, `outcome?`,
  `context_tokens?`.
- A run already closed keeps its closing, and is refused with 409.

#### The agent log

The log is every line an agent's run wrote: its starts, turns, tool calls and their results. The service
reads it to count a run's starts and to tell what a job's run has done.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Count a run's starts | `GET /log/starts?agent={agent}&run={run}` | | `count` |
| Read a job's last line | `GET /log/jobs/{job}/last` | | `id`, `at`, `kind`, `name?` |
| Read a job's lines | `GET /log/jobs/{job}` | | a job's lines |

- **A job's lines:**
  - `calls`: each top-level tool call, as `id`, `agent`, `run?`, `reference?`, `name`, `source?`, `at`,
    `command?` (the command it ran, where it ran one) and `args?` (its arguments, cut to 2000
    characters);
  - `results`: each tool result, as `id`, `agent`, `run?`, `reference?`, `complete` (false where the
    result was cut), `length?` and `at`;
  - `requests`: how many model requests the job's run made;
  - `compactions`: how many times its context was compacted.
- A job with no line answers 404 to its last line and empty lists to its lines.

#### Workers' runtime

A worker's runtime is what bmo-agency holds of it as it runs: the provider and model it is pinned to,
whether its keepalive is paused, and the context it last carried.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read a worker's runtime | `GET /workers/{worker}/runtime` | | `worker`, `pin?`, `keepalive_paused_at?`, `context_tokens?` |
| Pin it | `PUT /workers/{worker}/pin` | a pin | 204 |
| Unpin it | `DELETE /workers/{worker}/pin` | | 204 |
| Pause its keepalive | `PUT /workers/{worker}/keepalive-pause` | `at` | 204 |
| Resume its keepalive | `DELETE /workers/{worker}/keepalive-pause` | | 204 |
| Record its context | `PUT /workers/{worker}/context-tokens` | `tokens` | 204 |

- **A pin:** `provider` and `model?`; a pin with no model leaves the model to the route.
- **The pin read is the one in force:** a support worker with no pin of its own is answered its base
  worker's.

#### Routes, tiers and pins

| Call | Method and path | Request | Answer |
|---|---|---|---|
| List the providers | `GET /providers` | | a list of `name`, `enabled`, `registered`, `lines` (the work lines it carries) |
| List the tiers | `GET /tiers` | | a list of `name`, `coworker` (true where a co-worker may run at it) |
| Resolve a route | `POST /routes/resolve` | `work_key`, `provider?`, `tier` | `provider`, `tier`, `model`, `effort?`, `answer_tokens?`; 404 where no route serves it |
| Check a pin | `POST /pins/check` | `value` | a pin choice |

- **A pin's value** is `auto`, a provider's name, or a provider and a model joined by `::`.
- **A pin choice** is an object holding `kind`: `auto`, or `pinned` with the fields of a pin. A value
  that names no provider that is on, or a model with no address, is refused with 400.

#### The co-worker pool

The pool holds every request for a co-worker's place and every reservation of seats, and leases places
as the tiers' capacity allows.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Enqueue a request | `POST /pool/requests` | a pool request's asking | 201, a pool request |
| Read a request | `GET /pool/requests/{token}` | | a pool request |
| List open requests | `GET /pool/requests?owner={owner}` | | a list of pool requests waiting or active; the owner may be left out |
| Lease its place | `POST /pool/requests/{token}/lease` | | a pool request, active where a place was leased |
| Bind its child | `POST /pool/requests/{token}/bind` | `child` | a pool request |
| Finish it | `POST /pool/requests/{token}/finish` | `outcome`, `cancel` | `finished`: false where it had already ended |
| Reserve seats | `POST /pool/reservations` | a reservation's asking | 201, a reservation |
| List held reservations | `GET /pool/reservations?owner={owner}` | | a list of reservations held; the owner may be left out |
| Release a reservation | `POST /pool/reservations/{token}/release` | `reason` | `released`: false where it was no longer held |
| Record a place's write | `PUT /pool/places/{place}/write` | `provider`, `model`, `effort?`, `upstream?` | 204 |

- **A pool request's asking:** `request_key`, `owner`, `source`, `role`, `work_key`, `provider`,
  `model`, `effort?`, `tier`, `yielding`, `place_key?`, `turn`, `reservation?`.
- **A pool request:** the asking's fields, and `token`, `state` (`waiting`, `active` or `ended`),
  `enqueued_at`, `leased_at?`, `child?`, `ended_at?` and `outcome?`.
- **A reservation's asking:** `reservation_key`, `owner`, `source`, `role`, `provider`, `model`,
  `effort?`, `tier`, `wanted` (how many seats).
- **A reservation:** the asking's fields, and `token`, `state` (`held` or `released`) and
  `created_at`.
- **Cancel** finishes a request still waiting without its having run; a request that conflicts with
  the pool's present state is refused with 409.

#### Nodes

| Call | Method and path | Request | Answer |
|---|---|---|---|
| List the nodes | `GET /nodes` | | a list of `name`, `label`, `kind`, `role?`, `state`, `checked_at?` |
| Read a node's SSH reach | `GET /nodes/{name}/reach` | | `host`, `port`, `user`, `key_path?`, `shell?` |
| Place a heavy run | `POST /nodes/heavy-run` | | `machine?`: `name`, `label`, `node?`, the machine the next heavy run goes to, null where none has room |

- Placing a heavy run moves the rotation on, so each call may answer the next machine.

#### Claude accounts

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Check a requester's account | `GET /accounts/claude/held?user={user}` | | `held`: true where an enabled Claude account is held by the user |

#### The machine's room

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read the room on the machine | `GET /machine/room` | | the room |
| Tell launches are held | `PUT /machine/launch-held` | `at`, `held` | 204 |

- **The room:** `held` (why launches are held now, empty where there is room), `jobs`,
  `browser_jobs`, `browsers`, and `details`: an object of every other reading of the machine,
  handed on as it stands.
- **Launches held:** the service records them under its own setting `machine_launch_held` and tells
  bmo-agency the reasons. bmo-agency watches the machine, and once the room returns it forgets the
  notice and tells the queue through the service's own API. A notice with an empty `held` withdraws it.

#### Places

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read a worker's workbench | `GET /places/workbench/{worker}` | | `root`, `scratches` (the item scratch directories under it) |
| Read a pool's branch slots | `GET /places/branch-slots/{pool}` | | `root`, `slots`: a list of `slot` and `dir` |

- A path answered is absolute on the machine the service runs on.

### bmo-receiver

Rooms are bmo-receiver's: the chats it receives, their owner, the messages said in them, the agents who
answer in them, the instructions it hands an agent's session there, and the owner's talks about an item
or a document. The service reaches all of them through the calls below and nothing else. The crate
`job-receiver` holds them: the trait `Receiver` declares every call, and `ReceiverClient` makes them
over HTTP to the address it is given.

- **The address, the bodies, the path segments, the statuses, the refusals and the failures** read as
  they do for bmo-agency above, with bmo-receiver in its place.
- **A room is named by its chat,** a whole number that may be negative.
- **No sentence a person reads goes from the service:** what a room is told goes as a fact, and
  bmo-receiver's agent says it in the room's language.

#### Instructions

An instruction is what an agent's session in a room is handed to act on: a fact to tell the room, or an
order in English for the agent itself, which no person reads. bmo-receiver keeps it, frames it and hands
it over as the session takes it up. The service queues one, reads how far it has gone, and withdraws one
not yet taken.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Queue an instruction | `POST /instructions` | an instruction | 201, `id` |
| Read its state | `GET /instructions/{id}` | | `state`: `pending`, `handed` or `delivered` |
| Withdraw it | `DELETE /instructions/{id}` | | `withdrawn`: false where it was handed already or is gone |

- **An instruction:** `agent?`, `chat?`, `said`, `kind?`, `sort`, `summary?`, `expires_at?`, `[events]`,
  `rides_until?`, `via?`, `run?`.
  - `agent` is the agent whose session takes it, the deployment's agent where null; `chat` is the room,
    the owner's private chat where null.
  - `said` is an object of one key: `fact`, holding a fact as "Facts" above gives it, or `order`, holding
    the words of an order.
  - `kind` names what it is about: queueing one replaces the pending instruction of the same kind in the
    room.
  - `sort` names its family, such as `job`, `notice`, `overtime` or `run_failure`; `summary` is the line
    it is listed under, made from the fact where null.
  - `expires_at` is when it lapses; `events` are the events its delivery carries; `rides_until` makes it
    ride the next turn rather than open one, until that time; `via` is the road the session was read on,
    which it is delivered on; `run` is the run whose result it is.
- **A failed run's last action** travels inside its fact, `run_failed`, as `last_tool` and
  `tool_detail`, and bmo-receiver says it.
- An instruction withdrawn, replaced or lapsed reads as nothing found.

#### Voiced notices

A voiced notice is a fact a room is told by the agent that speaks for it, its voice, whoever that is.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Voice a notice | `POST /rooms/{chat}/notices` | a notice | 201, `id`: the instruction it was queued as |

- **A notice:** `fact` (a fact), `intent` (what the line is for, in English for the agent), `follow_up?`
  (what follows it), `optional` (true where the voice may leave it unsaid when the fact leaves the room
  nothing to hear), `kind?`, `expires_at?`, `[events]` and `rides_until?`, the last four read as an
  instruction's.

#### Rooms

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read the owner | `GET /owner` | | `sender` (the owner as a sender), `chat` (the owner's private chat) |
| Read a room | `GET /rooms/{chat}` | | a room |
| Read a room's answerers | `GET /rooms/{chat}/answerers` | | the answerers |

- **A room:** `chat`, `owner_room` (true where it is the owner's private chat with the deployment's
  agent), `registered` (true where its registration stands), and `work_chat`: where the news, questions
  and answers of work filed from it are told, the room itself while its registration stands and the
  owner's private chat once it is gone. Any chat reads as a room; one never registered reads as one
  whose registration is gone.
- **The answerers:** `default` (the agent that answers where a room names none), `voice` (the agent
  that speaks for the room), and `answerers`: a list of `agent` and `companion` (true where it answers
  beside the voice rather than as it), every agent named to answer in the room that is not resting.

#### Messages

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read a message | `GET /rooms/{chat}/messages/{message}` | | a message |

- **A message:** `sender?` (null where no sender is known), `sender_name?`, `sent_at`, `text`. A
  message bmo-receiver does not keep reads as nothing found.
- An item is filed with the sender of the first message it quotes as its requester.

#### Talks

A talk is the owner's conversation with the deployment's agent about an item or a document. The service
hands bmo-receiver the topic, the turns said so far and what the owner says now, and bmo-receiver's agent
answers it.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Talk | `POST /talks` | `topic`, `[turns]`, `said` | `answer?`: null where the agent said nothing |

- **A topic** is an object holding `kind`:
  - `item`, with `id`, `title`, `done`, `merged_into?`, `held` (true where it is held), `hold?` (why:
    `stop`, `pause`, `failure` or any other hold), `stopping` (true where a run of it still ends),
    `approved` and `status`;
  - `document`, with `document`, `label`, `writer`, `file`, `version`, `heading?`, `passage?` (the part
    the owner selected) and `translated`.
- **A turn:** `owner` (true where the owner said it) and `text`.
- **A talk that comes back with nothing** is refused: `resting` with 409 where the agent rests or the
  owner's chat has fallen silent, `limit` with 429 where its provider's limit is reached, `outage` with
  503 where its provider is out, and any other code for any other failure.

#### Room holdings

bmo-receiver asks every unit that holds rows naming a room what the room holds before it removes the
room. A unit declares itself a holder, and bmo-receiver asks it through the unit's own API: for the
service, its route for the rooms an item belongs to, which answers `RoomHoldings`.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Declare the service a holder | `PUT /room-holders/{unit}` | `address`, `[holdings]` | 204 |

- **`unit`** is the service's unit name and **`address`** the base of its own API, both from the
  deployment.
- **A holding:** `name` and `effect`: `kept` where the rows stay and keep the room's chat after it is
  removed, `warns` where bmo-receiver shows what the unit answers before it removes the room.
- Declaring again replaces the unit's declaration.

### bmo-web

Users, the senders linked to them, the grants they hold and the account model they stand in are
bmo-web's. The service reaches them through the calls below and nothing else. The crate `job-web` holds
them: the trait `Web` declares every call, and `WebClient` makes them over HTTP to the address it is
given.

- **The address, the bodies, the path segments, the statuses, the refusals and the failures** read as
  they do for bmo-agency above, with bmo-web in its place.
- **A user is named by its id as bmo-web gives it,** a string the service hands on as it stands, to
  bmo-agency's Claude account check too.
- **A sender** is a whole number, as bmo-receiver names a message's sender.

#### The account model

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read whether the model is in force | `GET /accounts/model` | | `in_force`: true where the account model is in force |

- **The model is in force** once bmo-web has moved the deployment's accounts into it. Until then no
  sender is linked to a user and no grant stands, so the service asks neither.

#### Users

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read a sender's user | `GET /senders/{sender}/user` | | `user`: the user the sender is linked to |

- A sender linked to no user reads as nothing found.

#### Checks

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Check a user's grant on an object | `POST /checks/object` | `kind`, `object`, `relation`, `user` | `allowed`, `grant` |

- **`kind`** is `bmo_worker` for a worker or `bmo_repository` for a repository, and **`object`** names
  the one asked about. **`relation`** is one its kind declares: `run` on a worker, `change` on a
  repository.
- **`allowed`** is true where the user holds the relation; the platform administrator holds every one.
  **`grant`** is what was asked for, `kind:object#relation`, as the service names it when it says what
  a user lacks.
- A kind or a relation bmo-web's rules do not declare is refused.

### bmo-gate

The gates are bmo-gate's: the translator, the sanitiser, the translations they keep, and the
selection of the brain's knowledge a piece of work is handed. The service
reaches them through the calls below and nothing else. The crate `job-gate` holds them: the trait
`Gate` declares every call, and `GateClient` makes them over HTTP to the address it is given.

- **The address, the bodies, the statuses, the refusals and the failures** read as they do for
  bmo-agency above, with bmo-gate in its place. No path carries a segment of the service's.
- **A language** is a language tag, such as `en`. The language a worker reads and the language the
  owner reads come from the deployment's settings.
- **An original** is a text exactly as it was written. A translation is kept under its original and
  its language, so the same original is never translated twice into one language.
- **A read of many originals is a post,** since originals are too long for a path.

#### Translations

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read kept translations | `POST /translations/kept/read` | `language`, `[originals]` | `[kept]`, each `original` and `text` |
| Translate | `POST /translations` | `language`, `[originals]` | `[readings]`, each `text` and `whole` |
| Keep a translation | `PUT /translations/kept` | `language`, `original`, `text` | |

- **Reading kept translations** never asks the translator. `kept` holds one entry for each original
  that has a translation kept in the language and none for the rest.
- **Translating** answers one reading for each original, in the order given:
  - an original with no part in another language stands as it is;
  - any other has its parts in another language translated, each part standing beside the rest as it
    did in the original, and is kept;
  - a part the translator could not render stays in its original, the reading has `whole` false, and
    nothing is kept for it, so the next call asks again.
- **Keeping a translation** keeps `text` as the reading of `original` in the language, in place of any
  kept before. The service keeps a worker's own English this way as the reading of the rendering
  written on the row, so a worker reading the item later is handed the English that was written.

#### The sanitiser

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Sanitise words | `POST /sanitised` | `text`, `label`, `chat?` | `text`, `changed` |

- **The sanitiser** reads words on their way into a model's context for security-intrusion material
  and defuses the smallest spans that carry it, leaving the rest as it was. `label` names what the
  words are, for the sanitiser's instructions, and `chat` the room whose run the words belong to,
  where there is one.
- **The answer** is the words as they are to be handed on, and `changed` is true where anything was
  defused. Words with nothing worth judging come back as they are. Where the sanitiser has no answer,
  bmo-gate blanks the words that made them worth judging, so they are never handed on raw.
- **A run of the sanitiser that was stopped** is refused with status 409 and code `stopped`.

#### Knowledge for a piece of work

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Select knowledge | `POST /knowledge/selected` | `agent`, `chat`, `[lines]` | `[pieces]`, each `path` and `text` |

- **Selecting knowledge** picks, from the brain's libraries the agent may be offered in the room, the
  pieces a piece of work's own lines call for: a job's item and task, or a search's query. The
  narrowing, the pick and the count of what was attached are bmo-gate's; bmo-brain only serves the
  material it picks from.
- **`pieces`** holds each picked piece's path in the brain and its text, in the order picked, and is
  empty where the work has no line or nothing was picked.
- **The service** sets the pieces into the work's prompt under its own heading. A call that fails
  leaves the work with no knowledge rather than holding it back.

### bmo-brain

The brain is bmo-brain's: the memories every agent keeps, the holds a job's run sets on the brain, and
the editing locks the brain's editors take. The service reaches them through the calls below and
nothing else. The crate `job-brain` holds them: the trait `Brain` declares every call, and
`BrainClient` makes them over HTTP to the address and with the token it is given.

bmo-brain states its own wire conventions in its boundary document, and the calls keep them:

- **The address** is the base of every path below, a plain `http:` address of bmo-brain on the machine.
  An address of any other kind is refused when the client is made.
- **Every call carries bmo-brain's token** as a bearer token in its `Authorization` header. The token
  is given with the address.
- **A body is JSON, both ways, with camelCase keys.** A time is Unix seconds, and a room, an item and a
  row are named by their numbers. An optional field with no value is left out; a field written with a
  question mark may be null.
- **An answer is 200 and says whether it was carried out:** `answered` true with the call's fields
  beside it, or `answered` false with a `refusal` holding `code` and `message`, an English sentence.
  The client hands a refusal on as it does any other, with status 200, its code and its sentence.
- **A status that is no success** carries `code` and `error`, and the client hands both on with the
  status as they stand.
- **A call that does not reach bmo-brain,** or an answer that does not read as its shape, is a failure
  of the call naming which call it was.
- **The agent** a call is for is named by its key, which the deployment's settings give.

#### Brain holds

A job's run that writes into the brain holds the brain's editors off, so an editor and a job never
write it at once. The hold is bmo-brain's: the service sets it and reads it, and bmo-brain lets it go
once the run is no longer open.

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Hold the brain | `POST /brain/hold` | `run` | `held`, `[taken]` |
| Read the holds | `GET /brain/hold` | | `[holders]`, `[taken]` |
| Wait for the editors | `POST /brain/wait` | | `free`, `[lines]` |

- **Holding the brain** sets the hold of `run`, the run's name, where none stands yet. `held` says
  whether the hold stands, and `taken` names each editing lock a brain editor holds now, looked at
  after the hold was set and without waiting. The hold is set before the locks are looked at, so an
  editor beginning after the look waits for the job, and an editor already writing is the only one the
  job waits for: the service refuses a job's write into the brain while its hold stands and a lock is
  taken.
- **Reading the holds** names the runs holding the brain and the editing locks taken now. A hold whose
  run is over is let go as it is read.
- **Waiting for the editors** looks every five seconds, for nine minutes at most, until no editing lock
  is taken. `free` says whether every lock came free, and `lines` is the outcome for a person, a line
  each. The client sets no limit of its own on the call.

#### Memories

| Call | Method and path | Request | Answer |
|---|---|---|---|
| Read a room's memories | `GET /memories?agent={agent}&chat={chat}` | | `[memories]` |
| File a memory | `POST /memory/add` | `turn`, `type`, `action`, `todo`, `chat`, `slug` | `[lines]` |

- **Reading a room's memories** answers every row the agent keeps in the room, newest first. Of each
  row the service reads `id`, `slug`, `type`, `chatId?`, `action`, `todoId?`, `active`, `setAt` and
  `doneAt?`, and leaves the rest of the row unread.
- **Filing a memory** is the memory tool's own call. `turn` names the turn it is filed in: its `agent`,
  `chatId?` and `messageId?`, both null for a filing of no turn. The rest are the tool's arguments:
  `type` and `action` always, and `todo`, the item the row is tied to, `chat`, the room it is filed
  in, and `slug`, its name, where they have one. The answer is the tool's reading, a line each, for a
  person; the number the row was given is read back by reading the room's memories.
- **An item's closing** files a `followup` for each strand of its unfinished note left to the agent:
  in the owner's room, tied to the item. It reads the room's memories first, so a strand an earlier
  closing filed is not filed again.

### bmo-testbed

Checks, suites, heavy commands, a repository's close line, the unit links into a checkout and the
commits waiting to go live are bmo-testbed's. Its API is its binary, `bmo-testbed`, whose first word is
the verb; it serves no HTTP. The service reaches it through the calls below and nothing else. The crate
`job-testbed` holds them: the trait `Testbed` declares every call, and `TestbedClient` makes them by
running the binary at the path it is given.

- **The program** is the binary's absolute path, from the deployment. A path of any other kind is
  refused when the client is made.
- **A call runs one verb with the words below,** in the checkout it names where it names one, with the
  service's own environment handed on: the testbed reads the job's agent process and its own variables
  from it.
- **A call answering a structure** asks for `--json` and reads standard output as JSON. **A call that
  runs something,** a check, a count or a suite, writes standard output and standard error together to
  the log file the call names, and answers its exit code read as an outcome. A close hands its output on
  as the service's own.
- **An outcome** is `passed` for exit code 0, `failed` for 1 and `not_run` for 75: the turn did not
  come, and the same call is made again.
- **Exit code 2 is a refusal,** handed on with the last line the verb wrote as it stands.
- **An exit code the call does not name,** a program ended by a signal, a program that does not start, a
  log file that cannot be written or an answer that does not read as its shape is a failure of the call
  naming which call it was.

#### Checks and the suite

| Call | Words | Answer |
|---|---|---|
| Check a change | `check [--since <commit>]` | an outcome |
| Count the tests | `count [--since <commit>]` | an outcome |
| Run the suite | `suite [--since <commit>] [--again] [--guard <s>] [--file-guard <s>] [--turn-wait <s>] [<word>…]` | an outcome |
| Read a suite's log | `log <file> --json` | a reading, on exit code 0 or 1 |
| Close in the close line | `close -- <command…>` | the command's exit code, or `not_run` |

- **A check** is the change check: the static steps and the quick checks over what changed since the
  commit. **A count** fails where the range removed a test that is not accounted for.
- **A suite** runs the units the change since the commit reaches, or all of them with no commit. A word
  naming a path narrows the files, a word that is a unit's name picks that unit, and every other word
  goes to each file's runner; `--again` runs once more what failed or never started. The guards and the
  wait for the turn are whole seconds.
- **A reading of a log:** `passed`, `failed` and `skipped` (counts), `verdict`, `failed_units`,
  `unstarted_units`, and `failures`, each with `test` and `unit`.
- **A close** stands in the close line of the checkout's repository and, once its turn comes, runs the
  command there, standing next after the front for every heavy command the command starts, and passes
  the command's exit code on. Exit code 75 is a close whose job was gone before its turn came; every other exit code is the
  command's, a 2 among them, so a close is never read as a refusal. A
  close's check and suite are calls its command makes inside the line.

#### Heavy commands

| Call | Words | Answer |
|---|---|---|
| Classify a shell line | `classify --json <line>` | a classification, on exit code 0 (light) or 1 (heavy) |

- **A classification:** `heavy` (true where the line starts a heavy command outside the testbed),
  `language?` (the language that claims it), `command?` (its words) and `instead?` (the line that runs
  it in the testbed).
- A job's shell hook refuses a heavy command started outside the testbed, naming `instead`.

#### Unit links into a checkout

An installed unit may be linked to a file in a checkout rather than in the deploy tree. As the checkout
moves, the link follows its file by the rule bmo-testbed keeps the deploy tree's links by: a link the
commit would leave dangling is pointed at the commit's one tracked file of the unit's own name.

| Call | Words | Answer |
|---|---|---|
| Plan a move | `units plan --to <commit> [--hold] --json` | a plan, on exit code 0 (the move may go) or 1 (held) |
| Relink after a move | `units relink --json` | the relinked links, on exit code 0 or 1 (something left dangling or unmatched) |

- **A plan:** `relinks`, each with `name` and `file` (the path in the commit it is pointed at);
  `running`, the units running in a folder the commit no longer has, read only with `--hold`; and
  `unmatched`, the links with no single file of their name in the commit. A move is held where
  `running` or `unmatched` holds anything.
- **The relinked links:** `relinked`, `dangling` and `unmatched`, each a list of names. Every link the
  checkout's present commit leaves dangling is relinked, and the user's units are read again where any
  moved.

#### Deploys

| Call | Words | Answer |
|---|---|---|
| Record a pending deploy | `pending add --commit <sha> [--item <id>] [--worker <name>] [--job <id>] --reason <words> --json` | the pending deploy standing for the commit |
| List pending deploys | `pending list [--job <id> \| --item <id>] --json` | `[pending deploys]` |
| Read the live deploy | `deployed --json` | `commit` and `release`, or null where nothing is live |

- **A pending deploy** is a commit of the checkout's repository landed on main and waiting to go live:
  `repository` (its declared name), `commit`, `item?`, `worker?`, `job?`, `reason` (in English),
  `urgent` (true where bmo-testbed reads the commit as one to put live ahead of the rest) and
  `created_at`.
- Recording a commit already recorded keeps the first record and answers it.
- A list is every repository's on the machine, in the order they were recorded, or those of one job or
  one item.
- **The live deploy** is the checkout's repository's: the commit its deploy tree runs and the release
  directory that holds it.
