-- Per-collection, per-namespace record counts.
-- Updated asynchronously after upsert/delete operations.
CREATE TABLE _onecortex_vector.collection_stats (
    collection_id   UUID        NOT NULL
                                REFERENCES _onecortex_vector.collections(id) ON DELETE CASCADE,
    namespace       TEXT        NOT NULL DEFAULT '',
    record_count    BIGINT      NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, namespace)
);
