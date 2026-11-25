-- From version 9: the item and the repository a task's step waits on, null on every task standing.
ALTER TABLE tasks ADD COLUMN after_item_id INTEGER;
ALTER TABLE tasks ADD COLUMN after_repository TEXT;
