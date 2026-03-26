-- Full-text search index for hybrid retrieval (vector + keyword)
-- 'simple' config tokenizes on whitespace/punctuation — works for CJK + English mixed content
-- without requiring external language extensions.
CREATE INDEX IF NOT EXISTS idx_claims_fulltext
    ON epistemic_claims
    USING gin(to_tsvector('simple', content));
