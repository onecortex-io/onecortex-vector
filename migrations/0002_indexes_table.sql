CREATE TABLE _onecortex_vector.indexes (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT        NOT NULL UNIQUE
                                    CHECK (char_length(name) BETWEEN 1 AND 45),
    -- Vector dimension: 1 to 20,000.
    -- Vector dimension: maximum 20,000.
    dimension           INT         NOT NULL
                                    CHECK (dimension BETWEEN 1 AND 20000),
    metric              TEXT        NOT NULL
                                    CHECK (metric IN ('cosine', 'euclidean', 'dotproduct')),
    bm25_enabled        BOOLEAN     NOT NULL DEFAULT FALSE,
    -- Schema name for the per-index vectors table, e.g. 'idx_550e8400e29b41d4'
    schema_name         TEXT        NOT NULL UNIQUE,
    status              TEXT        NOT NULL DEFAULT 'initializing'
                                    CHECK (status IN ('initializing', 'ready', 'deleting')),
    -- Deletion protection: when true, DELETE /indexes/:name returns 403
    deletion_protected  BOOLEAN     NOT NULL DEFAULT FALSE,
    -- Arbitrary JSON tags (e.g. {"env": "prod"})
    tags                JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for fast name lookups (covered by UNIQUE but explicit for clarity)
CREATE INDEX idx_indexes_name ON _onecortex_vector.indexes (name);
CREATE INDEX idx_indexes_status ON _onecortex_vector.indexes (status);
