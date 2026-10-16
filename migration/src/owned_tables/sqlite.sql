-- The tables bmo-job owns, on SQLite (docs/schema.md). Each table's columns, in order, are the same as
-- on PostgreSQL; a key column is declared NOT NULL, since SQLite would otherwise let a text key be null.

CREATE TABLE "todos" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "title" TEXT NOT NULL,
  "instruction" TEXT NOT NULL,
  "requester" TEXT NOT NULL,
  "requester_id" INTEGER,
  "chat_id" INTEGER,
  "approved" INTEGER NOT NULL DEFAULT 0,
  "done" INTEGER NOT NULL DEFAULT 0,
  "priority" INTEGER NOT NULL,
  "status" TEXT NOT NULL,
  "model" TEXT,
  "note" TEXT,
  "report" TEXT,
  "unfinished" TEXT,
  "unfinished_resolved_at" INTEGER,
  "unfinished_resolution" TEXT,
  "created_at" INTEGER NOT NULL,
  "done_at" INTEGER,
  "held_at" INTEGER,
  "held_reason" TEXT,
  "held_kind" TEXT,
  "resume_after" INTEGER,
  "merged_into" INTEGER,
  "merged_at" INTEGER,
  "worker" TEXT,
  "cowork" INTEGER,
  "repairs_todo_id" INTEGER,
  "resume_after_deploy" TEXT,
  "approved_via" TEXT,
  "agent" TEXT,
  "impact" TEXT,
  "job_impact" TEXT,
  "brief" TEXT,
  "brief_korean" TEXT
);
CREATE INDEX "todos_merged_into" ON "todos" (merged_into);
CREATE INDEX "todos_queue" ON "todos" (done, approved, priority, id);
CREATE INDEX "todos_repairs_todo_id" ON "todos" (repairs_todo_id);

CREATE TRIGGER todos_merge_insert BEFORE INSERT ON todos
WHEN NEW.merged_into IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'todo merge: an item is never merged into itself') WHERE NEW.merged_into = NEW.id;
  SELECT RAISE(ABORT, 'todo merge: the item merged into does not exist')
    WHERE NOT EXISTS (SELECT 1 FROM todos WHERE id = NEW.merged_into);
  SELECT RAISE(ABORT, 'todo merge: the item merged into is itself merged')
    WHERE (SELECT merged_into FROM todos WHERE id = NEW.merged_into) IS NOT NULL;
  SELECT RAISE(ABORT, 'todo merge: other items are merged into this one')
    WHERE EXISTS (SELECT 1 FROM todos WHERE merged_into = NEW.id);
END;

CREATE TRIGGER todos_merge_update BEFORE UPDATE OF merged_into ON todos
WHEN NEW.merged_into IS NOT NULL AND NEW.merged_into IS NOT OLD.merged_into
BEGIN
  SELECT RAISE(ABORT, 'todo merge: an item is never merged into itself') WHERE NEW.merged_into = NEW.id;
  SELECT RAISE(ABORT, 'todo merge: the item merged into does not exist')
    WHERE NOT EXISTS (SELECT 1 FROM todos WHERE id = NEW.merged_into);
  SELECT RAISE(ABORT, 'todo merge: the item merged into is itself merged')
    WHERE (SELECT merged_into FROM todos WHERE id = NEW.merged_into) IS NOT NULL;
  SELECT RAISE(ABORT, 'todo merge: other items are merged into this one')
    WHERE EXISTS (SELECT 1 FROM todos WHERE merged_into = NEW.id);
END;

CREATE TRIGGER todos_merge_delete BEFORE DELETE ON todos
WHEN EXISTS (SELECT 1 FROM todos WHERE merged_into = OLD.id)
BEGIN
  SELECT RAISE(ABORT, 'todo merge: other items are merged into this one');
END;

CREATE TABLE "todo_notes" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "todo_id" INTEGER NOT NULL,
  "author" TEXT NOT NULL,
  "author_id" INTEGER,
  "text" TEXT NOT NULL,
  "added_at" INTEGER NOT NULL,
  "job_id" INTEGER,
  "delivered_at" INTEGER,
  "removed_at" INTEGER,
  "removed_by" TEXT,
  "kind" TEXT
);
CREATE INDEX "todo_notes_item" ON "todo_notes" (todo_id, id);

