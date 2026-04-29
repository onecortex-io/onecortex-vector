# Onecortex Vector

> **A modern, high-performance hybrid vector database built on PostgreSQL in Rust.**

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.2.0-green.svg)](CHANGELOG.md)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org)
[![Docs](https://img.shields.io/badge/docs-api%20reference-informational)](docs/api-reference.md)

The full retrieval stack production RAG actually needs — dense ANN,
BM25, reranking, geo filtering, and a rich metadata DSL — without the
operational tax of a separate distributed system to run alongside your
primary database.

---

## Quickstart

```bash
docker compose up -d postgres
cargo run                                              # listens on :8080

curl -X POST localhost:8080/v1/collections \
  -d '{"name":"docs","dimension":3,"metric":"cosine"}'

curl -X POST localhost:8080/v1/collections/docs/records/upsert \
  -d '{"records":[{"id":"r1","values":[0.1,0.2,0.3],"metadata":{"genre":"sci-fi"}}]}'

curl -X POST localhost:8080/v1/collections/docs/query \
  -d '{"vector":[0.1,0.2,0.3],"topK":5,"includeMetadata":true}'
```

That's the whole loop. No cluster bootstrap, no etcd, no message
queue, no proprietary index format on disk.

## Why

**Built on Postgres.** A thin REST API on top of `pgvector`,
`pgvectorscale`, and `pg_textsearch`. Your embeddings live in the same
database as the rest of your application data. Your existing backups,
IAM, monitoring, and migrations keep working.

**Hybrid out of the box.** Dense ANN, BM25 keyword, geo radius, and
reranking — fused with reciprocal rank fusion in a single endpoint.
No glue code, no second system to operate.

**One container to operate.** No distributed-system tax. Run it on
your laptop, behind your gateway, or as part of the
[Onecortex platform](https://github.com/onecortex-io). Same binary.

For the full take and trade-offs, see
[Why Onecortex Vector](docs/why-onecortex-vector.md).

## Compared to

|                          | Onecortex Vector | Pinecone | Qdrant / Weaviate / Milvus | Bare pgvector |
|--------------------------|:---:|:---:|:---:|:---:|
| Self-hosted              | ✓ | — | ✓ | ✓ |
| Hybrid search built-in   | ✓ | add-on | ✓ | DIY |
| Reranking built-in       | ✓ (Cohere/Voyage/Jina/Pinecone/local) | DIY | add-on | DIY |
| Geo filtering            | ✓ | DIY | ✓ | DIY |
| Operational footprint    | one Postgres | managed SaaS | cluster + state | one Postgres |
| Vendor lock-in           | none | yes | none | none |

## Features

- **Dense ANN** — cosine, Euclidean, dot product via StreamingDiskANN.
- **Hybrid search** — dense + BM25 + geo radius, fused with RRF.
- **Reranking** — Cohere, Voyage, Jina, Pinecone Inference, or a
  self-hosted cross-encoder. One env var to switch.
- **Filtering DSL** — `$eq`/`$ne`/`$gt`/`$lt`/`$in`/`$nin`/`$and`/`$or`,
  ISO 8601 datetimes, `$geoRadius`, `$geoBBox`, `$elemMatch`.
- **Namespaces** — scoped operations inside a collection for
  multi-tenant data isolation.
- **Aliases** — atomic, zero-downtime collection swaps for A/B
  testing and reindex flips.
- **Batch & scroll** — fan out 10 queries in one call, cursor through
  millions of records, draw random samples.
- **Group by, score threshold, faceted counts, recommendations** — all
  the retrieval primitives RAG pipelines actually use.
- **Stable error taxonomy** — typed codes, structured `details`, and
  an `X-Request-Id` on every response so a paste of an error gives an
  operator something to grep.

## Install

**Docker:**

```bash
docker pull ghcr.io/onecortex-io/onecortex-vector:latest
```

**From source (Rust 1.75+):**

```bash
cargo install --git https://github.com/onecortex-io/onecortex-vector
```

**Local development:**

```bash
git clone https://github.com/onecortex-io/onecortex-vector
cd onecortex-vector
docker compose up -d postgres
cargo run
```

Migrations apply automatically on startup. The server listens on
`:8080` (public) and `:9090` (admin / Prometheus metrics).

## Docs

- **[API reference](docs/api-reference.md)** — every endpoint, every parameter
- **[Filters](docs/filters.md)** — the metadata DSL with examples
- **[Reranking](docs/reranking.md)** — pick a backend, configure it
- **[Errors & request ids](docs/api-reference/errors.md)** — full code table and retry guidance
- **[Configuration](docs/configuration.md)** — environment variables
- **[Deployment](docs/deployment.md)** — standalone, behind a gateway, or on the Onecortex platform
- **[Architecture](docs/architecture.md)** — what's running where
- **[Why Onecortex Vector](docs/why-onecortex-vector.md)** — the long version

## SDKs

| Language | Package | Repository |
|----------|---------|------------|
| Python | [`onecortex`](https://pypi.org/project/onecortex/) | [onecortex-python-client](https://github.com/onecortex-io/onecortex-python-client) |
| TypeScript | [`@onecortex/sdk`](https://www.npmjs.com/package/@onecortex/sdk) | [onecortex-typescript-client](https://github.com/onecortex-io/onecortex-typescript-client) |

```python
from onecortex import Onecortex
client = Onecortex(url="http://localhost:8080")
results = await client.vector.collection("docs").query(
    vector=[0.1, 0.2, 0.3], top_k=5
)
```

```typescript
import { Onecortex } from '@onecortex/sdk';
const client = new Onecortex({ url: 'http://localhost:8080' });
const results = await client.vector.collection('docs').query({
  vector: [0.1, 0.2, 0.3], topK: 5,
});
```

## What's next

Highlights from the next few releases:

- Server-side embeddings (`embedder` on the collection) and an
  `/ingest` endpoint that handles chunking + embedding for you.
- A `search(text=...)` cross-mode query that picks dense / hybrid /
  rerank by query plan.
- A `/_playground` web UI for quick exploration.
- An `EXPLAIN`-style endpoint that returns the underlying SQL plan
  and selectivity estimates.

Watch the [CHANGELOG](CHANGELOG.md) for what shipped, and open an
issue if there's something you'd like prioritised.

## Contributing

Issues and PRs welcome. Quick orientation:

```bash
docker compose up -d postgres   # tests need Postgres
cargo test                      # 127 tests, mostly integration
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

Look for issues tagged
[`good-first-issue`](https://github.com/onecortex-io/onecortex-vector/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22)
for a low-friction starting point.

## License

Apache License 2.0 — see [LICENSE](LICENSE).
