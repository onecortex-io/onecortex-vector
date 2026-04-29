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
array field matches a sub-filter object:

```json
{ "tags": { "$elemMatch": { "type": "premium" } } }
```

## Errors

Malformed or unsupported filters return 400 with a typed code:

- `FILTER_MALFORMED` — structurally invalid (e.g. `$and` value is not
  an array, a comparison expects a number and got a string).
- `FILTER_UNSUPPORTED_OPERATOR` — the filter uses an operator this
  server does not implement.

See [errors.md](api-reference/errors.md) for the full envelope and
`details` schema.
