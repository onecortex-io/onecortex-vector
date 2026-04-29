# Architecture

```
Client
  │
  ▼
[ APISIX gateway — JWT validation ]   (production deployment only)
  │
  ▼
[ REST API — axum 0.7 ]
  │
  ▼
[ PostgreSQL 18 ]
  ├── pgvector         — vector storage + distance operators
  ├── pgvectorscale    — StreamingDiskANN indexing
  └── pg_textsearch    — BM25 full-text search
```

The service is a thin REST layer on top of PostgreSQL; there is no
separate index process, no message queue, and no metadata service to
operate. Backups, replication, point-in-time recovery, IAM, and
monitoring are whatever you already use for Postgres.

## Schemas

Two PostgreSQL schemas are used:

- **`_onecortex_vector`** — system catalog: `collections`, `aliases`,
  `collection_stats`. Onecortex Vector owns this schema.
- **`_onecortex`** — user data, one table per collection named
  `col_<uuid>`. The `_onecortex` schema is intentionally a shared
  namespace for user-facing data across all Onecortex services on the
  same database.

## Migrations

Migrations are managed by sqlx and applied automatically on server
startup. The migration history lives in `_onecortex_vector.\_sqlx_migrations`.

## Observability

- Every response carries an `X-Request-Id` header (UUID v4 if the
  client did not supply one).
- The same id appears in `error.details.requestId` on every error
  body and as `request_id` on every server log span.
- Prometheus metrics are exposed on the admin port at `/metrics`.

See the codebase's `CLAUDE.md` for the full set of internal technical
decisions (catalog naming, RLS pattern, distance scoring conventions,
etc.).
