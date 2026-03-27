# @onecortex/vector

TypeScript client for [Onecortex Vector](https://github.com/onecortex-io/onecortex-vector) — a self-hosted vector database.

## Installation

```bash
npm install @onecortex/vector
```

## Quick Start

```typescript
import { Onecortex } from '@onecortex/vector';

const pc = new Onecortex({ apiKey: 'your-api-key', host: 'http://localhost:8080' });

// Create an index
await pc.createIndex({ name: 'my-index', dimension: 1536, metric: 'cosine' });

// Upsert vectors
const idx = pc.index('my-index');
await idx.upsert({
  vectors: [{ id: 'v1', values: [0.1, 0.2, ...], metadata: { text: 'hello world' } }],
});

// Query
const results = await idx.query({ vector: [0.1, 0.2, ...], topK: 10 });
for (const match of results.matches) {
  console.log(match.id, match.score);
}
```

## Drop-in Replacement

If you are already using a vector database SDK with the same API shape, switching to Onecortex requires only 2 lines:

```typescript
import { Onecortex } from '@onecortex/vector';
const pc = new Onecortex({ apiKey: 'your-onecortex-key', host: 'http://your-server:8080' });
```

All other method calls remain identical.
