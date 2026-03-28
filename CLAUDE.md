# OneCortex Vector — Claude Code Context

## What is this project?

OneCortex Vector is an open-source, self-hosted vector database with a Pinecone-compatible REST API. It is built on PostgreSQL 18 with pgvector, pgvectorscale (StreamingDiskANN), and pg_textsearch (BM25).

The server is written in Rust using axum 0.7 + sqlx 0.8 + tokio. Official SDKs live in separate repos:
- **Python**: [`onecortex-python-client`](https://github.com/onecortex-io/onecortex-python-client) (`onecortex` on PyPI)
- **TypeScript**: [`onecortex-typescript-client`](https://github.com/onecortex-io/onecortex-typescript-client) (`@onecortex/sdk` on npm)

**Current status:** Phases 0-4 complete (foundation, REST API, hybrid search, reranking).

---

## Repository Layout

```
onecortex-vector/
├── Cargo.toml                    # Rust package config
├── Cargo.lock
├── .env.example                  # All environment variables with defaults
├── migrations/                   # sqlx migrations (numbered 0001–000N)
│   ├── 0001_catalog_schema.sql
│   ├── 0002_indexes_table.sql
│   ├── 0003_index_stats_table.sql
│   ├── 0004_api_keys_table.sql
│   └── 0006_pg_textsearch.sql
├── src/
│   ├── main.rs                   # Router, AppState, server startup
│   ├── lib.rs                    # Module declarations
│   ├── config.rs                 # AppConfig from ONECORTEX_VECTOR_* env vars
│   ├── error.rs                  # ApiError → HTTP response
│   ├── state.rs                  # AppState (pool, config, reranker)
│   ├── db/
│   │   ├── pool.rs               # PgPool creation + migration runner
│   │   └── lifecycle.rs          # DDL for creating/dropping index schemas
│   ├── handlers/                 # One file per resource group
│   │   ├── indexes.rs
│   │   ├── vectors.rs
│   │   ├── query.rs
│   │   ├── namespaces.rs
│   │   ├── health.rs
│   │   └── admin.rs
│   ├── middleware/
│   │   └── auth.rs
│   └── planner/
│       ├── filter_translator.rs  # Metadata filter DSL → SQL
│       ├── hybrid.rs             # Dense + BM25 fusion (RRF)
│       └── reranker.rs           # Cohere / cross-encoder / cloud reranking
├── tests/                        # Integration tests (require running Postgres)
│   ├── common/                   # Test server helpers
│   ├── auth_tests.rs
│   ├── control_plane.rs
│   ├── data_plane.rs
│   ├── foundation.rs
│   ├── hybrid_test.rs
│   ├── query_tests.rs
│   └── reranking_test.rs
└── deploy/
    ├── Dockerfile                # Builds PG18 + pgvector + pgvectorscale + pg_textsearch
    ├── docker-compose.yml        # Development environment
    ├── init-extensions.sql
    └── cross-encoder/            # Optional TEI reranker docs
```

---

## Development Environment

```bash
# Start PostgreSQL (with pgvector + pgvectorscale + pg_textsearch)
docker compose -f deploy/docker-compose.yml up -d postgres

# Run the API server (applies migrations automatically on startup)
cargo run

# Run tests (requires Postgres to be running)
cargo test

# Start with all services including PgBouncer (Phase 8+)
docker compose -f deploy/docker-compose.yml up -d

# Start with cross-encoder reranker (Phase 4, optional)
docker compose -f deploy/docker-compose.yml --profile reranking up -d
```

---

## Key Technical Decisions

| Decision | Value |
|---|---|
| Database image | Pre-built image with pg18 + pgvector + pgvectorscale + pg_textsearch |
| PostgreSQL minimum | 17+ (pg_textsearch requires 17+) |
| API framework | axum 0.7 |
| Database client | sqlx 0.8 (`runtime-tokio-rustls`, compile-time checked queries) |
| Catalog schema | `_onecortex_vector` |
| Index schemas | `idx_<sanitized-name>` (one per user index) |
| Column name for vectors | `values` (not `embedding`) — matches Pinecone API field name |
| DiskANN `num_neighbors` | 50 (not 64) |
| RLS pattern | `set_config('app.current_namespace', $1, true)` inside `pool.begin()` — NEVER bare `SET` |
| Euclidean score | `1/(1+dist)` — known Pinecone deviation |
| SPARSEVEC | Not stored; `sparseValues` in upsert requests is silently dropped + WARN logged |
| BM25 score | `<@>` returns negative; always negate before ranking |

---

## Environment Variables

All env vars use the `ONECORTEX_VECTOR_` prefix. See `.env.example` for the full list with defaults.

Key vars:

```
ONECORTEX_VECTOR_DATABASE_URL         postgres://user:pass@host:5432/onecortex
ONECORTEX_VECTOR_DATABASE_URL_DIRECT  (for migrations, bypasses PgBouncer)
ONECORTEX_VECTOR_API_PORT             8080 (public)
ONECORTEX_VECTOR_ADMIN_PORT           9090 (admin/metrics)
ONECORTEX_VECTOR_MAX_CONNS            50
ONECORTEX_VECTOR_DEFAULT_DISKANN_NEIGHBORS  50
ONECORTEX_VECTOR_LOG_LEVEL            info
ONECORTEX_VECTOR_ENABLE_RLS           false
ONECORTEX_VECTOR_RERANK_BACKEND       none | cohere | voyage | jina | pinecone | cross-encoder
```

---

## Common Commands

```bash
# Build
cargo build

# Run all tests
cargo test

# Run specific test file
cargo test --test hybrid_test

# Run with specific log level
ONECORTEX_VECTOR_LOG_LEVEL=debug cargo run

# Apply migrations manually
sqlx migrate run --source migrations --database-url $ONECORTEX_VECTOR_DATABASE_URL

# Seed a test API key
curl -X POST http://localhost:9090/admin/keys \
  -H "Content-Type: application/json" \
  -d '{"name":"dev-key"}'

# Check metrics
curl http://localhost:9090/metrics | grep onecortex
```
