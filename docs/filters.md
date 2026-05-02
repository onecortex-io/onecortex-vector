# Metadata filtering

Every endpoint that accepts a `filter` parameter uses the same JSON DSL.
Filters apply to the `metadata` JSONB column of each record.

## Operators

### Equality and comparison

```json
{ "category":   { "$eq":  "news" } }
{ "category":   { "$ne":  "news" } }
{ "score":      { "$gt":  0.5,  "$lte": 1.0 } }
{ "tag":        { "$in":  ["a", "b"] } }
{ "tag":        { "$nin": ["c"] } }
```

### Boolean composition

```json
{ "$and": [
    { "category": { "$eq": "news" } },
    { "score":    { "$gt": 0.5 } }
] }

{ "$or": [
    { "category": { "$eq": "news" } },
    { "category": { "$eq": "blog" } }
] }
```

### Datetime ranges

ISO 8601 strings work directly with `$gt`/`$gte`/`$lt`/`$lte` — no
epoch conversion needed:

```json
{ "createdAt": { "$gte": "2025-01-01T00:00:00Z" } }
{ "updatedAt": { "$lte": "2025-12-31T23:59:59Z" } }
```

### Geo radius

Filter records within a distance from a lat/lon point. Records must
have a metadata field with `lat`/`lon` sub-keys:

```json
{ "location": {
    "$geoRadius": { "lat": 40.7, "lon": -74.0, "radiusMeters": 5000 }
} }
```

### Geo bounding box

```json
{ "location": {
    "$geoBBox": {
      "minLat": 40.0, "maxLat": 41.5,
      "minLon": -75.0, "maxLon": -73.0
    }
} }
```

### Array element matching

`$elemMatch` filters records where at least one element in a metadata
array field matches a sub-filter object. Use it for **arrays of objects**:

```json
{ "tags": { "$elemMatch": { "type": "premium" } } }
```

### Array contains (scalar elements)

For metadata fields whose value is an **array of scalars** (strings,
numbers, or booleans) — e.g. `tags`, `authors`, `categories`,
`tenant_ids` — use the `$contains` family:

| Operator | Operand | Semantics |
|---|---|---|
| `$contains` | scalar | array contains the given value |
| `$containsAny` | array of scalars | array intersects the given list (OR) |
| `$containsAll` | array of scalars | array is a superset of the given list (AND) |

```json
{ "authors":  { "$contains": "Cortex Team" } }
{ "authors":  { "$containsAny": ["Cortex Team", "Lewis"] } }
{ "authors":  { "$containsAll": ["Smith", "Johnson"] } }
```

Notes:

- The operand must be a JSON scalar (or array of scalars). Nested
  objects/arrays are rejected with `FILTER_MALFORMED` — use
  `$elemMatch` for arrays of objects.
- `$containsAny` / `$containsAll` reject empty arrays.
- These operators are distinct from `$in` / `$nin`, which test whether
  a **scalar** field equals one of a list of values. `$in` does not
  inspect array contents.

## Errors

Malformed or unsupported filters return 400 with a typed code:

- `FILTER_MALFORMED` — structurally invalid (e.g. `$and` value is not
  an array, a comparison expects a number and got a string).
- `FILTER_UNSUPPORTED_OPERATOR` — the filter uses an operator this
  server does not implement.

See [errors.md](api-reference/errors.md) for the full envelope and
`details` schema.
