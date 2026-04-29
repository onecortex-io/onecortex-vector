# Configuration

All environment variables use the `ONECORTEX_VECTOR_` prefix. Copy
`.env.example` for the full list with documentation.

## Core

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | (required) | PostgreSQL connection string |
| `API_PORT` | `8080` | Public API port |
| `ADMIN_PORT` | `9090` | Admin / metrics port |
| `MAX_CONNS` | `50` | Max database pool connections |
| `LOG_LEVEL` | `info` | Log level (`trace`/`debug`/`info`/`warn`/`error`) |
| `ENABLE_RLS` | `false` | Enable row-level security for namespace isolation |

## Reranking

| Variable | Default | Description |
|----------|---------|-------------|
| `RERANK_BACKEND` | `none` | One of `none`, `cohere`, `voyage`, `jina`, `pinecone`, `cross-encoder` |
| `RERANK_COHERE_API_KEY` | — | Required when backend is `cohere` |
| `RERANK_VOYAGE_API_KEY` | — | Required when backend is `voyage` |
| `RERANK_JINA_API_KEY` | — | Required when backend is `jina` |
| `RERANK_PINECONE_API_KEY` | — | Required when backend is `pinecone` |
| `RERANK_CROSS_ENCODER_URL` | — | Required when backend is `cross-encoder` |

See [reranking.md](reranking.md) for backend selection guidance.

## Deployment

When run behind the Onecortex platform's APISIX gateway, the gateway
handles JWT validation; the vector service has no auth layer of its
own. See [deployment.md](deployment.md) for production guidance.
