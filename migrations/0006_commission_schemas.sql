-- Commission-flow draft-00 slice 1 (thread 6873a1ec).
--
-- rfqs: buyer identity becomes a union — the pay-side commission path
-- submits with the Solana key the buyer will fund the engagement with,
-- direct captures keep using the npub. At least one is required; the CHECK
-- keeps the invariant at the storage layer too. `brief` carries the intake
-- interview's structured output (JSON, validated upstream at capture).
--
-- quotes: `engagement_endpoint` is the 402-gated URL acceptance opens the
-- MPP session against — required on every quote from now on. Existing rows
-- (local dev data only; scarce.sh is not deployed yet) are backfilled with
-- the production path shape so NOT NULL can hold.
--
-- Both tables are rebuilt rather than altered: SQLite cannot relax NOT NULL
-- or add a CHECK in place, and both are projections — rebuildable by design.

CREATE TABLE rfqs_new (
    id                  TEXT PRIMARY KEY,
    query               TEXT NOT NULL,
    product             TEXT,
    monetization        TEXT,
    competition         TEXT NOT NULL DEFAULT '[]', -- JSON array of strings
    budget_amount       INTEGER,                    -- minor units; NULL = no signal
    budget_mint         TEXT,
    buyer_npub          TEXT,
    buyer_solana_pubkey TEXT,
    buyer_signature     TEXT,
    brief               TEXT,                       -- JSON commission brief
    created_at          TEXT NOT NULL,              -- RFC 3339, UTC
    CHECK (buyer_npub IS NOT NULL OR buyer_solana_pubkey IS NOT NULL)
) STRICT;

INSERT INTO rfqs_new (id, query, product, monetization, competition,
                      budget_amount, budget_mint, buyer_npub,
                      buyer_signature, created_at)
SELECT id, query, product, monetization, competition,
       budget_amount, budget_mint, buyer_npub,
       buyer_signature, created_at
FROM rfqs;

CREATE TABLE quotes_new (
    rfq_id               TEXT PRIMARY KEY REFERENCES rfqs_new (id),
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
    engagement_endpoint  TEXT NOT NULL, -- 402-gated URL acceptance funds against
    expires_at           TEXT NOT NULL, -- RFC 3339, UTC
    status               TEXT NOT NULL CHECK (status IN ('QUOTED', 'LAPSED', 'ACCEPTED')),
    created_at           TEXT NOT NULL, -- RFC 3339, UTC
    lapsed_at            TEXT,
    accepted_at          TEXT           -- RFC 3339, UTC; set exactly once
) STRICT;

INSERT INTO quotes_new (rfq_id, id, price_amount, price_mint, milestones,
                        timeline, payout_destination, grace_seconds,
                        idle_timeout_seconds, gate_policy, policy_hash,
                        engagement_endpoint, expires_at, status, created_at,
                        lapsed_at, accepted_at)
SELECT rfq_id, id, price_amount, price_mint, milestones,
       timeline, payout_destination, grace_seconds,
       idle_timeout_seconds, gate_policy, policy_hash,
       'https://scarce.sh/api/v1/engagements/' || rfq_id,
       expires_at, status, created_at, lapsed_at, accepted_at
FROM quotes;

DROP TABLE quotes;
DROP TABLE rfqs;
ALTER TABLE rfqs_new RENAME TO rfqs;
ALTER TABLE quotes_new RENAME TO quotes;

CREATE INDEX idx_rfqs_created_at ON rfqs (created_at);
CREATE INDEX idx_quotes_status_expires_at ON quotes (status, expires_at);
