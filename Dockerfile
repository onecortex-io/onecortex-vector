# =============================================================================
# Onecortex Vector — Multi-stage Dockerfile
#
# Runtime: gcr.io/distroless/cc-debian12 (glibc, no shell, estimated size ~45 MB
#          plus the compiled binary)
#
# Build:
#   docker build -t onecortex-vector:latest .
#
# Run:
#   docker run \
#     -e ONECORTEX_VECTOR_DATABASE_URL=postgres://user:pass@host:5432/db \
#     -p 8080:8080 \
#     -p 9090:9090 \
#     onecortex-vector:latest
# =============================================================================

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS builder

WORKDIR /app

# Copy manifests first so dependency layer is cached separately from source.
COPY Cargo.toml Cargo.lock ./

# Fetch dependencies in a dedicated layer unless manifest files change.
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo fetch \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations

RUN cargo build --release

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
# cc-debian12 includes glibc, libgcc, and CA certificates only.
FROM gcr.io/distroless/cc-debian12

COPY --from=builder /app/target/release/onecortex-vector /usr/local/bin/onecortex-vector
COPY --from=builder /app/migrations /migrations

EXPOSE 8080 9090

ENTRYPOINT ["/usr/local/bin/onecortex-vector"]
