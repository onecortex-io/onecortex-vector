# @onecortex/vector

TypeScript client for [Onecortex Vector](https://github.com/onecortex-io/onecortex-vector) — a self-hosted vector database with hybrid search and reranking.

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
npm install @onecortex/vector
```

Requires Node.js 18+.

## Quick Start

```typescript
import { Onecortex } from '@onecortex/vector';

// Connect to the server
const pc = new Onecortex({ apiKey: 'your-api-key', host: 'http://localhost:8080' });

// Create an index (dimension=3 for brevity; use 1536 for OpenAI embeddings)
await pc.createIndex({ name: 'my-index', dimension: 3, metric: 'cosine' });

// Get an index handle
const idx = pc.index('my-index');

// Upsert vectors
await idx.upsert({
  vectors: [
    { id: 'vec-1', values: [0.1, 0.2, 0.3], metadata: { genre: 'sci-fi', year: 2024 } },
    { id: 'vec-2', values: [0.4, 0.5, 0.6], metadata: { genre: 'fantasy', year: 2023 } },
    { id: 'vec-3', values: [0.7, 0.8, 0.9], metadata: { genre: 'sci-fi', year: 2025 } },
  ],
});

// Query for similar vectors
const results = await idx.query({ vector: [0.1, 0.2, 0.3], topK: 3, includeMetadata: true });
for (const match of results.matches) {
  console.log(`${match.id}: score=${match.score}, metadata=${JSON.stringify(match.metadata)}`);
}

// Clean up
await pc.deleteIndex('my-index');
```

> All examples below assume an async context (e.g., inside an `async function`).

## Client Configuration

```typescript
import { Onecortex } from '@onecortex/vector';

// Both parameters
const pc = new Onecortex({ apiKey: 'your-api-key', host: 'http://localhost:8080' });

// host defaults to http://localhost:8080
const pc = new Onecortex({ apiKey: 'your-api-key' });
```

## Index Management

### Create an Index

```typescript
// Cosine similarity (default)
await pc.createIndex({ name: 'my-index', dimension: 3, metric: 'cosine' });

// Euclidean distance (scores normalized as 1/(1+distance))
await pc.createIndex({ name: 'euclidean-index', dimension: 3, metric: 'euclidean' });

// Dot product
await pc.createIndex({ name: 'dotprod-index', dimension: 3, metric: 'dotproduct' });
```

### Create a BM25-Enabled Index

Required for hybrid search (`queryHybrid`).

```typescript
await pc.createIndex({ name: 'articles', dimension: 3, metric: 'cosine', bm25_enabled: true });
```

### Create with Deletion Protection and Tags

```typescript
await pc.createIndex({
  name: 'production-index',
  dimension: 1536,
  metric: 'cosine',
  deletion_protection: 'enabled',
  tags: { env: 'production', team: 'search' },
});
```

### Describe an Index

```typescript
const desc = await pc.describeIndex('my-index');

console.log(desc.name);          // "my-index"
console.log(desc.dimension);     // 3
console.log(desc.metric);        // "cosine"
console.log(desc.status.ready);  // true
console.log(desc.status.state);  // "Ready"
console.log(desc.host);          // "localhost:8080"
console.log(desc.tags);          // { env: "production" } or undefined
```

### List All Indexes

```typescript
const { indexes } = await pc.listIndexes();
for (const idx of indexes) {
  console.log(`${idx.name} (dim=${idx.dimension}, metric=${idx.metric})`);
}
```

### Configure an Index

Update deletion protection and/or tags on an existing index.

```typescript
const updated = await pc.configureIndex('my-index', {
  deletion_protection: 'disabled',
  tags: { env: 'staging', version: '2' },
});
console.log(updated.tags); // { env: "staging", version: "2" }
```

### Delete an Index

```typescript
// Deletion protection must be disabled first
await pc.configureIndex('my-index', { deletion_protection: 'disabled' });
await pc.deleteIndex('my-index');
```

## Data Operations

### Get an Index Handle

Returns a handle for data-plane operations. This does not make a network call.

```typescript
const idx = pc.index('my-index');
```

### Upsert Vectors

```typescript
const result = await idx.upsert({
  vectors: [
    { id: 'vec-1', values: [0.1, 0.2, 0.3], metadata: { genre: 'sci-fi', year: 2024 } },
    { id: 'vec-2', values: [0.4, 0.5, 0.6], metadata: { genre: 'fantasy', year: 2023 } },
  ],
});

console.log(result.upsertedCount); // 2
```

If a vector with the same ID already exists, it is overwritten (upsert semantics).

### Upsert with Text

Include a `text` field for BM25 full-text search. Requires a BM25-enabled index.

```typescript
const idx = pc.index('articles');
await idx.upsert({
  vectors: [
    {
      id: 'doc-1',
      values: [0.1, 0.2, 0.3],
      metadata: { category: 'tech' },
      text: 'Machine learning fundamentals and neural networks',
    },
    {
      id: 'doc-2',
      values: [0.4, 0.5, 0.6],
      metadata: { category: 'cooking' },
      text: 'The quick brown fox jumps over the lazy dog',
    },
  ],
});
```

### Fetch by IDs

```typescript
const result = await idx.fetch({ ids: ['vec-1', 'vec-2'] });

for (const [vid, data] of Object.entries(result.vectors)) {
  console.log(`${vid}: values=${data.values}, metadata=${JSON.stringify(data.metadata)}`);
}

console.log(result.namespace); // "" (default namespace)
```

### Fetch by Metadata Filter

An Onecortex extension for retrieving vectors that match a metadata filter without a vector query.

```typescript
const result = await idx.fetchByMetadata({
  filter: { genre: { $eq: 'sci-fi' } },
  limit: 50,
  includeValues: false,
  includeMetadata: true,
});

for (const [vid, data] of Object.entries(result.vectors)) {
  console.log(`${vid}: ${JSON.stringify(data.metadata)}`);
}
```

### Update a Vector

Update a vector's embedding values, metadata, and/or text. Metadata is **merged** with existing metadata, not replaced.

```typescript
// Update only metadata (merged with existing)
await idx.update({ id: 'vec-1', setMetadata: { year: 2025, reviewed: true } });

// Update vector values
await idx.update({ id: 'vec-1', values: [0.11, 0.22, 0.33] });

// Update text content (for BM25 re-indexing)
await idx.update({ id: 'doc-1', text: 'Updated machine learning content' });

// Update everything at once
await idx.update({
  id: 'vec-1',
  values: [0.11, 0.22, 0.33],
  setMetadata: { year: 2025 },
  text: 'Updated content',
});
```

### Delete Vectors

#### Delete by IDs

```typescript
await idx.delete({ ids: ['vec-1', 'vec-2'] });
```

#### Delete by Metadata Filter

```typescript
await idx.delete({ filter: { genre: { $eq: 'fantasy' } } });
```

#### Delete All Vectors in a Namespace

```typescript
await idx.delete({ deleteAll: true });

// Delete all in a specific namespace
await idx.delete({ deleteAll: true, namespace: 'articles' });
```

### List Vector IDs

List vector IDs with optional prefix filtering and pagination.

```typescript
// Basic list
const result = await idx.list();
for (const v of result.vectors) {
  console.log(v.id);
}

// Filter by ID prefix
const filtered = await idx.list({ prefix: 'doc-', limit: 50 });

// Paginate through all vectors
let token: string | undefined;
const allIds: string[] = [];
do {
  const page = await idx.list({ limit: 100, paginationToken: token });
  allIds.push(...page.vectors.map((v) => v.id));
  token = page.pagination?.next;
} while (token);

console.log(`Total vectors: ${allIds.length}`);
```

### Describe Index Stats

```typescript
const stats = await idx.describeIndexStats();

console.log(`Dimension: ${stats.dimension}`);
console.log(`Total vectors: ${stats.totalVectorCount}`);
console.log(`Index fullness: ${stats.indexFullness}`);

for (const [nsName, nsSummary] of Object.entries(stats.namespaces)) {
  console.log(`  Namespace '${nsName}': ${nsSummary.vectorCount} vectors`);
}
```

## Querying

### Dense ANN Query

```typescript
const results = await idx.query({
  vector: [0.1, 0.2, 0.3],
  topK: 5,
  includeMetadata: true,
  includeValues: false,
});

for (const match of results.matches) {
  console.log(`${match.id}: score=${match.score}, metadata=${JSON.stringify(match.metadata)}`);
}
```

### Query by Vector ID

Find vectors similar to an existing vector, referenced by its ID.

```typescript
const results = await idx.query({ id: 'vec-1', topK: 5, includeMetadata: true });

for (const match of results.matches) {
  console.log(`${match.id}: score=${match.score}`);
}
```

### Query with Metadata Filter

Combine vector similarity search with metadata filtering.

```typescript
// Simple filter
const results = await idx.query({
  vector: [0.1, 0.2, 0.3],
  topK: 10,
  filter: { genre: { $eq: 'sci-fi' } },
  includeMetadata: true,
});

// Compound filter
const filtered = await idx.query({
  vector: [0.1, 0.2, 0.3],
  topK: 10,
  filter: {
    $and: [
      { genre: { $eq: 'sci-fi' } },
      { year: { $gte: 2024 } },
    ],
  },
  includeMetadata: true,
});
```

### Hybrid Search (Dense + BM25)

Combines dense vector similarity with BM25 keyword matching using Reciprocal Rank Fusion (RRF). Requires a BM25-enabled index.

```typescript
const results = await idx.queryHybrid({
  vector: [0.1, 0.2, 0.3],
  text: 'machine learning',
  topK: 10,
  alpha: 0.5, // 0.0 = pure BM25, 0.5 = balanced, 1.0 = pure dense
  includeMetadata: true,
});

for (const match of results.matches) {
  console.log(`${match.id}: score=${match.score}`);
}
```

The `alpha` parameter controls the blend:

| Alpha | Behavior |
|-------|----------|
| `0.0` | Pure BM25 keyword search |
| `0.5` | Equal blend of dense and BM25 (default) |
| `0.7` | Favor dense similarity |
| `1.0` | Pure dense ANN search |

```typescript
// Favor keyword matching
await idx.queryHybrid({ vector: [0.1, 0.2, 0.3], text: 'neural networks', topK: 5, alpha: 0.3 });

// Favor dense similarity
await idx.queryHybrid({ vector: [0.1, 0.2, 0.3], text: 'neural networks', topK: 5, alpha: 0.8 });
```

Hybrid search also supports metadata filters:

```typescript
const results = await idx.queryHybrid({
  vector: [0.1, 0.2, 0.3],
  text: 'machine learning',
  topK: 10,
  alpha: 0.5,
  filter: { category: { $eq: 'tech' } },
  includeMetadata: true,
});
```

### Reranking

Apply a reranker to re-score results using a natural language query. Works with both `query` and `queryHybrid`.

```typescript
// Rerank on a dense query
const results = await idx.query({
  vector: [0.1, 0.2, 0.3],
  topK: 20,
  rerank: {
    query: 'What are the fundamentals of machine learning?',
    topN: 5,          // Return top 5 after reranking (default: topK)
    rankField: 'text', // Metadata field to rank against (default: "text")
  },
  includeMetadata: true,
});

// Rerank on a hybrid query
const hybridResults = await idx.queryHybrid({
  vector: [0.1, 0.2, 0.3],
  text: 'machine learning',
  topK: 20,
  alpha: 0.5,
  rerank: {
    query: 'What are the fundamentals of machine learning?',
    topN: 5,
  },
  includeMetadata: true,
});
```

Rerank options:

| Option | Type | Description |
|--------|------|-------------|
| `query` | `string` | Natural language query for the reranker (required) |
| `topN` | `number` | Number of results to return after reranking (defaults to `topK`) |
| `rankField` | `string` | Metadata field containing text to rank against (defaults to `"text"`) |
| `model` | `string` | Per-request reranker model override |

## Namespaces

All data operations support an optional `namespace` parameter. The default namespace is `""` (empty string).

```typescript
const idx = pc.index('my-index');

// Upsert into a namespace
await idx.upsert({
  vectors: [{ id: 'vec-1', values: [0.1, 0.2, 0.3] }],
  namespace: 'articles',
});

// Query within a namespace
const results = await idx.query({ vector: [0.1, 0.2, 0.3], topK: 5, namespace: 'articles' });

// Fetch from a namespace
const fetched = await idx.fetch({ ids: ['vec-1'], namespace: 'articles' });

// List vectors in a namespace
const listed = await idx.list({ namespace: 'articles' });

// Delete within a namespace
await idx.delete({ ids: ['vec-1'], namespace: 'articles' });

// Delete all vectors in a namespace
await idx.delete({ deleteAll: true, namespace: 'articles' });

// View per-namespace stats
const stats = await idx.describeIndexStats();
for (const [nsName, nsSummary] of Object.entries(stats.namespaces)) {
  console.log(`Namespace '${nsName}': ${nsSummary.vectorCount} vectors`);
}
```

## Metadata Filtering Reference

Metadata filters can be used with `query`, `queryHybrid`, `fetchByMetadata`, and `delete`.

### Comparison Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `$eq` | Equal to | `{ status: { $eq: 'active' } }` |
| `$ne` | Not equal to | `{ status: { $ne: 'deleted' } }` |
| `$gt` | Greater than | `{ price: { $gt: 100 } }` |
| `$gte` | Greater than or equal | `{ price: { $gte: 100 } }` |
| `$lt` | Less than | `{ price: { $lt: 50 } }` |
| `$lte` | Less than or equal | `{ price: { $lte: 50 } }` |
| `$in` | In set | `{ color: { $in: ['red', 'blue'] } }` |
| `$nin` | Not in set | `{ color: { $nin: ['green'] } }` |

### Logical Operators

```typescript
// AND: all conditions must match
const andFilter = {
  $and: [
    { genre: { $eq: 'sci-fi' } },
    { year: { $gte: 2024 } },
  ],
};

// OR: at least one condition must match
const orFilter = {
  $or: [
    { genre: { $eq: 'sci-fi' } },
    { genre: { $eq: 'fantasy' } },
  ],
};

// Combine AND and OR
const combinedFilter = {
  $and: [
    { year: { $gte: 2020 } },
    {
      $or: [
        { genre: { $eq: 'sci-fi' } },
        { genre: { $eq: 'fantasy' } },
      ],
    },
  ],
};
```

### Nested Fields (Dot Notation)

```typescript
const filter = { 'address.city': { $eq: 'San Francisco' } };
const filter2 = { 'user.role': { $in: ['admin', 'editor'] } };
```

## Error Handling

```typescript
import { Onecortex, OnecortexHttpError } from '@onecortex/vector';

try {
  await pc.describeIndex('nonexistent');
} catch (e) {
  if (e instanceof OnecortexHttpError) {
    console.error(`Error ${e.statusCode} [${e.code}]: ${e.message}`);
    // e.statusCode: HTTP status code (404, 409, 400, 401, 403, 500, etc.)
    // e.code: "NOT_FOUND", "ALREADY_EXISTS", "INVALID_ARGUMENT",
    //         "UNAUTHENTICATED", "PERMISSION_DENIED", "UNKNOWN"
  }
}
```

Common error scenarios:

```typescript
// Index not found (404)
try {
  await pc.describeIndex('nonexistent');
} catch (e) {
  if (e instanceof OnecortexHttpError && e.statusCode === 404) {
    console.log('Index does not exist');
  }
}

// Index already exists (409)
try {
  await pc.createIndex({ name: 'my-index', dimension: 3 });
} catch (e) {
  if (e instanceof OnecortexHttpError && e.code === 'ALREADY_EXISTS') {
    console.log('Index already exists, continuing...');
  }
}

// Deletion protection (403)
try {
  await pc.deleteIndex('protected-index');
} catch (e) {
  if (e instanceof OnecortexHttpError && e.statusCode === 403) {
    console.log('Disable deletion protection first');
  }
}
```

## Auto-Retry Behavior

The SDK automatically retries requests on:

- **HTTP 429** (rate limited)
- **HTTP 5xx** (server errors)

Retries use exponential backoff with delays of 1s, 2s, and 4s (up to 3 retries). No configuration is needed.

## Drop-in Replacement

If you are migrating from another vector database SDK with the same API shape, switching to Onecortex requires only changing the import and client initialization:

```typescript
// Before
// import { Pinecone } from '@pinecone-database/pinecone';
// const pc = new Pinecone({ apiKey: '...' });

// After
import { Onecortex } from '@onecortex/vector';
const pc = new Onecortex({ apiKey: 'your-onecortex-key', host: 'http://your-server:8080' });
```

The following parameters are accepted and silently ignored for compatibility:
- `spec` on `createIndex()` (e.g., `spec: { serverless: { cloud: 'aws', region: 'us-east-1' } }`)
- `sparseValues` on upsert vectors (sparse vectors are not stored)

## API Reference

### Client Methods

| Method | Description |
|--------|-------------|
| `new Onecortex({ apiKey, host })` | Create a client |
| `createIndex({ name, dimension, metric, bm25_enabled, deletion_protection, tags })` | Create a new index |
| `describeIndex(name)` | Get index metadata |
| `listIndexes()` | List all indexes |
| `configureIndex(name, { deletion_protection, tags })` | Update index settings |
| `deleteIndex(name)` | Delete an index |
| `index(name)` | Get an index handle for data operations |

### Index Methods

| Method | Description |
|--------|-------------|
| `upsert({ vectors, namespace })` | Insert or update vectors |
| `fetch({ ids, namespace })` | Fetch vectors by ID |
| `fetchByMetadata({ filter, namespace, limit, includeValues, includeMetadata })` | Fetch vectors matching a metadata filter |
| `delete({ ids, filter, deleteAll, namespace })` | Delete vectors |
| `update({ id, values, setMetadata, text, namespace })` | Update a vector |
| `query({ vector, topK, namespace, filter, includeValues, includeMetadata, id, rerank })` | Dense ANN search |
| `queryHybrid({ vector, text, topK, alpha, namespace, filter, includeMetadata, includeValues, rerank })` | Hybrid dense + BM25 search |
| `list({ namespace, prefix, limit, paginationToken })` | List vector IDs |
| `describeIndexStats()` | Get index statistics |
