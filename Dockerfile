FROM rust:1.79-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p server

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/server /usr/local/bin/server
CMD ["/usr/local/bin/server"]
