-- Per-account storage quota, in bytes. The default is 5 GiB.
-- A quota of 0 means "no limit".
ALTER TABLE users ADD COLUMN quota INTEGER NOT NULL DEFAULT 5368709120;
