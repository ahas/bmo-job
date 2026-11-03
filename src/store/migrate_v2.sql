-- A store of schema version 2 brought to version 3: the item an item was raised from, filled from its
-- `raised-by` label where that label is the key of another standing item, and the caller who holds a lease, none
-- for a lease standing already.
ALTER TABLE items ADD COLUMN raised_by INTEGER;
CREATE INDEX items_by_raised_by ON items (raised_by);
UPDATE items SET raised_by = CAST(json_extract(labels, '$."raised-by"') AS INTEGER)
WHERE json_valid(labels)
  AND CAST(CAST(json_extract(labels, '$."raised-by"') AS INTEGER) AS TEXT) = json_extract(labels, '$."raised-by"')
  AND EXISTS (
    SELECT 1 FROM items AS parent
    WHERE parent.id = CAST(json_extract(items.labels, '$."raised-by"') AS INTEGER) AND parent.id != items.id
  );
ALTER TABLE leases ADD COLUMN caller_name TEXT;
