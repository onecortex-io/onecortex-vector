# Deployment

## Standalone (local development)

The simplest setup — Postgres in Docker, the API direct on `localhost:8080`,
no auth.

```bash
docker compose up -d postgres
cargo run
# API:    http://localhost:8080
# Admin:  http://localhost:9090
```

Migrations are applied automatically on startup.

## Production (Onecortex platform)

In production Onecortex Vector runs behind the Onecortex platform's
APISIX gateway, which handles JWT validation. The vector service
itself has no auth layer.

- Authentication is handled by the gateway using JWTs issued by
  [onecortex-auth](https://github.com/onecortex-io/onecortex-auth).
- Clients send `Authorization: Bearer <jwt>` to the gateway.
- The vector API is reachable at `http://<host>/vector/v1/...`.

To bring up the full platform:

```bash
# From the org root (onecortex-io/)
docker compose up -d
```

## Production (standalone behind your own gateway)

If you're not using the Onecortex platform, put your own gateway or
reverse proxy in front of the service and terminate auth there. The
service trusts whatever reaches it on the public port.

- Run Postgres on its own host (or a managed service: RDS, Cloud SQL,
  Supabase, Neon — anything with `pgvector` + `pgvectorscale` +
  `pg_textsearch` available).
- Run `onecortex-vector` as a container or systemd unit, pointing at
  the database via `DATABASE_URL`.
- Expose `:8080` only to your gateway; keep `:9090` (admin / metrics)
  on a private network.

The published Docker image is tagged on each release.

## Backups, replication, IAM

There is nothing Onecortex Vector-specific to do here. Whatever you
already do for Postgres applies — `pg_dump`, `pgBackRest`, streaming
replication, IAM policies on the database. The catalog tables live in
the `_onecortex_vector` schema and user data in the `_onecortex`
schema; both are included in standard logical / physical backups.

## Migrations

Schema migrations run automatically on startup against the
`_onecortex_vector._sqlx_migrations` table. Roll forward by deploying
a newer image; roll back by restoring from a backup taken before the
upgrade.
