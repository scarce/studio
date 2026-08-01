-- Demand ledger (M1). One row per captured catalog miss.
--
-- Projection caveat, on the record: RFQ capture is currently API-authored —
-- there is no substrate event behind these rows yet, so this is the one
-- table the M3 replay test cannot rebuild from relay+RPC alone. Raised with
-- archy at the M1 boundary; expected resolution is mirroring demand records
-- onto a studio Buzz channel so the invariant (ARCHITECTURE.md §1) holds.
CREATE TABLE rfqs (
    id            TEXT PRIMARY KEY,
    query         TEXT NOT NULL,
    product       TEXT,
    monetization  TEXT,
    competition   TEXT NOT NULL DEFAULT '[]', -- JSON array of strings
    budget_amount INTEGER,                    -- minor units; NULL = no signal
    budget_mint   TEXT,
    buyer_npub    TEXT NOT NULL,
    created_at    TEXT NOT NULL               -- RFC 3339, UTC
) STRICT;

CREATE INDEX idx_rfqs_created_at ON rfqs (created_at);
