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

Non-masters receive `403` with `{"code":"NOT_MASTER"}`.

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

### Live Flow Checklist

1. **Lobby** – send `character_select` to lock in cosmetics and `ready_set` to toggle ready state.
2. **Start** – only the room master may `POST /start` or send `start_request`; the `start` payload includes the entire player roster (id/name/role/characterId).
3. **Countdown** – `start_countdown` announces the authoritative start time.
4. **Running** – on `start`, the server immediately broadcasts a **full** `snapshot` (all alive players, incl. characterId) to every connection. Full snapshots repeat at least once per second and whenever a client reconnects.
5. **Reconnect** – open a new WS, send `reconnect` with the last `ackTick` and `resumeToken`, and expect `welcome` + `lobby_state` followed by a full snapshot before deltas resume.

Run tests with `make test`.
