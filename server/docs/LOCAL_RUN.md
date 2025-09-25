# Local Development

1. Copy `.env.example` or use the provided `.env` to set ports and JWT secret.
2. Start the HTTP + WebSocket server:

```bash
make up
```

The REST API listens on `http://localhost:8080`, and the WebSocket endpoint is `ws://localhost:8081/v1/ws`.

## REST Examples

Create a room:

```bash
curl -X POST http://localhost:8080/v1/rooms \
  -H 'Content-Type: application/json' \
  -d '{"name":"host","region":"local-dev","maxPlayers":4}'
```

Join a room:

```bash
curl -X POST http://localhost:8080/v1/rooms/<ROOM_ID>/join \
  -H 'Content-Type: application/json' \
  -d '{"name":"guest"}'
```

Start a match (master only):

```bash
curl -X POST http://localhost:8080/v1/rooms/<ROOM_ID>/start \
  -H 'Content-Type: application/json' \
  -d '{"playerId":"<MASTER_ID>","countdownSec":3}'
```

Toggle ready:

```bash
curl -X POST http://localhost:8080/v1/rooms/<ROOM_ID>/ready \
  -H 'Content-Type: application/json' \
  -d '{"playerId":"<PLAYER_ID>","ready":true}'
```

## WebSocket Testing

Use `websocat` or `wscat` with the `wsToken` returned from REST:

```bash
websocat ws://localhost:8081/v1/ws?token=<wsToken>
```

Immediately send a `join` payload:

```json
{"type":"join","pv":1,"seq":1,"ts":1,"payload":{"name":"tester"}}
```

The server responds with `welcome`, `lobby_state`, and later lobby or simulation updates.

Run tests with `make test`.
