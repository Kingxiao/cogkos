-- Remove fixed vector dimension constraint
-- Dimension is now determined by the embedding model at runtime.
-- pgvector allows unconstrained vector columns; HNSW index is created
-- at startup after probing the embedding service for actual dimension.

-- Drop existing HNSW index if any (it was bound to old dimension)
DROP INDEX IF EXISTS idx_claims_embedding_hnsw;

-- Alter column to unconstrained vector type
ALTER TABLE epistemic_claims ALTER COLUMN embedding TYPE vector;
