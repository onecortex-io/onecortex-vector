# Onecortex Vector

High performance (⚡) vector database built in Rust (🦀) on PostgreSQL. Simple Pinecone like API with dense vector and hybrid search (🔎), plus rich metadata filtering, namespace support, and advanced retrieval features.

## Features

- **Dense ANN search** — cosine, euclidean, and dot product similarity via StreamingDiskANN
- **Hybrid search** — combine dense vector similarity with BM25 text search using Reciprocal Rank Fusion (RRF)
- **Reranking** — plug in Cohere, Voyage, Jina, Pinecone Inference, or a self-hosted cross-encoder to rerank results with natural language queries
- **Metadata filtering** — rich query DSL with `$eq`, `$ne`, `$gt`, `$lt`, `$in`, `$nin`, `$and`, `$or` operators
- **Namespaces** — isolate data within an index using scoped operations by namespace
- **Batch queries** — fan out up to 10 queries in a single request with concurrent execution
- **Scroll & sample** — paginate over all vectors or draw a random sample, both with full filter support
- **Score threshold** — filter results by minimum similarity score, applied after reranking
- **GroupBy** — group nearest-neighbor results by any metadata field to avoid same-source clustering
- **Recommendations** — find similar items from positive/negative example IDs without supplying a query vector
- **Index aliases** — point a named alias at any index for zero-downtime swaps and A/B testing
- **Self-hosted** — runs on your own PostgreSQL instance, no vendor lock-in

## Quick Start

### 1. Start PostgreSQL

```bash
docker compose -f deploy/docker-compose.yml up -d postgres
```

This starts PostgreSQL 18 with pgvector, pgvectorscale, and pg_textsearch pre-installed.

### 2. Start the API server

```bash
cp .env.example .env
cargo run
```

The server applies migrations automatically on startup.

- **Public API:** http://localhost:8080
- **Admin API:** http://localhost:9090

### 3. Create an API key

```bash
curl -s -X POST http://localhost:9090/admin/api_keys \
  -H "Content-Type: application/json" \
  -d '{"name":"dev-key"}' | jq .
```

### 4. Create an index and query vectors

```bash
API_KEY="<key-from-step-3>"

# Create an index
curl -s -X POST http://localhost:8080/indexes \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-index","dimension":3,"metric":"cosine"}'

# Upsert vectors
curl -s -X POST http://localhost:8080/indexes/my-index/vectors/upsert \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "vectors": [
      {"id":"vec-1","values":[0.1,0.2,0.3],"metadata":{"genre":"sci-fi"}},
      {"id":"vec-2","values":[0.4,0.5,0.6],"metadata":{"genre":"fantasy"}}
    ]
  }'

# Query
curl -s -X POST http://localhost:8080/indexes/my-index/query \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"vector":[0.1,0.2,0.3],"topK":5,"includeMetadata":true}'
```

## API Endpoints

All data-plane endpoints require an `Api-Key` header.

### Control Plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/indexes` | Create an index |
| GET | `/indexes` | List all indexes |
| GET | `/indexes/:name` | Describe an index |
| PATCH | `/indexes/:name` | Configure an index (tags) |
| DELETE | `/indexes/:name` | Delete an index |
| POST | `/indexes/:name/describe_index_stats` | Get index statistics |

### Data Plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/indexes/:name/vectors/upsert` | Upsert vectors |
| POST | `/indexes/:name/vectors/fetch` | Fetch vectors by ID |
| POST | `/indexes/:name/vectors/fetch_by_metadata` | Fetch vectors by metadata filter |
| POST | `/indexes/:name/vectors/delete` | Delete vectors |
| POST | `/indexes/:name/vectors/update` | Update a vector's metadata |
| GET | `/indexes/:name/vectors/list` | List vector IDs (IDs only) |
| POST | `/indexes/:name/vectors/scroll` | Scroll all vectors with cursor pagination |
| POST | `/indexes/:name/sample` | Random sample of vectors |
| POST | `/indexes/:name/query` | Query nearest neighbors |
| POST | `/indexes/:name/query/hybrid` | Hybrid dense + BM25 query |
| POST | `/indexes/:name/query/batch` | Run up to 10 queries concurrently |
| POST | `/indexes/:name/recommend` | Recommend by positive/negative example IDs |

### Namespaces

| Method | Path | Description |
|--------|------|-------------|
| GET | `/indexes/:name/namespaces` | List namespaces |
| POST | `/indexes/:name/namespaces` | Create a namespace |
| GET | `/indexes/:name/namespaces/:ns` | Describe a namespace |
| DELETE | `/indexes/:name/namespaces/:ns` | Delete a namespace |

### Aliases

| Method | Path | Description |
|--------|------|-------------|
| POST | `/aliases` | Create or update an alias |
| GET | `/aliases` | List all aliases |
| GET | `/aliases/:alias` | Describe an alias |
| DELETE | `/aliases/:alias` | Delete an alias |

### Health & Admin

| Method | Path | Port | Description |
|--------|------|------|-------------|
| GET | `/health` | 8080 | Health check |
| GET | `/ready` | 8080 | Readiness check |
| GET | `/version` | 8080 | Server version |
| GET | `/metrics` | 9090 | Prometheus metrics |
| POST | `/admin/api_keys` | 9090 | Create API key |
| DELETE | `/admin/api_keys/:id` | 9090 | Revoke API key |
| POST | `/admin/indexes/:name/reindex` | 9090 | Rebuild DiskANN index |
| POST | `/admin/indexes/:name/vacuum` | 9090 | Vacuum an index |
| GET | `/admin/config` | 9090 | Dump current config |