CREATE TABLE "todo_sources" (
  "todo_id" INTEGER NOT NULL,
  "source_todo_id" INTEGER NOT NULL,
  PRIMARY KEY ("todo_id", "source_todo_id")
);
CREATE INDEX "todo_sources_source" ON "todo_sources" (source_todo_id);

CREATE TABLE "todo_approvals" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "todo_id" INTEGER NOT NULL,
  "approved" INTEGER NOT NULL,
  "via" TEXT,
  "who_kind" TEXT NOT NULL,
  "who" TEXT NOT NULL,
  "road" TEXT NOT NULL,
  "at" INTEGER NOT NULL
);
CREATE INDEX "todo_approvals_item" ON "todo_approvals" (todo_id, id);

CREATE TABLE "work_orders" (
  "todo_id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "body" TEXT NOT NULL
);
CREATE INDEX "work_orders_status" ON "work_orders" (json_extract(body, '$.status'));

CREATE TABLE "work_order_locks" (
  "todo_id" INTEGER NOT NULL PRIMARY KEY,
  "host" TEXT NOT NULL,
  "pid" INTEGER NOT NULL,
  "start" TEXT NOT NULL,
  "token" TEXT NOT NULL,
  "taken_at" INTEGER NOT NULL
);

CREATE TABLE "jobs" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "chat_id" INTEGER NOT NULL,
  "unit" TEXT NOT NULL,
  "launched_at" INTEGER NOT NULL,
  "prompt" TEXT NOT NULL,
  "finished_at" INTEGER,
  "report" TEXT,
  "ok" INTEGER,
  "started_by" TEXT,
  "todo_id" INTEGER,
  "activity_version" INTEGER,
  "model" TEXT,
  "outcome" TEXT,
  "execution_mode" TEXT,
  "execution_model" TEXT,
  "execution_effort" TEXT,
  "execution_tier" TEXT,
  "execution_upstream" TEXT,
  "worker" TEXT,
  "purpose" TEXT,
  "prompt_english" TEXT,
  "prompt_korean" TEXT
);
CREATE INDEX "jobs_open" ON "jobs" (id) WHERE finished_at IS NULL;
CREATE INDEX "jobs_todo" ON "jobs" (todo_id);
CREATE INDEX "jobs_unit" ON "jobs" (unit, id);
CREATE INDEX "jobs_finished" ON "jobs" (finished_at);
CREATE INDEX "jobs_launched" ON "jobs" (launched_at);
CREATE INDEX "jobs_report" ON "jobs" (length(report), finished_at);

CREATE TABLE "job_spans" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "job_id" INTEGER NOT NULL,
  "todo_id" INTEGER,
  "kind" TEXT NOT NULL,
  "name" TEXT NOT NULL,
  "seconds" REAL NOT NULL,
  "times" INTEGER NOT NULL DEFAULT 1,
  "tokens" INTEGER,
  "input_tokens" INTEGER,
  "output_tokens" INTEGER,
  "cache_read_tokens" INTEGER,
  "cache_write_tokens" INTEGER
);
CREATE INDEX "job_spans_job" ON "job_spans" (job_id);
CREATE INDEX "job_spans_todo" ON "job_spans" (todo_id);

CREATE TABLE "job_questions" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "job_id" INTEGER,
  "todo_id" INTEGER NOT NULL,
  "chat_id" INTEGER NOT NULL,
  "question" TEXT NOT NULL,
  "options" TEXT NOT NULL,
  "recommended" INTEGER NOT NULL,
  "why" TEXT NOT NULL,
  "asked_at" INTEGER NOT NULL,
  "due_at" INTEGER NOT NULL,
  "chosen" INTEGER,
  "answer" TEXT,
  "paused_left" INTEGER,
  "settled_at" INTEGER,
  "settled_by" TEXT,
  "talks" INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX "job_questions_open" ON "job_questions" (chat_id, settled_at);

CREATE TABLE "question_talk" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "question_id" INTEGER NOT NULL,
  "by" TEXT NOT NULL,
  "text" TEXT NOT NULL,
  "said_at" INTEGER NOT NULL,
  "heard_at" INTEGER
);
CREATE INDEX "question_talk_question" ON "question_talk" (question_id);

CREATE TABLE "checkout_leases" (
  "worker" TEXT NOT NULL PRIMARY KEY,
  "job_id" INTEGER NOT NULL,
  "taken_at" INTEGER NOT NULL
);

