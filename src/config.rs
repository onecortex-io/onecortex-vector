/// Application configuration loaded from environment variables.
/// All variables use the ONECORTEX_VECTOR_ prefix.
/// See docs/implementation/00-reference.md §12 for the full variable list.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// ONECORTEX_VECTOR_DATABASE_URL — required, no default
    pub database_url: String,

    /// ONECORTEX_VECTOR_API_PORT — default 8080
    pub api_port: u16,

    /// ONECORTEX_VECTOR_ADMIN_PORT — default 9090
    pub admin_port: u16,

    /// ONECORTEX_VECTOR_MAX_CONNS — default 50
    pub max_conns: u32,

    /// ONECORTEX_VECTOR_DEFAULT_DISKANN_NEIGHBORS — default 50
    /// Used as num_neighbors when creating new DiskANN indexes.
    /// See 00-reference.md §6.
    pub default_diskann_neighbors: u32,

    /// ONECORTEX_VECTOR_DEFAULT_DISKANN_SEARCH_LIST — default 100
    pub default_diskann_search_list: u32,

    /// ONECORTEX_VECTOR_ENABLE_RLS — default false
    /// Enables PostgreSQL Row-Level Security for namespace isolation.
    /// Requires SET LOCAL pattern — see 00-reference.md §9.
    #[allow(dead_code)]
    pub enable_rls: bool,

    /// ONECORTEX_VECTOR_LOG_LEVEL — default "info"
    pub log_level: String,

    /// ONECORTEX_VECTOR_API_HOST — default "localhost"
    /// Returned in the `host` field of index descriptors.
    pub api_host: String,

    /// Reranking backend. Values: "none" | "cohere" | "voyage" | "jina" | "pinecone" | "cross-encoder".
    /// Default: "none" (reranking disabled — no latency overhead).
    pub rerank_backend: String,

    // ── Cohere ─────────────────────────────────────────────────────────────────
    /// Required when rerank_backend = "cohere".
    pub rerank_cohere_api_key: Option<String>,
    /// Default: "rerank-v3.5" (multilingual, high quality).
    /// Also available: "rerank-v4.0-pro", "rerank-english-v3.0".
    pub rerank_cohere_model: String,

    // ── Voyage AI ──────────────────────────────────────────────────────────────
    /// Required when rerank_backend = "voyage".
    pub rerank_voyage_api_key: Option<String>,
    /// Default: "rerank-2.5". Also available: "rerank-2.5-lite", "rerank-2", "rerank-lite-1".
    pub rerank_voyage_model: String,

    // ── Jina AI ────────────────────────────────────────────────────────────────
    /// Required when rerank_backend = "jina".
    pub rerank_jina_api_key: Option<String>,
    /// Default: "jina-reranker-v2-base-multilingual". Also: "jina-reranker-v1-base-en".
    pub rerank_jina_model: String,

    // ── Pinecone Inference ─────────────────────────────────────────────────────
    /// Required when rerank_backend = "pinecone".
    pub rerank_pinecone_api_key: Option<String>,
    /// Default: "pinecone-rerank-v0". Also hosted: "bge-reranker-v2-m3", "cohere-rerank-3.5".
    pub rerank_pinecone_model: String,

    // ── Self-hosted cross-encoder (TEI) ───────────────────────────────────────
    /// Required when rerank_backend = "cross-encoder".
    /// Example: "http://cross-encoder:8080"
    pub rerank_cross_encoder_url: Option<String>,

    // ── Shared HTTP behavior ──────────────────────────────────────────────────
    /// Timeout in seconds for reranker HTTP calls. Default: 30.
    pub rerank_http_timeout_secs: u64,
    /// Max retry attempts on 429 (rate limit). Default: 3. Applies to cloud backends only.
    pub rerank_max_retries: u32,
}

