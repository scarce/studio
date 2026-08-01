-- Baseline: proves the migration machinery end-to-end (M0).
-- Domain tables (rfqs, quotes, projects, transitions+evidence) arrive with
-- their milestones. Everything in this database is a rebuildable projection;
-- no table may ever hold a fact that is not derivable from the substrates.
CREATE TABLE store_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

INSERT INTO store_meta (key, value) VALUES ('purpose', 'projection');
