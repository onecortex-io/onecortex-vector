# Onecortex Vector

High-performance hybrid vector database built in Rust 🦀. Simple API with hybrid search (Combining Dense vector, Full-text, Geo radius) including rich metadata filtering, namespace support, and advanced retrieval/search features.

---

## The Problem

Building AI applications with Retreival Augmented Generation (RAG) pipelines, Semantic search or Recommendation engines is quite simple in a demo but it is surprisingly difficult in production at scale.

Developers typically hit five major roadblocks at this stage:

- **Search Precision Failures:** Pure vector search is great at semantic meaning but it fails at exact details match. For instace, it struggles to distinguish between similar identifiers (like `invoice #123456` vs. `invoice #123457`) and misses rare technical terms. This leads to LLM hallucinations and inaccurate outputs in RAG setups.

- **Architectural Complexity:** High quality retrieval needs "Hybrid Search," which usually means bolting together separate vector and full-text search systems. Managing this merge logic and re-ranking layers transforms a simple query into a complex distributed pipeline in AI application backends.

- **Fragmented Infrastructure:**
    * **Managed Services:** Managed services like Pinecone and Qdrant leads to high costs and proprietary vendor lock-in. Plus another third-party data store which can raise compliance, data privacy, and GDPR concerns.
    * **Self-Hosted Engines:** Like Milvus and Weaviate require orchestrating heavy, stateful components like etcd and message queues. Qdrant is open source but it forces you to manage complex stack outside your primary database infrastructure.

- **Unpredictable Scaling:** Combining scalar / metadata filters with hybrid vector search is a major bottleneck. Most systems suffer from unpredictable latency and degraded recall as datasets grow from thousands to millions of documents.

- **Consistency & Maintenance:** Most vector databases struggle with continuous updates and data shifts. Upgrading embedding models or handling real-time inserts often leads to silent precision drops with no easy way to roll back in Production.

**The result** of all this is that AI Engineering teams spend more time managing retrieval infrastructure than building actual products.

**Onecortex Vector** is built to change that: a self-hosted hybrid vector database built on familiar **PostgreSQL** to handle the full retrieval stack - dense vector search, hybrid BM25 fusion, geo radius search, re-ranking, and rich filtering without the overhead of a distributed system to run at scale.

---

## Features

- **Dense ANN search:** *cosine, euclidean, and dot product similarity search using StreamingDiskANN within pgvectorscale.*
- **Hybrid search:** *combine dense vector search with BM25 text search and geo radius search using Reciprocal Rank Fusion (RRF)*
- **Re-ranking:** *using Cohere, Voyage, Jina, Pinecone Inference, or a self-hosted cross-encoder to rerank results*
- **Metadata filtering:** *rich query DSL with `$eq`, `$ne`, `$gt`, `$lt`, `$in`, `$nin`, `$and`, `$or` operators*
- **Geo filtering:** *filter records by geographic proximity (`$geoRadius`) or bounding box (`$geoBBox`) using coordinates stored in metadata*
- **Datetime filtering:** *native ISO 8601 datetime comparisons using the standard `$gt`, `$gte`, `$lt`, `$lte` operators*
- **Array element matching:** *`$elemMatch` filters records where at least one element in a metadata array field matches a sub-filter object*
- **Namespaces:** *isolate data within a collection using scoped operations by namespace*
- **Batch queries:** *fan out up to 10 queries in a single request with concurrent execution*
- **Scroll & sample:** *paginate over all records or draw a random sample, both with full filter support*
- **Score threshold:** *filter results by minimum similarity score - applied after reranking results*
- **Group by:** *group nearest-neighbor results by any metadata field to avoid same-source clustering*
- **Faceted counts:** *aggregate counts of distinct metadata values for any field, optionally scoped to a filter*
- **Recommendations API:** *find similar items from positive/negative example IDs without supplying a query vector*
- **Collection aliases:** *point a named alias at any collection for zero-downtime swaps and A/B testing before releases*
- **Self-hosted:** *uses PostgreSQL database as its backend, no vendor lock-in*

