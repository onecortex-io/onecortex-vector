-- Install pgvector and pgvectorscale.
-- CASCADE installs pgvector if not already present.
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS vectorscale CASCADE;

-- NOTE: pg_textsearch is NOT installed here.
-- It requires shared_preload_libraries=pg_textsearch (set in Docker Compose).
-- It will be installed in Phase 3 migration 0006_pg_textsearch.sql
-- after confirming the Docker Compose config is correct.

-- Internal catalog schema — Onecortex Vector system metadata (collections, api_keys, aliases, stats).
CREATE SCHEMA IF NOT EXISTS _onecortex_vector;

-- Shared user-data schema — collection record tables (col_<uuid>) live here.
-- Kept separate from the catalog so other Onecortex services on the same PostgreSQL
-- instance can store their user data under the same _onecortex namespace.
CREATE SCHEMA IF NOT EXISTS _onecortex;
