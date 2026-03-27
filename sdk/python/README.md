# onecortex-vector

Python client for [Onecortex Vector](https://github.com/onecortex-io/onecortex-vector) — a self-hosted vector database.

## Installation

```bash
pip install onecortex-vector
```

## Quick Start

```python
from onecortex import Onecortex

pc = Onecortex(api_key="your-api-key", host="http://localhost:8080")

# Create an index
pc.create_index(name="my-index", dimension=1536, metric="cosine")

# Upsert vectors
idx = pc.Index("my-index")
idx.upsert(vectors=[
    {"id": "v1", "values": [0.1, 0.2, ...], "metadata": {"text": "hello world"}},
])

# Query
results = idx.query(vector=[0.1, 0.2, ...], top_k=10, include_metadata=True)
for match in results.matches:
    print(match.id, match.score)
```

## Drop-in Replacement

If you are already using a vector database SDK with the same API shape, switching to Onecortex requires only 2 lines:

```python
from onecortex import Onecortex
pc = Onecortex(api_key="your-onecortex-key", host="http://your-server:8080")
```

All other method calls remain identical.
