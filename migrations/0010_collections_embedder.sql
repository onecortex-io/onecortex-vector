-- F1: Server-side embeddings.
-- A collection may bind a default embedder (backend + model + inputType)
-- so callers can send `text` instead of `values` on upsert and query.
-- Existing collections (with NULL) keep the values-only behaviour.
ALTER TABLE _onecortex_vector.collections
    ADD COLUMN embedder_config JSONB;
