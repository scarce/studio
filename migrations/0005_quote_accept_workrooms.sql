-- Buyer acceptance on the quote row (stands in for FUNDED while payments are
-- stubbed — PLAN.md §6 override path) and the workroom projection. The
-- workroom row carries the Buzz channel-create event id: the evidence for
-- the FUNDED → WORKROOM_ACTIVE transition (ARCHITECTURE.md §evidence table).
--
-- The quotes table is rebuilt rather than altered: SQLite cannot widen the
-- status CHECK in place, and the table is a projection — rebuildable by
-- design, so the copy is cheap and safe.

CREATE TABLE quotes_new (
    rfq_id               TEXT PRIMARY KEY REFERENCES rfqs (id),
    id                   TEXT NOT NULL UNIQUE,
    price_amount         INTEGER NOT NULL,
    price_mint           TEXT NOT NULL,
    milestones           TEXT NOT NULL, -- JSON array of milestone specs
    timeline             TEXT NOT NULL,
    payout_destination   TEXT NOT NULL, -- JSON, kind-tagged
    grace_seconds        INTEGER NOT NULL,
    idle_timeout_seconds INTEGER NOT NULL,
    gate_policy          TEXT NOT NULL, -- JSON, hash-committed via policy_hash
    policy_hash          TEXT NOT NULL, -- sha256 hex of canonical gate_policy
    expires_at           TEXT NOT NULL, -- RFC 3339, UTC
    status               TEXT NOT NULL CHECK (status IN ('QUOTED', 'LAPSED', 'ACCEPTED')),
    created_at           TEXT NOT NULL, -- RFC 3339, UTC
    lapsed_at            TEXT,
    accepted_at          TEXT           -- RFC 3339, UTC; set exactly once
) STRICT;

INSERT INTO quotes_new (rfq_id, id, price_amount, price_mint, milestones,
                        timeline, payout_destination, grace_seconds,
                        idle_timeout_seconds, gate_policy, policy_hash,
                        expires_at, status, created_at, lapsed_at)
SELECT rfq_id, id, price_amount, price_mint, milestones,
       timeline, payout_destination, grace_seconds,
       idle_timeout_seconds, gate_policy, policy_hash,
       expires_at, status, created_at, lapsed_at
FROM quotes;

DROP TABLE quotes;
ALTER TABLE quotes_new RENAME TO quotes;
CREATE INDEX idx_quotes_status_expires_at ON quotes (status, expires_at);

CREATE TABLE workrooms (
    rfq_id          TEXT PRIMARY KEY REFERENCES rfqs (id),
    channel_id      TEXT NOT NULL,
    create_event_id TEXT NOT NULL,
    created_at      TEXT NOT NULL -- RFC 3339, UTC
) STRICT;
