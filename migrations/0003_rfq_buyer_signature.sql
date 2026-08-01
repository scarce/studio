-- Reserved buyer-signature field (M2, per archy at the M1 boundary):
-- upgrade path (b) makes RFQs buyer-authored substrate. Recorded when
-- supplied, not yet verified — verification arrives with the orchestrator.
ALTER TABLE rfqs ADD COLUMN buyer_signature TEXT;
