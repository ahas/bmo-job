-- A store of schema version 1 brought to version 2: a question's recommendation and a reply's source and
-- reason. A reply already standing came from a person.
ALTER TABLE questions ADD COLUMN recommendation TEXT;
ALTER TABLE replies ADD COLUMN source TEXT NOT NULL DEFAULT 'person';
ALTER TABLE replies ADD COLUMN why TEXT;
