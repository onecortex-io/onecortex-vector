# Errors & request ids

Every error response uses the same envelope:

```json
{
  "status": 400,
  "error": {
    "code": "DIMENSION_MISMATCH",
    "message": "record 'r42' has 768 dimensions but collection expects 1536",
    "details": {
      "recordId": "r42",
      "expected": 1536,
      "got": 768,
      "requestId": "0f7c3a90-bb23-4e2c-9aa1-2b6f7d8a9d1c"
    }
  }
}
```

`code` is a stable, machine-readable identifier — SDKs build typed
exception hierarchies on top of it. `details` carries structured
context that's specific to the failure mode, plus a `requestId` that
matches the `X-Request-Id` response header.

## Code reference

### 400 Bad Request

| Code | Meaning | `details` |
|---|---|---|
| `DIMENSION_MISMATCH` | Vector length does not match the collection's `dimension`. | `{ recordId?, expected, got }` |
| `SPARSE_NOT_SUPPORTED` | Request includes `sparseValues`; this server does not support sparse vectors. | `{ recordId }` |
| `FILTER_MALFORMED` | Filter DSL is structurally invalid. | `{ reason }` |
| `FILTER_UNSUPPORTED_OPERATOR` | Filter uses an operator the server does not implement. | `{ operator }` |
| `HYBRID_REQUIRES_BM25` | Hybrid search requested on a collection without BM25 enabled. | `{ collection }` |
| `GROUPBY_FIELD_MISSING` | The `groupBy.field` was not present on any matched record. | `{ field }` |
| `FACET_FIELD_INVALID` | Facet field name is empty, too long, or has invalid characters. | `{ field, reason }` |
| `INVALID_ARGUMENT` | Generic validation failure (catch-all for messages without a more specific code). | — |

### 403 Forbidden

| Code | Meaning |
|---|---|
| `PERMISSION_DENIED` | The caller is not authorised for this action. |

### 404 Not Found

| Code | Meaning | `details` |
|---|---|---|
| `COLLECTION_NOT_FOUND` | Named collection (or alias target) does not exist. | `{ collection }` |
| `NOT_FOUND` | A non-collection resource (record, alias, namespace) was not found. | — |

### 409 Conflict

| Code | Meaning | `details` |
|---|---|---|
| `COLLECTION_ALREADY_EXISTS` | A collection with the given name already exists. | `{ collection }` |
| `INDEX_NOT_READY` | Collection exists but is not in `ready` state yet — retry shortly. | `{ collection, status }` |
| `ALREADY_EXISTS` | Generic conflict for non-collection resources. | — |

### 429 / 5xx (upstream)

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `RERANKER_RATE_LIMITED` | 429 | Reranker upstream returned 429 after the configured retry budget. | `{ retries }` |
| `RERANKER_UPSTREAM` | 502 | Reranker upstream returned a non-2xx response, refused to connect, or returned an unparseable body. | `{ kind, upstreamStatus? }` |
| `RERANKER_CONFIG` | 503 | Reranker is misconfigured (missing api key, invalid model, etc.). | — |
| `RERANKER_TIMEOUT` | 504 | Reranker upstream did not respond before the deadline. | — |
| `INTERNAL` | 500 | Unhandled server error — `requestId` lets operators correlate with logs. | — |

## Request ids

Every response carries an `X-Request-Id` header. If the client supplies
the header on the request, the server echoes it; otherwise the server
assigns a UUID v4. The same id appears in `error.details.requestId` on
every error body and as the `request_id` field on the corresponding
server log span — so a single token correlates the client report, the
response body, and the server logs.

```bash
$ curl -i -X POST http://localhost:8080/v1/collections/docs/query \
    -H "X-Request-Id: my-trace-abc123" \
    -d '{"vector":[0.1,0.2],"topK":5}'

HTTP/1.1 400 Bad Request
x-request-id: my-trace-abc123
content-type: application/json

{
  "status": 400,
  "error": {
    "code": "DIMENSION_MISMATCH",
    "message": "vector has 2 dimensions but collection expects 1536",
    "details": { "expected": 1536, "got": 2, "requestId": "my-trace-abc123" }
  }
}
```

Use any opaque string you like — request ids are propagated as-is so
they can join an upstream tracing system (W3C `traceparent`, an
internal correlation id, etc.).

## Retry guidance

Use the HTTP status family, not the message text:

- **429 `RERANKER_RATE_LIMITED`** — retry with exponential backoff. The
  `details.retries` field is the number of attempts the server already
  made before giving up.
- **502 `RERANKER_UPSTREAM`** — retry once or twice; the upstream may
  be flapping.
- **503 `RERANKER_CONFIG`** — do not retry. A human needs to fix the
  reranker config.
- **504 `RERANKER_TIMEOUT`** — retry with caution. The upstream may
  still be processing your previous request.
- **5xx `INTERNAL`** — retry once and then escalate with the
  `requestId`.
- **4xx everything else** — do not retry; fix the request.
