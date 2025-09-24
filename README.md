# Game Server

This repository contains a simplified realtime game server skeleton implemented in Rust. It demonstrates:

- REST bootstrap flows using Axum
- Deterministic simulation primitives for a vertical jumper game
- Token based authentication for WebSocket bootstrap
- Rate limiting, backpressure, and metrics utilities

## Running locally

```bash
cargo run -p server
```

Configuration defaults live in `configs/server.example.toml`. Override values via environment variables listed in `server/src/config.rs`.

## Development workflow

- `make build`
- `make test`
- `make bench`
- `make lint`
- `make fmt`

## Example REST flow

```bash
curl -X POST http://localhost:3000/v1/rooms
```

The response includes the room id, seed, and a WebSocket token for connecting to `/v1/ws`.
