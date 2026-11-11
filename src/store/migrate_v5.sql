-- A store of schema version 5 brought to version 6: an item carries no flag of work done by several at once.
ALTER TABLE items DROP COLUMN cowork;