CREATE TABLE "branch_agents" (
  "pool" TEXT NOT NULL DEFAULT '',
  "slot" INTEGER NOT NULL,
  "mode" TEXT NOT NULL,
  "model" TEXT NOT NULL,
  "role" TEXT NOT NULL,
  "upstream" TEXT NOT NULL DEFAULT '',
  "prefix_digest" TEXT NOT NULL DEFAULT '',
  "last_reply_at" INTEGER NOT NULL,
  PRIMARY KEY ("pool", "slot", "mode", "model", "upstream")
);

CREATE TABLE "session_work_items" (
  "session_id" TEXT NOT NULL PRIMARY KEY,
  "todo_id" INTEGER NOT NULL,
  "served_at" INTEGER NOT NULL
);

CREATE TABLE "session_work_results" (
  "session_id" TEXT NOT NULL,
  "result" TEXT NOT NULL,
  "version" TEXT NOT NULL,
  "loaded_at" INTEGER NOT NULL,
  PRIMARY KEY ("session_id", "result")
);

CREATE TABLE "live_claude_openings" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "job_id" INTEGER NOT NULL,
  "todo_id" INTEGER,
  "purpose" TEXT NOT NULL,
  "opened_at" INTEGER NOT NULL
);
CREATE INDEX "live_claude_openings_job" ON "live_claude_openings" (job_id);

CREATE TABLE "live_claude_runs" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "opening_id" INTEGER NOT NULL,
  "what" TEXT NOT NULL,
  "ran_at" INTEGER NOT NULL
);
CREATE INDEX "live_claude_runs_opening" ON "live_claude_runs" (opening_id);

CREATE TABLE "clock_holds" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "run" TEXT NOT NULL,
  "agent" TEXT NOT NULL,
  "what" TEXT NOT NULL,
  "clock" TEXT NOT NULL,
  "clock_ms" INTEGER NOT NULL,
  "started_at" INTEGER NOT NULL,
  "last_activity_at" INTEGER,
  "last_activity" TEXT,
  "asked_at" INTEGER NOT NULL,
  "answer_by" INTEGER NOT NULL,
  "instruction_id" INTEGER,
  "verdict" TEXT,
  "keep_until" INTEGER,
  "decided_by" TEXT,
  "why" TEXT,
  "decided_at" INTEGER,
  "closed" TEXT,
  "closed_at" INTEGER
);

CREATE TABLE "workers" (
  "name" TEXT NOT NULL PRIMARY KEY,
  "label" TEXT,
  "cwd" TEXT,
  "icon" TEXT,
  "builtin" INTEGER NOT NULL DEFAULT 0,
  "preamble" TEXT,
  "dev_command" TEXT,
  "support_count" INTEGER NOT NULL DEFAULT 0,
  "support_keepalive" INTEGER NOT NULL DEFAULT 0,
  "support_of" TEXT,
  "support_slot" INTEGER,
  "node" TEXT,
  "role" TEXT,
  "resting_since" INTEGER,
  "created_at" INTEGER NOT NULL
);

CREATE TABLE "settings" (
  "key" TEXT NOT NULL PRIMARY KEY,
  "value" TEXT NOT NULL,
  CONSTRAINT "settings_key_owned" CHECK (
    "key" IN (
      'queue_paused', 'deploy_hold', 'deploy_waits_on', 'machine_launch_held', 'failure_orders',
      'job_queue_told', 'work_retry_model'
    )
    OR "key" GLOB 'job_stop:?*'
    OR "key" GLOB 'job_route_attempts:?*'
    OR "key" GLOB 'job_route_block:?*'
    OR "key" GLOB 'job_report_waiting:?*'
    OR "key" GLOB 'job_ending_waiting:?*'
    OR "key" GLOB 'job_report_roads:?*'
    OR "key" GLOB 'job_wait:?*'
    OR "key" GLOB 'job_command:?*'
    OR "key" GLOB 'job_quiet_told:?*'
    OR "key" GLOB 'job_spin_told:?*'
    OR "key" GLOB 'job_queue_waiting:?*'
    OR "key" GLOB 'job_queue_waiting_told:?*'
    OR "key" GLOB 'todo_repair_ceiling_told:?*'
  )
);
