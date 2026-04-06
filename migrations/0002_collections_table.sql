CREATE TABLE _onecortex_vector.collections (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name                TEXT        NOT NULL UNIQUE
                                    CHECK (char_length(name) BETWEEN 1 AND 45),
    -- Vector dimension: 1 to 20,000.
    dimension           INT         NOT NULL
                                    CHECK (dimension BETWEEN 1 AND 20000),
    metric              TEXT        NOT NULL
                                    CHECK (metric IN ('cosine', 'euclidean', 'dotproduct')),
    bm25_enabled        BOOLEAN     NOT NULL DEFAULT FALSE,
    status              TEXT        NOT NULL DEFAULT 'initializing'
                                    CHECK (status IN ('initializing', 'ready', 'deleting')),
    -- Deletion protection: when true, DELETE /collections/:name returns 403
    deletion_protected  BOOLEAN     NOT NULL DEFAULT FALSE,
    -- Arbitrary JSON tags (e.g. {"env": "prod"})
    tags                JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for fast name lookups (covered by UNIQUE but explicit for clarity)
CREATE INDEX idx_collections_name ON _onecortex_vector.collections (name);
CREATE INDEX idx_collections_status ON _onecortex_vector.collections (status);
