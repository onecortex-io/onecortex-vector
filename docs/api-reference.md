# API Reference

All public endpoints live under `/v1/`. Health endpoints (`/health`, `/ready`,
`/version`) are unversioned. Admin endpoints live under `/admin/` on a
separate port.

JSON field names are camelCase on the wire. Errors share a single envelope
documented in [errors.md](api-reference/errors.md), and every response
carries an `X-Request-Id` header.

## Control plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/collections` | Create a collection |
| GET | `/v1/collections` | List all collections |
| GET | `/v1/collections/:name` | Describe a collection |
| PATCH | `/v1/collections/:name` | Configure a collection (tags, bm25Enabled, deletionProtected) |
| DELETE | `/v1/collections/:name` | Delete a collection |
| POST | `/v1/collections/:name/describe_collection_stats` | Collection statistics |

## Data plane

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/collections/:name/records/upsert` | Upsert records (see [Upsert: duplicate ids](#upsert-duplicate-ids)) |
| POST | `/v1/collections/:name/records/fetch` | Fetch records by id |
| POST | `/v1/collections/:name/records/fetch_by_metadata` | Fetch records by metadata filter |
| POST | `/v1/collections/:name/records/delete` | Delete records |
| POST | `/v1/collections/:name/records/update` | Update a record's metadata |
| GET | `/v1/collections/:name/records/list` | List record ids only |
| POST | `/v1/collections/:name/records/scroll` | Scroll all records (cursor pagination) |
| POST | `/v1/collections/:name/sample` | Random sample of records |
| POST | `/v1/collections/:name/query` | Nearest-neighbour query |
| POST | `/v1/collections/:name/query/hybrid` | Hybrid dense + BM25 query |
| POST | `/v1/collections/:name/query/batch` | Up to 10 queries concurrently |
| POST | `/v1/collections/:name/recommend` | Recommend by positive/negative example ids |
| POST | `/v1/collections/:name/facets` | Aggregated counts of distinct metadata values |

### Upsert: duplicate ids

Records are deduplicated by `id` within each upsert request before being
written. If the same `id` appears multiple times in `records[]`, the
**last** occurrence wins (last-write-wins). The response field
`upsertedCount` reflects the number of distinct ids that were actually
written, which may be less than `records.length`. Namespace is
request-scoped, so dedupe is by `id` only.

## Namespaces

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/collections/:name/namespaces` | List namespaces |
| POST | `/v1/collections/:name/namespaces` | Create a namespace |
| GET | `/v1/collections/:name/namespaces/:ns` | Describe a namespace |
| DELETE | `/v1/collections/:name/namespaces/:ns` | Delete a namespace |

## Aliases

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/aliases` | Create or update an alias |
| GET | `/v1/aliases` | List all aliases |
| GET | `/v1/aliases/:alias` | Describe an alias |
| DELETE | `/v1/aliases/:alias` | Delete an alias |

Aliases resolve transparently on every endpoint that takes a `:name`,
which is what makes zero-downtime collection swaps work:

```bash
curl -X POST http://localhost:8080/v1/aliases \
  -H "Content-Type: application/json" \
  -d '{"alias":"prod","collectionName":"docs-v2"}'
```

## Health & admin

| Method | Path | Port | Description |
|--------|------|------|-------------|
| GET | `/health` | 8080 | Liveness check |
| GET | `/ready` | 8080 | Readiness check |
| GET | `/version` | 8080 | Server version |
| GET | `/metrics` | 9090 | Prometheus metrics |
| POST | `/admin/collections/:name/reindex` | 9090 | Rebuild DiskANN index |
| POST | `/admin/collections/:name/vacuum` | 9090 | Vacuum a collection |
| GET | `/admin/config` | 9090 | Dump current config |

## Advanced query features

### Score threshold

Drop results below a minimum similarity score (applied after reranking):

```json
{ "vector": [...], "topK": 10, "scoreThreshold": 0.75 }
```

### Batch query

Up to 10 queries in one round-trip; results returned in the same order:

```bash
curl -X POST http://localhost:8080/v1/collections/docs/query/batch \
  -H "Content-Type: application/json" \
  -d '{"queries":[{"vector":[1,0,0],"topK":5},{"vector":[0,1,0],"topK":3}]}'
```

### Group by

Group nearest-neighbour results by a metadata field to avoid
same-source clustering:

```json
{
  "vector": [...],
  "topK": 50,
  "groupBy": { "field": "documentId", "limit": 5, "groupSize": 2 }
}
```

If the field is absent on every matched record, the response is a 400
`GROUPBY_FIELD_MISSING` rather than a single empty-keyed bucket.

### Recommendations

Find similar items from example ids — no query vector needed:

```bash
curl -X POST http://localhost:8080/v1/collections/docs/recommend \
  -H "Content-Type: application/json" \
  -d '{"positiveIds":["r1","r2"],"negativeIds":["r9"],"topK":10}'
```

### Faceted counts

Aggregated counts of distinct metadata values for a field, ordered by
count, with optional filter and namespace scoping:

```json
{ "field": "category", "filter": { "inStock": { "$eq": "true" } }, "limit": 20 }
```

Records missing the field are excluded. Maximum `limit` is 100 (default 20).

For the metadata filter DSL itself, see [filters.md](filters.md).
