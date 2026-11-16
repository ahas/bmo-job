-- A store of schema version 7 brought to version 8: a question keeps who it is put to, the wait it was asked with,
-- who put it to a person in the supervisor's place, and the asker's reference to the notice it was told with. Every question standing then is a person's, asked with the
-- wait it has between its asking and its due time, or a minute where that is not known.
ALTER TABLE questions ADD COLUMN addressee TEXT NOT NULL DEFAULT 'person';
ALTER TABLE questions ADD COLUMN wait_ms INTEGER NOT NULL DEFAULT 60000;
ALTER TABLE questions ADD COLUMN readdressed_by TEXT;
ALTER TABLE questions ADD COLUMN readdressed_at INTEGER;
ALTER TABLE questions ADD COLUMN told_as TEXT;
UPDATE questions SET wait_ms = due_at - created_at WHERE due_at IS NOT NULL AND due_at > created_at;
