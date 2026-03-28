# onecortex-vector

Python client for [Onecortex Vector](https://github.com/onecortex-io/onecortex-vector) — a self-hosted vector database with hybrid search and reranking.

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Client Configuration](#client-configuration)
- [Index Management](#index-management)
- [Data Operations](#data-operations)
- [Querying](#querying)
- [Namespaces](#namespaces)
- [Metadata Filtering Reference](#metadata-filtering-reference)
- [Error Handling](#error-handling)
- [Auto-Retry Behavior](#auto-retry-behavior)
- [Drop-in Replacement](#drop-in-replacement)
- [API Reference](#api-reference)

## Installation

```bash
pip install onecortex-vector
```

Requires Python 3.11+.

## Quick Start

```python
from onecortex import Onecortex

# Connect to the server
pc = Onecortex(api_key="your-api-key", host="http://localhost:8080")

# Create an index (dimension=3 for brevity; use 1536 for OpenAI embeddings)
pc.create_index(name="my-index", dimension=3, metric="cosine")

# Get an index handle
idx = pc.Index("my-index")

# Upsert vectors
idx.upsert(vectors=[
    {"id": "vec-1", "values": [0.1, 0.2, 0.3], "metadata": {"genre": "sci-fi", "year": 2024}},
    {"id": "vec-2", "values": [0.4, 0.5, 0.6], "metadata": {"genre": "fantasy", "year": 2023}},
    {"id": "vec-3", "values": [0.7, 0.8, 0.9], "metadata": {"genre": "sci-fi", "year": 2025}},
])

# Query for similar vectors
results = idx.query(vector=[0.1, 0.2, 0.3], top_k=3, include_metadata=True)
for match in results.matches:
    print(f"{match.id}: score={match.score}, metadata={match.metadata}")

# Clean up
pc.delete_index("my-index")
```

## Client Configuration

```python
from onecortex import Onecortex

# Both parameters
pc = Onecortex(api_key="your-api-key", host="http://localhost:8080")

# host defaults to http://localhost:8080
pc = Onecortex(api_key="your-api-key")
```

## Index Management

### Create an Index

```python
# Cosine similarity (default)
idx = pc.create_index(name="my-index", dimension=3, metric="cosine")

# Euclidean distance (scores normalized as 1/(1+distance))
idx = pc.create_index(name="euclidean-index", dimension=3, metric="euclidean")

# Dot product
idx = pc.create_index(name="dotprod-index", dimension=3, metric="dotproduct")
```

### Create a BM25-Enabled Index

Required for hybrid search (`query_hybrid`).

```python
idx = pc.create_index(name="articles", dimension=3, metric="cosine", bm25_enabled=True)
```

### Create with Deletion Protection and Tags

```python
idx = pc.create_index(
    name="production-index",
    dimension=1536,
    metric="cosine",
    deletion_protection="enabled",
    tags={"env": "production", "team": "search"},
)
```

### Describe an Index

```python
desc = pc.describe_index("my-index")

print(desc.name)           # "my-index"
print(desc.dimension)      # 3
print(desc.metric)         # "cosine"
print(desc.status.ready)   # True
print(desc.status.state)   # "Ready"
print(desc.host)           # "localhost:8080"
print(desc.tags)           # {"env": "production"} or None
```

### List All Indexes

```python
indexes = pc.list_indexes()
for idx_desc in indexes:
    print(f"{idx_desc.name} (dim={idx_desc.dimension}, metric={idx_desc.metric})")
```

### Configure an Index

Update deletion protection and/or tags on an existing index.

```python
updated = pc.configure_index(
    "my-index",
    deletion_protection="disabled",
    tags={"env": "staging", "version": "2"},
)
print(updated.tags)  # {"env": "staging", "version": "2"}
```

### Check if an Index Exists

```python
if pc.has_index("my-index"):
    print("Index exists")
else:
    print("Index does not exist")
```

### Delete an Index

```python
# Deletion protection must be disabled first
pc.configure_index("my-index", deletion_protection="disabled")
pc.delete_index("my-index")
```

## Data Operations

### Get an Index Handle

Returns a handle for data-plane operations. This does not make a network call.

```python
idx = pc.Index("my-index")
```

### Upsert Vectors

```python
result = idx.upsert(vectors=[
    {"id": "vec-1", "values": [0.1, 0.2, 0.3], "metadata": {"genre": "sci-fi", "year": 2024}},
    {"id": "vec-2", "values": [0.4, 0.5, 0.6], "metadata": {"genre": "fantasy", "year": 2023}},
])

print(result.upserted_count)  # 2
```

If a vector with the same ID already exists, it is overwritten (upsert semantics).

### Upsert with Text

Include a `text` field for BM25 full-text search. Requires a BM25-enabled index.

```python
idx = pc.Index("articles")
idx.upsert(vectors=[
    {
        "id": "doc-1",
        "values": [0.1, 0.2, 0.3],
        "metadata": {"category": "tech"},
        "text": "Machine learning fundamentals and neural networks",
    },
    {
        "id": "doc-2",
        "values": [0.4, 0.5, 0.6],
        "metadata": {"category": "cooking"},
        "text": "The quick brown fox jumps over the lazy dog",
    },
])
```

### Batch Upsert

Upsert large datasets in configurable batches. Returns the total number of upserted vectors.

```python
vectors = [
    {"id": f"vec-{i}", "values": [float(i), 0.0, 0.0]}
    for i in range(10_000)
]

total = idx.upsert_batch(vectors=vectors, batch_size=200)
print(f"Upserted {total} vectors")
```

### Fetch by IDs

```python
result = idx.fetch(ids=["vec-1", "vec-2"])

for vid, data in result.vectors.items():
    print(f"{vid}: values={data['values']}, metadata={data['metadata']}")

print(result.namespace)  # "" (default namespace)
```

### Fetch by Metadata Filter

An Onecortex extension for retrieving vectors that match a metadata filter without a vector query.

```python
result = idx.fetch_by_metadata(
    filter={"genre": {"$eq": "sci-fi"}},
    limit=50,
    include_values=False,
    include_metadata=True,
)

for vid, data in result.vectors.items():
    print(f"{vid}: {data['metadata']}")
```

### Update a Vector

Update a vector's embedding values, metadata, and/or text. Metadata is **merged** with existing metadata, not replaced.

```python
# Update only metadata (merged with existing)
idx.update(id="vec-1", set_metadata={"year": 2025, "reviewed": True})

# Update vector values
idx.update(id="vec-1", values=[0.11, 0.22, 0.33])

# Update text content (for BM25 re-indexing)
idx.update(id="doc-1", text="Updated machine learning content")

# Update everything at once
idx.update(
    id="vec-1",
    values=[0.11, 0.22, 0.33],
    set_metadata={"year": 2025},
    text="Updated content",
)
```

### Delete Vectors

#### Delete by IDs

```python
idx.delete(ids=["vec-1", "vec-2"])
```

#### Delete by Metadata Filter

```python
idx.delete(filter={"genre": {"$eq": "fantasy"}})
```

#### Delete All Vectors in a Namespace

```python
idx.delete(delete_all=True)

# Delete all in a specific namespace
idx.delete(delete_all=True, namespace="articles")
```

### List Vector IDs

List vector IDs with optional prefix filtering and pagination.

```python
# Basic list
result = idx.list()
for v in result.vectors:
    print(v["id"])

# Filter by ID prefix
result = idx.list(prefix="doc-", limit=50)

# Paginate through all vectors
token = None
all_ids = []
while True:
    result = idx.list(limit=100, pagination_token=token)
    all_ids.extend(v["id"] for v in result.vectors)
    if not result.pagination or not result.pagination.get("next"):
        break
    token = result.pagination["next"]

print(f"Total vectors: {len(all_ids)}")
```

### Describe Index Stats

```python
stats = idx.describe_index_stats()

print(f"Dimension: {stats.dimension}")
print(f"Total vectors: {stats.total_vector_count}")
print(f"Index fullness: {stats.index_fullness}")

for ns_name, ns_summary in stats.namespaces.items():
    print(f"  Namespace '{ns_name}': {ns_summary.vector_count} vectors")
```

## Querying

### Dense ANN Query

```python
results = idx.query(
    vector=[0.1, 0.2, 0.3],
    top_k=5,
    include_metadata=True,
    include_values=False,
)

for match in results.matches:
    print(f"{match.id}: score={match.score}, metadata={match.metadata}")
```

### Query by Vector ID

Find vectors similar to an existing vector, referenced by its ID.

```python
results = idx.query(id="vec-1", top_k=5, include_metadata=True)

for match in results.matches:
    print(f"{match.id}: score={match.score}")
```

### Query with Metadata Filter

Combine vector similarity search with metadata filtering.

```python
# Simple filter
results = idx.query(
    vector=[0.1, 0.2, 0.3],
    top_k=10,
    filter={"genre": {"$eq": "sci-fi"}},
    include_metadata=True,
)

# Compound filter
results = idx.query(
    vector=[0.1, 0.2, 0.3],
    top_k=10,
    filter={
        "$and": [
            {"genre": {"$eq": "sci-fi"}},
            {"year": {"$gte": 2024}},
        ]
    },
    include_metadata=True,
)
```

### Hybrid Search (Dense + BM25)

Combines dense vector similarity with BM25 keyword matching using Reciprocal Rank Fusion (RRF). Requires a BM25-enabled index.

```python
results = idx.query_hybrid(
    vector=[0.1, 0.2, 0.3],
    text="machine learning",
    top_k=10,
    alpha=0.5,  # 0.0 = pure BM25, 0.5 = balanced, 1.0 = pure dense
    include_metadata=True,
)

for match in results.matches:
    print(f"{match.id}: score={match.score}")
```

The `alpha` parameter controls the blend:

| Alpha | Behavior |
|-------|----------|
| `0.0` | Pure BM25 keyword search |
| `0.5` | Equal blend of dense and BM25 (default) |
| `0.7` | Favor dense similarity |
| `1.0` | Pure dense ANN search |

```python
# Favor keyword matching
results = idx.query_hybrid(vector=[0.1, 0.2, 0.3], text="neural networks", top_k=5, alpha=0.3)

# Favor dense similarity
results = idx.query_hybrid(vector=[0.1, 0.2, 0.3], text="neural networks", top_k=5, alpha=0.8)
```

Hybrid search also supports metadata filters:

```python
results = idx.query_hybrid(
    vector=[0.1, 0.2, 0.3],
    text="machine learning",
    top_k=10,
    alpha=0.5,
    filter={"category": {"$eq": "tech"}},
    include_metadata=True,
)
```

### Reranking

Apply a reranker to re-score results using a natural language query. Works with both `query` and `query_hybrid`.

```python
# Rerank on a dense query
results = idx.query(
    vector=[0.1, 0.2, 0.3],
    top_k=20,
    rerank={
        "query": "What are the fundamentals of machine learning?",
        "topN": 5,          # Return top 5 after reranking (default: top_k)
        "rankField": "text", # Metadata field to rank against (default: "text")
    },
    include_metadata=True,
)

# Rerank on a hybrid query
results = idx.query_hybrid(
    vector=[0.1, 0.2, 0.3],
    text="machine learning",
    top_k=20,
    alpha=0.5,
    rerank={
        "query": "What are the fundamentals of machine learning?",
        "topN": 5,
    },
    include_metadata=True,
)
```

Rerank options:

| Option | Type | Description |
|--------|------|-------------|
| `query` | `str` | Natural language query for the reranker (required) |
| `topN` | `int` | Number of results to return after reranking (defaults to `top_k`) |
| `rankField` | `str` | Metadata field containing text to rank against (defaults to `"text"`) |
| `model` | `str` | Per-request reranker model override |

## Namespaces

All data operations support an optional `namespace` parameter. The default namespace is `""` (empty string).

```python
idx = pc.Index("my-index")

# Upsert into a namespace
idx.upsert(
    vectors=[{"id": "vec-1", "values": [0.1, 0.2, 0.3]}],
    namespace="articles",
)

# Query within a namespace
results = idx.query(vector=[0.1, 0.2, 0.3], top_k=5, namespace="articles")

# Fetch from a namespace
fetched = idx.fetch(ids=["vec-1"], namespace="articles")

# List vectors in a namespace
listed = idx.list(namespace="articles")

# Delete within a namespace
idx.delete(ids=["vec-1"], namespace="articles")

# Delete all vectors in a namespace
idx.delete(delete_all=True, namespace="articles")

# View per-namespace stats
stats = idx.describe_index_stats()
for ns_name, ns_summary in stats.namespaces.items():
    print(f"Namespace '{ns_name}': {ns_summary.vector_count} vectors")
```

## Metadata Filtering Reference

Metadata filters can be used with `query`, `query_hybrid`, `fetch_by_metadata`, and `delete`.

### Comparison Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `$eq` | Equal to | `{"status": {"$eq": "active"}}` |
| `$ne` | Not equal to | `{"status": {"$ne": "deleted"}}` |
| `$gt` | Greater than | `{"price": {"$gt": 100}}` |
| `$gte` | Greater than or equal | `{"price": {"$gte": 100}}` |
| `$lt` | Less than | `{"price": {"$lt": 50}}` |
| `$lte` | Less than or equal | `{"price": {"$lte": 50}}` |
| `$in` | In set | `{"color": {"$in": ["red", "blue"]}}` |
| `$nin` | Not in set | `{"color": {"$nin": ["green"]}}` |

### Logical Operators

```python
# AND: all conditions must match
filter = {
    "$and": [
        {"genre": {"$eq": "sci-fi"}},
        {"year": {"$gte": 2024}},
    ]
}

# OR: at least one condition must match
filter = {
    "$or": [
        {"genre": {"$eq": "sci-fi"}},
        {"genre": {"$eq": "fantasy"}},
    ]
}

# Combine AND and OR
filter = {
    "$and": [
        {"year": {"$gte": 2020}},
        {
            "$or": [
                {"genre": {"$eq": "sci-fi"}},
                {"genre": {"$eq": "fantasy"}},
            ]
        },
    ]
}
```

### Nested Fields (Dot Notation)

```python
filter = {"address.city": {"$eq": "San Francisco"}}
filter = {"user.role": {"$in": ["admin", "editor"]}}
```

## Error Handling

```python
from onecortex import (
    OnecortexError,
    NotFoundError,
    AlreadyExistsError,
    InvalidArgumentError,
    UnauthorizedError,
    PermissionDeniedError,
)

try:
    pc.describe_index("nonexistent")
except NotFoundError as e:
    print(f"Index not found: {e} (status={e.status_code})")
except AlreadyExistsError as e:
    print(f"Already exists: {e}")
except InvalidArgumentError as e:
    print(f"Bad request: {e}")
except UnauthorizedError as e:
    print(f"Invalid API key: {e}")
except PermissionDeniedError as e:
    print(f"Permission denied: {e}")
except OnecortexError as e:
    print(f"Server error: {e} (status={e.status_code})")
```

All exceptions inherit from `OnecortexError` and carry an optional `status_code` attribute.

## Auto-Retry Behavior

The SDK automatically retries requests on:

- **HTTP 429** (rate limited)
- **HTTP 5xx** (server errors)

Retries use exponential backoff with delays of 1s, 2s, and 4s (up to 3 retries). No configuration is needed.

## Drop-in Replacement

If you are migrating from another vector database SDK with the same API shape, switching to Onecortex requires only changing the import and client initialization:

```python
# Before
# from pinecone import Pinecone
# pc = Pinecone(api_key="...")

# After
from onecortex import Onecortex
pc = Onecortex(api_key="your-onecortex-key", host="http://your-server:8080")
```

The following parameters are accepted and silently ignored for compatibility:
- `spec=` on `create_index()` (e.g., `spec={"serverless": {"cloud": "aws", "region": "us-east-1"}}`)
- `sparseValues` on upsert vectors (sparse vectors are not stored)

## API Reference

### Client Methods

| Method | Description |
|--------|-------------|
| `Onecortex(api_key, host)` | Create a client |
| `create_index(name, dimension, metric, bm25_enabled, deletion_protection, tags)` | Create a new index |
| `describe_index(name)` | Get index metadata |
| `list_indexes()` | List all indexes |
| `configure_index(name, deletion_protection, tags)` | Update index settings |
| `has_index(name)` | Check if an index exists |
| `delete_index(name)` | Delete an index |
| `Index(name)` | Get an index handle for data operations |

### Index Methods

| Method | Description |
|--------|-------------|
| `upsert(vectors, namespace)` | Insert or update vectors |
| `upsert_batch(vectors, namespace, batch_size)` | Upsert in configurable batches |
| `fetch(ids, namespace)` | Fetch vectors by ID |
| `fetch_by_metadata(filter, namespace, limit, include_values, include_metadata)` | Fetch vectors matching a metadata filter |
| `delete(ids, filter, delete_all, namespace)` | Delete vectors |
| `update(id, values, set_metadata, text, namespace)` | Update a vector |
| `query(vector, top_k, namespace, filter, include_values, include_metadata, id, rerank)` | Dense ANN search |
| `query_hybrid(vector, text, top_k, alpha, namespace, filter, include_metadata, include_values, rerank)` | Hybrid dense + BM25 search |
| `list(namespace, prefix, limit, pagination_token)` | List vector IDs |
| `describe_index_stats()` | Get index statistics |
