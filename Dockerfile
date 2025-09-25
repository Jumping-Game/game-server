# syntax=docker/dockerfile:1.6

FROM rust:1.79-slim AS builder

WORKDIR /app

# Install build tooling that is not bundled with the slim image
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates binutils \
    && rm -rf /var/lib/apt/lists/*

# Pre-fetch dependencies so Docker can cache them between builds
COPY Cargo.toml Cargo.lock ./
COPY server/Cargo.toml server/Cargo.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo fetch -p server --locked

# Copy the actual source last so dependency layers stay cached
COPY server ./server

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p server --locked \
    && strip target/release/server

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/server /usr/local/bin/server

USER nonroot

ENTRYPOINT ["/usr/local/bin/server"]