## Hybrid Search

Create a BM25-enabled index, upsert vectors with text, and query with both vector and keyword:

```bash
# Create with BM25 enabled
curl -s -X POST http://localhost:8080/indexes \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"hybrid-index","dimension":3,"metric":"cosine","bm25_enabled":true}'

# Upsert with text for BM25
curl -s -X POST http://localhost:8080/indexes/hybrid-index/vectors/upsert \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "vectors": [
      {"id":"v1","values":[1,0,0],"text":"machine learning basics"},
      {"id":"v2","values":[0,1,0],"text":"cooking recipes"}
    ]
  }'

# Hybrid query (dense + BM25, fused with RRF)
curl -s -X POST http://localhost:8080/indexes/hybrid-index/query/hybrid \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"vector":[1,0,0],"text":"machine learning","topK":5}'
```

## Reranking

Add a `rerank` object to any query to rerank results using a natural language query:

```bash
curl -s -X POST http://localhost:8080/indexes/my-index/query \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "vector":[0.1,0.2,0.3],
    "topK":10,
    "rerank":{"query":"machine learning fundamentals","topN":3,"rankField":"text"}
  }'
```

Supported reranking backends (All Optional and configured via `ONECORTEX_VECTOR_RERANK_BACKEND`):

| Backend | Value | Notes |
|---------|-------|-------|
| None | `none` | Default, no reranking |
| Cohere | `cohere` | Requires `ONECORTEX_VECTOR_RERANK_COHERE_API_KEY` |
| Voyage AI | `voyage` | Requires `ONECORTEX_VECTOR_RERANK_VOYAGE_API_KEY` |
| Jina AI | `jina` | Requires `ONECORTEX_VECTOR_RERANK_JINA_API_KEY` |
| Pinecone Inference | `pinecone` | Requires `ONECORTEX_VECTOR_RERANK_PINECONE_API_KEY` |
| Self-hosted cross-encoder | `cross-encoder` | Requires `ONECORTEX_VECTOR_RERANK_CROSS_ENCODER_URL` |

To start the optional self-hosted cross-encoder:

```bash
docker compose -f deploy/docker-compose.yml --profile reranking up -d
```

## Advanced Query Features

### Score Threshold

Drop results below a minimum similarity score (applied after reranking):

```json
{ "vector": [...], "topK": 10, "scoreThreshold": 0.75 }
```

### Batch Query

Send up to 10 queries in one round-trip; results are returned in the same order:

```bash
curl -s -X POST http://localhost:8080/indexes/my-index/query/batch \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"queries":[{"vector":[1,0,0],"topK":5},{"vector":[0,1,0],"topK":3}]}'
```

### GroupBy

Group results by a metadata field to ensure diversity across sources:

```json
{ "vector": [...], "topK": 50, "groupBy": { "field": "document_id", "limit": 5, "groupSize": 2 } }
```

### Recommendations

Find similar items from example IDs — no query vector needed:

```bash
curl -s -X POST http://localhost:8080/indexes/my-index/recommend \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"positiveIds":["vec-1","vec-2"],"negativeIds":["vec-9"],"topK":10}'
```

### Index Aliases

Aliases resolve transparently on every endpoint, enabling zero-downtime index swaps:

```bash
# Point "prod" at a new index atomically
curl -s -X POST http://localhost:8080/aliases \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"alias":"prod","indexName":"my-index-v2"}'
```

## Configuration

All environment variables use the `ONECORTEX_VECTOR_` prefix. Copy `.env.example` for the full list with documentation.

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | (required) | PostgreSQL connection string |
| `API_PORT` | `8080` | Public API port |
| `ADMIN_PORT` | `9090` | Admin/metrics port |
| `MAX_CONNS` | `50` | Max database pool connections |
| `LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |
| `RERANK_BACKEND` | `none` | Reranking backend |
| `ENABLE_RLS` | `false` | Enable row-level security for namespace isolation |

## SDKs

| Language | Package | Repository |
|----------|---------|------------|
| Python | `onecortex` | [onecortex-python-client](https://github.com/onecortex-io/onecortex-python-client) |
| TypeScript | `@onecortex/sdk` | [onecortex-typescript-client](https://github.com/onecortex-io/onecortex-typescript-client) |

```python
from onecortex import Onecortex

client = Onecortex(url="http://localhost:8080", api_key="your-api-key")
idx = client.vector.index("my-index")
results = await idx.query(vector=[0.1, 0.2, 0.3], top_k=5)
```

```typescript
import { Onecortex } from '@onecortex/sdk';

const client = new Onecortex({ url: 'http://localhost:8080', apiKey: 'your-api-key' });
const idx = client.vector.index('my-index');
const results = await idx.query({ vector: [0.1, 0.2, 0.3], topK: 5 });
```

## Architecture

```
Client → REST API (axum) → PostgreSQL 18
                              ├── pgvector (vector storage + operators)
                              ├── pgvectorscale (StreamingDiskANN indexing)
                              └── pg_textsearch (BM25 full-text search)
```

Each user index gets its own PostgreSQL schema (`idx_<name>`), providing full isolation. Index metadata is tracked in the `_onecortex_vector` catalog schema. Migrations are managed by sqlx and applied automatically on server startup.

## Development

```bash
# Build
cargo build

# Run tests (requires PostgreSQL)
docker compose -f deploy/docker-compose.yml up -d postgres
cargo test

# Lint
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
