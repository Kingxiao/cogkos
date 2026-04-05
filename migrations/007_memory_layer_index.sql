-- Index for memory_layer metadata field (working memory queries)
-- GIN index on metadata for flexible JSON queries
CREATE INDEX IF NOT EXISTS idx_claims_metadata_gin ON epistemic_claims USING gin (metadata);

-- Partial index for working memory (high-frequency queries)
CREATE INDEX IF NOT EXISTS idx_claims_memory_layer ON epistemic_claims ((metadata->>'memory_layer'))
WHERE metadata->>'memory_layer' IS NOT NULL;
