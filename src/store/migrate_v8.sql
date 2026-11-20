-- A store of schema version 8 brought to version 9: an item keeps why it is held and why notices of it are
-- switched off, each with the caller and the time, both null for every item standing then; and the values a label
-- key may take are kept, none published then.
ALTER TABLE items ADD COLUMN hold_reason TEXT;
ALTER TABLE items ADD COLUMN hold_caller_name TEXT;
ALTER TABLE items ADD COLUMN hold_at INTEGER;
ALTER TABLE items ADD COLUMN notices_off_reason TEXT;
ALTER TABLE items ADD COLUMN notices_off_caller_name TEXT;
ALTER TABLE items ADD COLUMN notices_off_at INTEGER;
CREATE TABLE label_values (
  key TEXT PRIMARY KEY,
  allowed TEXT NOT NULL,
  changed_at INTEGER NOT NULL
);
