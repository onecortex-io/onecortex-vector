# Reranking

Add a `rerank` object to any query to re-score results against a
natural-language query. Reranking runs after the initial ANN/hybrid
fetch and before the final `topK` truncation.

```bash
curl -X POST http://localhost:8080/v1/collections/docs/query \
  -H "Content-Type: application/json" \
  -d '{
    "vector": [0.1, 0.2, 0.3],
    "topK": 10,
    "rerank": {
      "query":     "machine learning fundamentals",
      "topN":      3,
      "rankField": "text"
    }
  }'
```

`rankField` names the metadata field whose text the reranker should
score. If the field is absent on a record, the reranker falls back to
the record id.

## Backends

All backends are optional and configured via
`ONECORTEX_VECTOR_RERANK_BACKEND`. Switch backends with one env var —
no code or schema changes required.

| Backend | Value | Required env |
|---------|-------|--------------|
| None (default) | `none` | — |
| Cohere | `cohere` | `ONECORTEX_VECTOR_RERANK_COHERE_API_KEY` |
| Voyage AI | `voyage` | `ONECORTEX_VECTOR_RERANK_VOYAGE_API_KEY` |
| Jina AI | `jina` | `ONECORTEX_VECTOR_RERANK_JINA_API_KEY` |
| Pinecone Inference | `pinecone` | `ONECORTEX_VECTOR_RERANK_PINECONE_API_KEY` |
| Self-hosted cross-encoder | `cross-encoder` | `ONECORTEX_VECTOR_RERANK_CROSS_ENCODER_URL` |

To start the optional self-hosted cross-encoder service:

```bash
docker compose --profile reranking up -d
```

## Failure modes

Reranker failures map to specific HTTP statuses so clients can pick the
right retry strategy:

| Code | HTTP | When |
|---|---|---|
| `RERANKER_RATE_LIMITED` | 429 | Upstream returned 429 after the retry budget |
| `RERANKER_TIMEOUT` | 504 | Upstream did not respond before the deadline |
| `RERANKER_UPSTREAM` | 502 | Upstream returned a non-2xx, refused to connect, or returned an unparseable body |
| `RERANKER_CONFIG` | 503 | Reranker is misconfigured (missing api key, etc.) |

See [errors.md](api-reference/errors.md) for the full retry guidance.
