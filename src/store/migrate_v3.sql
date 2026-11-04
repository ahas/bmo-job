-- A store of schema version 3 brought to version 4: whether a question's asker tells of it itself, none of those
-- standing having done so, and the last reply its asker was handed.
ALTER TABLE questions ADD COLUMN told_by_asker INTEGER NOT NULL DEFAULT 0;
ALTER TABLE questions ADD COLUMN heard_through INTEGER;
