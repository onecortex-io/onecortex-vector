-- Enables BM25 full-text search via Timescale pg_textsearch extension.
-- PREREQUISITE: The PostgreSQL server must have been started with:
--   shared_preload_libraries = 'pg_textsearch'
-- This is already set in deploy/docker-compose.yml (Task 0.1).
-- If pg_textsearch is missing from shared_preload_libraries, this CREATE
-- will succeed but BM25 indexes will fail to build at runtime.

CREATE EXTENSION IF NOT EXISTS pg_textsearch;

-- NOTE: The bm25_enabled column already exists in the indexes table from migration 0002.
-- No ALTER TABLE needed here — it was added in Phase 0 so the schema is forward-compatible.
