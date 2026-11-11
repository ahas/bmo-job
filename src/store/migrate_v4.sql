-- A store of schema version 4 brought to version 5: the entries of each item's review.
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
