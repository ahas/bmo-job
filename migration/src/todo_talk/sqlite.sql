-- The owner's talk about an item, on SQLite (docs/schema.md).

CREATE TABLE "todo_talk" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "todo_id" INTEGER NOT NULL,
  "speaker" TEXT NOT NULL,
  "text" TEXT NOT NULL,
  "said_at" INTEGER NOT NULL
);
CREATE INDEX "todo_talk_item" ON "todo_talk" (todo_id, id);
