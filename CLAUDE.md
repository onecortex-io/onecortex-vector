# Onecortex Vector — Claude Code Context

## What is this project?

Onecortex Vector is an open-source, self-hosted vector database with a REST API. Built on PostgreSQL 18 with pgvector, pgvectorscale (StreamingDiskANN), and pg_textsearch (BM25). Written in Rust with axum 0.7 + sqlx 0.8 + tokio.

---

## Build & Test

```bash
cargo build
cargo test
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

---

## Key Technical Decisions

| Decision | Value |
|---|---|
| Catalog schema | `_onecortex_vector` — system metadata only (collections, collection_stats, api_keys, aliases) |
| User data schema | `_onecortex` — shared namespace for user-facing data across Onecortex services |
| Collection tables | `_onecortex.col_<uuid>` (one table per collection) |
| Column name for embeddings | `values` (not `embedding`) — matches Pinecone API field name |
| DiskANN `num_neighbors` | 50 (not 64) |
| RLS pattern | `set_config('app.current_namespace', $1, true)` inside `pool.begin()` — NEVER bare `SET` |
| Euclidean score | `1/(1+dist)` — known Pinecone deviation |
| SPARSEVEC | Not stored; `sparseValues` in upsert requests is silently dropped + WARN logged |
| BM25 score | `<@>` returns negative; always negate before ranking |

---

## Cross-Service Context

`onecortex-vector` is the primary upstream dependency of both SDK clients.

When you change a public REST endpoint here:
1. Run `/impact-analysis <description of change>` from the org root to identify all downstream files.
2. Propagate changes to both SDKs using `/sync-clients <description>`.
3. Update `docs/api-reference/` pages.

See `../CLAUDE.md` for the full org context, dependency graph, and available slash commands.