---

## Quick Start

### 1. Start PostgreSQL

```bash
docker compose up -d postgres
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

### 4. Create a collection and query records

```bash
API_KEY="<key-from-step-3>"

# Create a collection
curl -s -X POST http://localhost:8080/v1/collections \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-collection","dimension":3,"metric":"cosine"}'

# Upsert records
curl -s -X POST http://localhost:8080/v1/collections/my-collection/records/upsert \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "records": [
      {"id":"rec-1","values":[0.1,0.2,0.3],"metadata":{"genre":"sci-fi"}},
      {"id":"rec-2","values":[0.4,0.5,0.6],"metadata":{"genre":"fantasy"}}
    ]
  }'

# Query
curl -s -X POST http://localhost:8080/v1/collections/my-collection/query \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"vector":[0.1,0.2,0.3],"topK":5,"includeMetadata":true}'
```

## API Endpoints

All data-plane endpoints require an `Api-Key` header.

### Control Plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/collections` | Create a collection |
| GET | `/v1/collections` | List all collections |
| GET | `/v1/collections/:name` | Describe a collection |
| PATCH | `/v1/collections/:name` | Configure a collection (tags, bm25) |
| DELETE | `/v1/collections/:name` | Delete a collection |
| POST | `/v1/collections/:name/describe_collection_stats` | Get collection statistics |

### Data Plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/collections/:name/records/upsert` | Upsert records |
| POST | `/v1/collections/:name/records/fetch` | Fetch records by ID |
| POST | `/v1/collections/:name/records/fetch_by_metadata` | Fetch records by metadata filter |
| POST | `/v1/collections/:name/records/delete` | Delete records |
| POST | `/v1/collections/:name/records/update` | Update a record's metadata |
| GET | `/v1/collections/:name/records/list` | List record IDs (IDs only) |
| POST | `/v1/collections/:name/records/scroll` | Scroll all records with cursor pagination |
| POST | `/v1/collections/:name/sample` | Random sample of records |
| POST | `/v1/collections/:name/query` | Query nearest neighbors |
| POST | `/v1/collections/:name/query/hybrid` | Hybrid dense + BM25 query |
| POST | `/v1/collections/:name/query/batch` | Run up to 10 queries concurrently |
| POST | `/v1/collections/:name/recommend` | Recommend by positive/negative example IDs |
| POST | `/v1/collections/:name/facets` | Aggregated counts of distinct metadata values |

### Namespaces

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/collections/:name/namespaces` | List namespaces |
| POST | `/v1/collections/:name/namespaces` | Create a namespace |
| GET | `/v1/collections/:name/namespaces/:ns` | Describe a namespace |
| DELETE | `/v1/collections/:name/namespaces/:ns` | Delete a namespace |

### Aliases

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/aliases` | Create or update an alias |
| GET | `/v1/aliases` | List all aliases |
| GET | `/v1/aliases/:alias` | Describe an alias |
| DELETE | `/v1/aliases/:alias` | Delete an alias |

### Health & Admin

| Method | Path | Port | Description |
|--------|------|------|-------------|
| GET | `/health` | 8080 | Health check |
| GET | `/ready` | 8080 | Readiness check |
| GET | `/version` | 8080 | Server version |
| GET | `/metrics` | 9090 | Prometheus metrics |
| POST | `/admin/api_keys` | 9090 | Create API key |
| DELETE | `/admin/api_keys/:id` | 9090 | Revoke API key |
| POST | `/admin/collections/:name/reindex` | 9090 | Rebuild DiskANN index |
| POST | `/admin/collections/:name/vacuum` | 9090 | Vacuum a collection |
| GET | `/admin/config` | 9090 | Dump current config |

## Hybrid Search

Create a BM25-enabled collection, upsert records with text, and query with both vector and keyword:

```bash
# Create with BM25 enabled
curl -s -X POST http://localhost:8080/v1/collections \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"hybrid-col","dimension":3,"metric":"cosine","bm25Enabled":true}'

# Upsert with text for BM25
curl -s -X POST http://localhost:8080/v1/collections/hybrid-col/records/upsert \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "records": [
      {"id":"r1","values":[1,0,0],"text":"machine learning basics"},
      {"id":"r2","values":[0,1,0],"text":"cooking recipes"}
    ]
  }'

# Hybrid query (dense + BM25, fused with RRF)
curl -s -X POST http://localhost:8080/v1/collections/hybrid-col/query/hybrid \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"vector":[1,0,0],"text":"machine learning","topK":5}'
```

## Reranking

Add a `rerank` object to any query to rerank results using a natural language query:

```bash
curl -s -X POST http://localhost:8080/v1/collections/my-collection/query \
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
docker compose --profile reranking up -d
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
curl -s -X POST http://localhost:8080/v1/collections/my-collection/query/batch \
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
curl -s -X POST http://localhost:8080/v1/collections/my-collection/recommend \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"positiveIds":["rec-1","rec-2"],"negativeIds":["rec-9"],"topK":10}'
```

### Faceted Counts

Get aggregated counts of distinct metadata values for a field, ordered by count. Supports the same filter DSL as queries and an optional `namespace`:

```json
{ "field": "category", "filter": { "in_stock": { "$eq": "true" } }, "limit": 20 }
```

Records missing the field are excluded. Maximum `limit` is 100 (default 20).

### Advanced Metadata Filtering

**Datetime ranges** — use ISO 8601 strings with `$gt`/`$gte`/`$lt`/`$lte`; no epoch-integer conversion needed:

```json
{ "filter": { "created_at": { "$gte": "2025-01-01T00:00:00Z" } } }
```

**Geo radius** — filter records within a distance from a lat/lon point (requires a `location` field with `lat`/`lon` sub-keys):

```json
{ "filter": { "location": { "$geoRadius": { "lat": 40.7, "lon": -74.0, "radiusMeters": 5000 } } } }
```

**Geo bounding box:**

```json
{ "filter": { "location": { "$geoBBox": { "minLat": 40.0, "maxLat": 41.5, "minLon": -75.0, "maxLon": -73.0 } } } }
```

**Array element matching** — filter records where a metadata array contains at least one element matching a condition:

```json
{ "filter": { "tags": { "$elemMatch": { "type": "premium" } } } }
```

### Collection Aliases

Aliases resolve transparently on every endpoint, enabling zero-downtime collection swaps:

```bash
# Point "prod" at a new collection atomically
curl -s -X POST http://localhost:8080/v1/aliases \
  -H "Api-Key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"alias":"prod","collectionName":"my-collection-v2"}'
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
col = client.vector.collection("my-collection")
results = await col.query(vector=[0.1, 0.2, 0.3], top_k=5)
```

```typescript
import { Onecortex } from '@onecortex/sdk';

const client = new Onecortex({ url: 'http://localhost:8080', apiKey: 'your-api-key' });
const col = client.vector.collection('my-collection');
const results = await col.query({ vector: [0.1, 0.2, 0.3], topK: 5 });
```

## Architecture

```
Client → REST API (axum) → PostgreSQL 18
                              ├── pgvector (vector storage + operators)
                              ├── pgvectorscale (StreamingDiskANN indexing)
                              └── pg_textsearch (BM25 full-text search)
```

Two PostgreSQL schemas are used: `_onecortex_vector` holds the system catalog (collections, api_keys, aliases, stats), and `_onecortex` holds user data — one table per collection named `col_<uuid>`. Keeping user data in `_onecortex` allows other Onecortex services on the same PostgreSQL instance to store their own data under the same shared namespace. Migrations are managed by sqlx and applied automatically on server startup.

## Development

```bash
# Build
cargo build

# Run tests (requires PostgreSQL)
docker compose up -d postgres
cargo test

# Lint
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
