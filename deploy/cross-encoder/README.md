# Cross-Encoder Reranker

Self-hosted reranking using HuggingFace Text Embeddings Inference (TEI).

## Default model

`BAAI/bge-reranker-v2-m3` — multilingual, 8,192-token context window.
Strong performance across English and 100+ other languages. ~280MB.

## Alternative models

Edit `docker-compose.yml` and replace the `--model-id` flag:

| Model | Size | Context | Notes |
|---|---|---|---|
| `BAAI/bge-reranker-v2-m3` | ~280MB | 8K | **Default** — multilingual, best balance |
| `BAAI/bge-reranker-large` | ~1.3GB | 512 | English-focused, very high quality |
| `BAAI/bge-reranker-base` | ~280MB | 512 | English-focused, faster |
| `cross-encoder/ms-marco-MiniLM-L-6-v2` | ~90MB | 512 | Tiny, fastest, English only |
| `cross-encoder/ms-marco-MiniLM-L-12-v2` | ~130MB | 512 | Larger, better quality, English only |

## GPU support

Replace the image tag with `ghcr.io/huggingface/text-embeddings-inference:latest`
and add `deploy.resources.reservations.devices` (NVIDIA GPU) to the service in
docker-compose.yml.

## API contract

```
POST /rerank
Content-Type: application/json

Request:
  {
    "query": "search query text",
    "texts": ["candidate 1", "candidate 2"],
    "truncate": true
  }

Response:
  [
    { "index": 0, "score": 0.98 },
    { "index": 1, "score": 0.31 }
  ]
```

Results are NOT guaranteed to be sorted — `CrossEncoderReranker` sorts and truncates.
Scores are raw logits (not calibrated probabilities). Higher is more relevant.
