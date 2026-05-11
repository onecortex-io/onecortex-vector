-- Enables BM25 full-text search via Timescale pg_textsearch extension.
-- PREREQUISITE: The PostgreSQL server must have been started with:
--   shared_preload_libraries = 'pg_textsearch'
-- This is already set in the root docker-compose.yml.
-- NOTE: pg_textsearch extension is created by postgres-init container.

-- NOTE: The bm25_enabled column already exists in the indexes table from migration 0002.
-- No ALTER TABLE needed here — it was added in Phase 0 so the schema is forward-compatible.
