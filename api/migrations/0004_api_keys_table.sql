-- API keys are stored as SHA-256 hashes, never in plaintext.
-- allowed_namespaces = NULL means the key has access to all namespaces.
-- allowed_namespaces = '{"ns1","ns2"}' restricts to those namespaces only.
CREATE TABLE _onecortex_vector.api_keys (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    -- SHA-256 hex digest of the raw API key
    key_hash            TEXT        NOT NULL UNIQUE,
    -- Human-readable label (optional, for admin display)
    name                TEXT,
    -- NULL = unrestricted; array of namespace strings = restricted
    allowed_namespaces  TEXT[]      DEFAULT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Set when key is revoked; NULL = active
    revoked_at          TIMESTAMPTZ
);

CREATE INDEX idx_api_keys_hash ON _onecortex_vector.api_keys (key_hash) WHERE revoked_at IS NULL;
