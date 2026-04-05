-- Composite btree expression indexes for memory_layer queries.
-- The GIN index from 007 helps @> containment but not ->>'field' = equality.

-- memory_layer + session_id composite for session-scoped queries
CREATE INDEX IF NOT EXISTS idx_claims_memory_layer_session
ON epistemic_claims ((metadata->>'memory_layer'), (metadata->>'session_id'))
WHERE metadata->>'memory_layer' IS NOT NULL;

-- memory_layer + created_at for GC (ORDER BY created_at for expiry)
CREATE INDEX IF NOT EXISTS idx_claims_memory_layer_created
ON epistemic_claims ((metadata->>'memory_layer'), created_at)
WHERE metadata->>'memory_layer' IS NOT NULL;

-- memory_layer + rehearsal_count for promotion queries
CREATE INDEX IF NOT EXISTS idx_claims_memory_layer_rehearsal
ON epistemic_claims ((metadata->>'memory_layer'), ((metadata->>'rehearsal_count')::bigint))
WHERE metadata->>'memory_layer' IS NOT NULL;
