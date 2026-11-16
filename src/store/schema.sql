-- The tables the service keeps (docs/schema.md). Every key is the database's own number, never handed out
-- twice; every time is epoch milliseconds; lists and maps are JSON text.

CREATE TABLE items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL,
  brief TEXT NOT NULL,
  state TEXT NOT NULL,
  queue_name TEXT NOT NULL,
  position INTEGER NOT NULL,
  priority INTEGER NOT NULL,
  assignee_name TEXT,
  requester_name TEXT,
  scope TEXT,
  labels TEXT NOT NULL,
  approvals TEXT NOT NULL,
  merged_into INTEGER,
  raised_by INTEGER,
  lease_id INTEGER,
  created_at INTEGER NOT NULL,
  changed_at INTEGER NOT NULL,
  ended_at INTEGER
);
CREATE INDEX items_by_queue ON items (queue_name, state, priority DESC, position, id);
CREATE INDEX items_by_scope ON items (scope);
CREATE INDEX items_by_raised_by ON items (raised_by);

CREATE TABLE notes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL,
  text TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  labels TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX notes_by_item ON notes (item_id, id);

CREATE TABLE reports (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  labels TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX reports_by_item ON reports (item_id, id);

CREATE TABLE reviews (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL,
  ref TEXT,
  stage INTEGER,
  round INTEGER NOT NULL,
  kind TEXT NOT NULL,
  body TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  labels TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX reviews_by_item ON reviews (item_id, id);
CREATE INDEX reviews_by_kind ON reviews (item_id, kind);

CREATE TABLE work_orders (
  item_id INTEGER PRIMARY KEY,
  purpose TEXT NOT NULL,
  directive TEXT NOT NULL,
  steps TEXT NOT NULL,
  state TEXT NOT NULL,
  changed_at INTEGER NOT NULL
);

CREATE TABLE tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL,
  stage INTEGER NOT NULL,
  title TEXT NOT NULL,
  state TEXT NOT NULL,
  assignee_name TEXT,
  changed_at INTEGER NOT NULL
);
CREATE INDEX tasks_by_item ON tasks (item_id, stage, id);

CREATE TABLE queues (
  name TEXT PRIMARY KEY,
  state TEXT NOT NULL,
  holds TEXT NOT NULL,
  changed_at INTEGER NOT NULL
);

CREATE TABLE leases (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  queue_name TEXT NOT NULL,
  item_id INTEGER NOT NULL,
  holder_name TEXT NOT NULL,
  caller_name TEXT,
  ttl_ms INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  ended_at INTEGER,
  frozen TEXT
);
CREATE INDEX leases_standing ON leases (ended_at, expires_at);
CREATE INDEX leases_by_item ON leases (item_id);

CREATE TABLE questions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER,
  asker_name TEXT NOT NULL,
  scope TEXT,
  text TEXT NOT NULL,
  choices TEXT NOT NULL,
  recommendation TEXT,
  state TEXT NOT NULL,
  due_at INTEGER,
  remaining_ms INTEGER,
  due_told INTEGER NOT NULL,
  told_by_asker INTEGER NOT NULL DEFAULT 0,
  heard_through INTEGER,
  created_at INTEGER NOT NULL,
  addressee TEXT NOT NULL DEFAULT 'person',
  wait_ms INTEGER NOT NULL DEFAULT 60000,
  readdressed_by TEXT,
  readdressed_at INTEGER,
  told_as TEXT
);
CREATE INDEX questions_by_item ON questions (item_id);
CREATE INDEX questions_by_scope ON questions (scope);

CREATE TABLE replies (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  question_id INTEGER NOT NULL,
  choice TEXT,
  text TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  settles INTEGER NOT NULL,
  source TEXT NOT NULL,
  why TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX replies_by_question ON replies (question_id, id);

CREATE TABLE subscriptions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  holder_name TEXT NOT NULL,
  kinds TEXT NOT NULL,
  queue_names TEXT NOT NULL,
  scope TEXT,
  item_ids TEXT NOT NULL,
  url TEXT,
  secret TEXT,
  state TEXT NOT NULL,
  ttl_ms INTEGER,
  expires_at INTEGER,
  failure_count INTEGER NOT NULL,
  delivered_event_id INTEGER,
  start_event_id INTEGER NOT NULL,
  labels TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  changed_at INTEGER NOT NULL,
  ended_at INTEGER
);

CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  at INTEGER NOT NULL,
  data TEXT NOT NULL,
  queue_name TEXT,
  scope TEXT,
  item_id INTEGER
);
CREATE INDEX events_by_time ON events (at);
