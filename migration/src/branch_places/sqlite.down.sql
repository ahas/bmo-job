-- Gives the service its own list of branch places again on SQLite (docs/schema.md).

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
