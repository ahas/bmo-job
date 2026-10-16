-- Takes away what the PostgreSQL schema of the owned tables made (docs/schema.md). The merge triggers go
-- with their table.

DROP TABLE "settings";
DROP TABLE "workers";
DROP TABLE "clock_holds";
DROP TABLE "live_claude_runs";
DROP TABLE "live_claude_openings";
DROP TABLE "session_work_results";
DROP TABLE "session_work_items";
DROP TABLE "branch_agents";
DROP TABLE "checkout_leases";
DROP TABLE "question_talk";
DROP TABLE "job_questions";
DROP TABLE "job_spans";
DROP TABLE "jobs";
DROP TABLE "work_order_locks";
DROP TABLE "work_orders";
DROP TABLE "todo_approvals";
DROP TABLE "todo_sources";
DROP TABLE "todo_notes";
DROP TABLE "todos";
DROP FUNCTION todos_merge_delete_refusal();
DROP FUNCTION todos_merge_refusal();
