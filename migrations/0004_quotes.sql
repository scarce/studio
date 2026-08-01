-- Quote ledger (M2). At most one quote per RFQ in v0 (the PLAN.md §4 routes
-- are singular: POST/GET /rfqs/{id}/quote); re-quoting after a lapse is an
-- open pricing question (ARCHITECTURE.md §8.3) and deliberately unsupported
-- until answered.
--
-- Projection caveat, same standing as rfqs (see 0002): quotes are
-- API-authored until the orchestrator mirrors them to the studio ops channel
-- (M3, decided at the M1 boundary). Structured columns carry what the sweep
-- and reads filter on; the negotiated payload (milestones, splits, gate
-- policy) stays canonical JSON so the projection round-trips exactly.
CREATE TABLE quotes (
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
    status               TEXT NOT NULL CHECK (status IN ('QUOTED', 'LAPSED')),
    created_at           TEXT NOT NULL, -- RFC 3339, UTC
    lapsed_at            TEXT
) STRICT;

CREATE INDEX idx_quotes_status_expires_at ON quotes (status, expires_at);
