# Changelog

All notable changes to onecortex-vector are documented here. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] — Unreleased

### Added

- **`$contains`, `$containsAny`, `$containsAll` metadata filter
  operators** for fields whose value is an array of scalars (strings,
  numbers, booleans). Closes a gap that previously forced users to
  denormalize tags/authors/labels into delimited strings or filter
  client-side. `$contains` takes a scalar; `$containsAny` and
  `$containsAll` take a non-empty array of scalars and reject nested
  objects/arrays. `$elemMatch` remains the operator for arrays of
  objects. See `docs/filters.md` for the full DSL.

## [0.2.1] — 2026-05-01

### Fixed

- **Upsert no longer 500s on duplicate ids within a batch.** A request
  whose `records[]` contained the same `id` more than once would trip
  Postgres `ON CONFLICT DO UPDATE command cannot affect row a second
  time` and surface as `500 INTERNAL`. The handler now deduplicates
  records by `id` before issuing the SQL, keeping the **last**
  occurrence (last-write-wins). `upsertedCount` reflects the number of
  distinct ids actually written, and the server emits an `info`-level
  log line with `batch_size`, `unique_ids`, and `collapsed` whenever
  duplicates are collapsed.

### Changed

- Documentation: `CLAUDE.md` corrected to state that `sparseValues` is
  rejected with 400 `SPARSE_NOT_SUPPORTED` (the prior "silently dropped
  + WARN logged" wording was stale post-0.2.0).
- `docs/api-reference.md` documents the upsert duplicate-id contract.

## [0.2.0] — 2026-04-29

### Breaking changes

- **`sparseValues` on upsert is now rejected.** Previously the field was
  silently dropped with a WARN log; the request returned 200. It now
  returns 400 `SPARSE_NOT_SUPPORTED` with the offending `recordId` in
  `details`.
- **Reranker upstream failures are split by HTTP status.** Previously
  every reranker error mapped to 500 `INTERNAL`. They now map to:
  - 429 `RERANKER_RATE_LIMITED`
  - 502 `RERANKER_UPSTREAM` (non-2xx response, connect failure, parse
    failure)
  - 503 `RERANKER_CONFIG` (missing API key, etc.)
  - 504 `RERANKER_TIMEOUT`
- **`groupBy` with a missing field now errors.** A query whose `groupBy`
  field is absent on every matched record previously returned a single
  empty-keyed group. It now returns 400 `GROUPBY_FIELD_MISSING`.
- **Indexing-in-progress collections return 409 `INDEX_NOT_READY`.**
  Previously a non-`ready` collection collapsed to 404 `NOT_FOUND`.
  Genuine misses still return 404 `COLLECTION_NOT_FOUND`.
- **`COLLECTION_ALREADY_EXISTS` replaces `ALREADY_EXISTS` on the
  collection-create unique-violation path.** Status (409) is unchanged.
- **`HYBRID_REQUIRES_BM25` replaces `INVALID_ARGUMENT` on hybrid queries
  against non-BM25 collections.** Status (400) is unchanged.

### Added

- Stable, machine-readable error codes covering every recurring failure
  mode: `DIMENSION_MISMATCH`, `SPARSE_NOT_SUPPORTED`, `FILTER_MALFORMED`,
  `FILTER_UNSUPPORTED_OPERATOR`, `HYBRID_REQUIRES_BM25`,
  `GROUPBY_FIELD_MISSING`, `FACET_FIELD_INVALID`, `INDEX_NOT_READY`,
  `COLLECTION_NOT_FOUND`, `COLLECTION_ALREADY_EXISTS`,
  `RERANKER_RATE_LIMITED`, `RERANKER_TIMEOUT`, `RERANKER_CONFIG`,
  `RERANKER_UPSTREAM`. See README → Errors for the full table.
- Structured `details` on every error body. New fields include
  `recordId`, `expected`, `got`, `field`, `operator`, `collection`,
  `status`, `upstreamStatus`, and `requestId`.
- `X-Request-Id` response header on every response. The server assigns
  a UUID v4 if the client did not supply one and otherwise echoes the
  client value. The same id appears in `error.details.requestId` and as
  the `request_id` field on the corresponding tracing span, so a single
  identifier correlates the client report, the response body, and the
  server log.
- Query-vector dimension validation: query, hybrid-query, and recommend
  endpoints now reject vectors of the wrong length up front with 400
  `DIMENSION_MISMATCH` instead of bubbling a confusing pgvector error.

### Notes for downstream

The Python (`onecortex`) and TypeScript (`@onecortex/sdk`) clients should
mirror this change in the same release window:

- Add typed exceptions for each new code on top of the existing
  `OnecortexError` / `OnecortexHttpError` hierarchies.
- Capture `X-Request-Id` from response headers on every request and
  expose it as `error.request_id` / `error.requestId` on thrown errors.
- Update retry policy: `RERANKER_RATE_LIMITED` (429) is retryable with
  backoff; `RERANKER_TIMEOUT` (504) is retryable with caution;
  `RERANKER_UPSTREAM` (502) and `RERANKER_CONFIG` (503) are not.

Run `/sync-clients` from the org root to coordinate the SDK changes.

## [0.1.0]

Initial release.
