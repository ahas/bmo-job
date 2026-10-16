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
