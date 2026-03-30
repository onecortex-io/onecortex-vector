-- Aliases: symbolic names that resolve to an index.
-- Enables zero-downtime index swaps and A/B testing.
CREATE TABLE IF NOT EXISTS _onecortex_vector.aliases (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    alias      TEXT        NOT NULL UNIQUE
                           CHECK (char_length(alias) BETWEEN 1 AND 45),
    index_name TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX aliases_index_name_idx ON _onecortex_vector.aliases (index_name);
