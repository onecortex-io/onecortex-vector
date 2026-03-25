-- Per-index, per-namespace vector counts.
-- Updated asynchronously after upsert/delete operations.
CREATE TABLE _onecortex_vector.index_stats (
    index_id        UUID        NOT NULL
                                REFERENCES _onecortex_vector.indexes(id) ON DELETE CASCADE,
    namespace       TEXT        NOT NULL DEFAULT '',
    vector_count    BIGINT      NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (index_id, namespace)
);
