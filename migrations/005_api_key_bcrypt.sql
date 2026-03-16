-- Migration 005: Convert API key hashing from md5 to bcrypt (pgcrypto)
--
-- md5 hashes are unsalted and fast to brute-force. bcrypt uses per-row random
-- salts and a tunable work factor, making offline attacks infeasible.
--
-- Existing md5 hashes are one-way — we cannot recover the original keys to
-- re-hash them with bcrypt. Since this is a pre-production system, we truncate
-- the table. Any previously issued API keys must be regenerated.

TRUNCATE api_keys;