impl AppConfig {
    /// Load configuration from environment variables.
    /// Call dotenvy::dotenv().ok() before this to load a .env file in development.
    pub fn from_env() -> Result<Self, String> {
        Ok(AppConfig {
            database_url: required_env("ONECORTEX_VECTOR_DATABASE_URL")?,
            api_port: env_parse("ONECORTEX_VECTOR_API_PORT", 8080)?,
            admin_port: env_parse("ONECORTEX_VECTOR_ADMIN_PORT", 9090)?,
            max_conns: env_parse("ONECORTEX_VECTOR_MAX_CONNS", 50)?,
            default_diskann_neighbors: env_parse("ONECORTEX_VECTOR_DEFAULT_DISKANN_NEIGHBORS", 50)?,
            default_diskann_search_list: env_parse(
                "ONECORTEX_VECTOR_DEFAULT_DISKANN_SEARCH_LIST",
                100,
            )?,
            enable_rls: env_parse("ONECORTEX_VECTOR_ENABLE_RLS", false)?,
            log_level: std::env::var("ONECORTEX_VECTOR_LOG_LEVEL")
                .unwrap_or_else(|_| "info".into()),
            api_host: std::env::var("ONECORTEX_VECTOR_API_HOST")
                .unwrap_or_else(|_| "localhost".into()),
            rerank_backend: std::env::var("ONECORTEX_VECTOR_RERANK_BACKEND")
                .unwrap_or_else(|_| "none".into()),

            rerank_cohere_api_key: std::env::var("ONECORTEX_VECTOR_RERANK_COHERE_API_KEY").ok(),
            rerank_cohere_model: std::env::var("ONECORTEX_VECTOR_RERANK_COHERE_MODEL")
                .unwrap_or_else(|_| "rerank-v3.5".into()),

            rerank_voyage_api_key: std::env::var("ONECORTEX_VECTOR_RERANK_VOYAGE_API_KEY").ok(),
            rerank_voyage_model: std::env::var("ONECORTEX_VECTOR_RERANK_VOYAGE_MODEL")
                .unwrap_or_else(|_| "rerank-2.5".into()),

            rerank_jina_api_key: std::env::var("ONECORTEX_VECTOR_RERANK_JINA_API_KEY").ok(),
            rerank_jina_model: std::env::var("ONECORTEX_VECTOR_RERANK_JINA_MODEL")
                .unwrap_or_else(|_| "jina-reranker-v2-base-multilingual".into()),

            rerank_pinecone_api_key: std::env::var("ONECORTEX_VECTOR_RERANK_PINECONE_API_KEY").ok(),
            rerank_pinecone_model: std::env::var("ONECORTEX_VECTOR_RERANK_PINECONE_MODEL")
                .unwrap_or_else(|_| "pinecone-rerank-v0".into()),

            rerank_cross_encoder_url: std::env::var("ONECORTEX_VECTOR_RERANK_CROSS_ENCODER_URL")
                .ok(),

            rerank_http_timeout_secs: std::env::var("ONECORTEX_VECTOR_RERANK_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            rerank_max_retries: std::env::var("ONECORTEX_VECTOR_RERANK_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
        })
    }
}

fn required_env(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("Required environment variable {key} is not set"))
}

fn env_parse<T: std::str::FromStr + ToString>(key: &str, default: T) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(val) => val
            .parse::<T>()
            .map_err(|e| format!("Invalid value for {key}: {e}")),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_defaults() {
        // Clear any env vars that might be set from .env
        std::env::set_var("ONECORTEX_VECTOR_DATABASE_URL", "postgres://test");
        std::env::remove_var("ONECORTEX_VECTOR_API_PORT");
        std::env::remove_var("ONECORTEX_VECTOR_MAX_CONNS");
        std::env::remove_var("ONECORTEX_VECTOR_DEFAULT_DISKANN_NEIGHBORS");
        std::env::remove_var("ONECORTEX_VECTOR_ENABLE_RLS");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_BACKEND");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_COHERE_API_KEY");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_COHERE_MODEL");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_VOYAGE_API_KEY");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_VOYAGE_MODEL");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_JINA_API_KEY");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_JINA_MODEL");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_PINECONE_API_KEY");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_PINECONE_MODEL");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_CROSS_ENCODER_URL");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_HTTP_TIMEOUT_SECS");
        std::env::remove_var("ONECORTEX_VECTOR_RERANK_MAX_RETRIES");

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.api_port, 8080);
        assert_eq!(config.admin_port, 9090);
        assert_eq!(config.max_conns, 50);
        assert_eq!(config.default_diskann_neighbors, 50); // NOT 64 — see 00-reference.md §6
        assert_eq!(config.default_diskann_search_list, 100);
        assert!(!config.enable_rls);
        assert_eq!(config.rerank_backend, "none");
        assert_eq!(config.rerank_cohere_model, "rerank-v3.5");
        assert_eq!(config.rerank_voyage_model, "rerank-2.5");
        assert_eq!(
            config.rerank_jina_model,
            "jina-reranker-v2-base-multilingual"
        );
        assert_eq!(config.rerank_pinecone_model, "pinecone-rerank-v0");
        assert!(config.rerank_cross_encoder_url.is_none());
        assert_eq!(config.rerank_http_timeout_secs, 30);
        assert_eq!(config.rerank_max_retries, 3);
    }
}
