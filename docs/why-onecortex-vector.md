# Why Onecortex Vector

## The problem

Building AI applications with retrieval-augmented generation (RAG)
pipelines, semantic search, or recommendation engines is straightforward
in a demo and surprisingly difficult in production at scale.

Engineering teams hit the same five roadblocks:

**Search precision failures.** Pure vector search captures semantic
meaning but misses exact details. It struggles to distinguish similar
identifiers (`invoice #123456` vs. `invoice #123457`) and rare
technical terms. The result in a RAG pipeline is hallucinated or
incorrect output that's hard to debug.

**Architectural complexity.** High-quality retrieval needs hybrid
search — dense vectors plus full-text plus reranking — which usually
means bolting separate vector and full-text systems together with
custom merge logic. A simple query becomes a distributed pipeline.

**Fragmented infrastructure.** Managed services like Pinecone bring
high cost, vendor lock-in, and a third-party data store that raises
compliance and data-residency concerns. Self-hosted engines like
Milvus and Weaviate require orchestrating heavy stateful components
(etcd, message queues). Qdrant is open and lighter, but it still adds
a separate stack to operate alongside your primary database.

**Unpredictable scaling.** Combining scalar / metadata filters with
hybrid vector search is a common bottleneck. Most systems suffer
unpredictable latency and degraded recall as datasets grow from
thousands to millions of documents.

**Consistency and maintenance.** Most vector databases struggle with
continuous updates and schema shifts. Upgrading embedding models or
handling real-time inserts often leads to silent precision drops with
no obvious rollback path.

The result: AI engineering teams spend more time managing retrieval
infrastructure than building products on top of it.

## The take

Onecortex Vector is built on the bet that PostgreSQL — extended with
`pgvector`, `pgvectorscale`, and `pg_textsearch` — is enough for the
vast majority of RAG and semantic-search workloads. By staying inside
Postgres, you keep:

- One database to back up, monitor, and secure.
- Your existing IAM, network policies, and audit trail.
- Schema migrations, transactions, and joins between embeddings and
  the rest of your application data.
- The escape hatch of writing raw SQL when the API doesn't fit.

You give up nothing in capability: dense ANN, BM25 fusion, geo
filtering, reranking, and a rich filter DSL ship in the box. You give
up the distributed-system tax that other vector databases charge.

## When you might want something else

- You need millisecond p99 latency at billions of vectors and you've
  already validated that a single Postgres won't keep up. Distributed
  vector engines (Milvus, Vespa) are the right tool.
- You're committed to fully-managed SaaS and don't want to run any
  database. Pinecone or a managed Qdrant Cloud will fit better.
- Your only requirement is dense ANN and you don't need hybrid,
  reranking, or filtering. Bare `pgvector` is simpler.

For everything in between — which covers most production RAG today —
Onecortex Vector is designed to be the obvious choice.
