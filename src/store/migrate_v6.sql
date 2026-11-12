-- A store of schema version 6 brought to version 7: a lease keeps what of its item it holds fixed, nothing for
-- every lease standing then.
ALTER TABLE leases ADD COLUMN frozen TEXT;
