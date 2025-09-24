# NETWORK_PROTOCOL.md

**Project:** Vertical Jumper (Android-first)
**Scope:** Client ↔ Server networking for realtime gameplay and supporting APIs
**Audience:** Android game devs, Game server devs
**Status:** Draft v1.0 (compatible with Client Protocol `pv=1`)

---

## Table of Contents

* [1. Overview](#1-overview)
* [2. Transport & Environment](#2-transport--environment)
* [3. Versioning](#3-versioning)
* [4. Data Types & Conventions](#4-data-types--conventions)
* [5. REST APIs (Matchmaking & Metadata)](#5-rest-apis-matchmaking--metadata)
* [6. WebSocket Lifecycle](#6-websocket-lifecycle)
* [7. Message Envelope](#7-message-envelope)
* [8. Client → Server Messages](#8-client--server-messages)
* [9. Server → Client Messages](#9-server--client-messages)
* [10. Timing, Ticks & Reconciliation](#10-timing-ticks--reconciliation)
* [11. Reliability, Acknowledgments & Rate Limits](#11-reliability-acknowledgments--rate-limits)
* [12. Error Codes](#12-error-codes)
* [13. Reconnect Flow](#13-reconnect-flow)
* [14. Anti-Cheat & Validation](#14-anti-cheat--validation)
* [15. Security](#15-security)
* [16. Compression & Payload Size Targets](#16-compression--payload-size-targets)
* [17. Examples](#17-examples)
* [18. Test Matrix (Interoperability)](#18-test-matrix-interoperability)
* [19. Change Log](#19-change-log)

---

## 1. Overview

This document defines the **client/server protocol** for our Doodle-Jump–style vertical jumper with optional realtime multiplayer. The design uses:

* **REST** for lightweight matchmaking & metadata.
* **WebSocket over TLS (WSS)** for realtime game data (inputs/snapshots).
* **Deterministic simulation** (60 Hz fixed timestep) with **client prediction + server reconciliation**.

---

## 2. Transport & Environment

**Realtime:**

* Protocol: **WebSocket** over **TLS** (`wss://`)
* Framing: Text frames carrying **JSON** (compact models; consider binary in a later version)

**REST:**

* Protocol: **HTTPS** (`https://`)
* Auth: None for MVP (names only). Future: short-lived JWT from an auth service.

**Regions:**

* Server advertises POP/region in `GET /status`. Client picks nearest.

---

## 3. Versioning

* **Protocol version (pv)**: integer, increments on breaking changes.
* Every envelope includes: `type`, `pv`, and `ts` (client/server timestamp ms).
* Server rejects mismatched `pv` with `S2C_Error{ code: "BAD_VERSION" }`.

Current: `pv = 1`.

---

## 4. Data Types & Conventions

* **Tick**: 60 Hz discrete step (`tick = 0,1,2,...`).
* **Coordinates**: Float pixels in **logical world units** (origin bottom-left, **up is +Y**).
* **Player ID**: string (UUID or server-issued short ID).
* **Room ID**: string (short, e.g., base36).
* **Seed**: 64-bit signed integer (`Long`), identical across room.
* **Time**: Unix epoch milliseconds (`number`).

Field naming: `camelCase`.

---

## 5. REST APIs (Matchmaking & Metadata)

Base URL: `https://api.example.com`

### 5.1 Create Room

```
POST /v1/rooms
Content-Type: application/json
{
  "name": "bene",
  "region": "ap-southeast-1",
  "maxPlayers": 4,
  "mode": "endless"        // future: "race"
}
→ 201
{
  "roomId": "ab12",
  "seed": 7392012341,
  "region": "ap-southeast-1",
  "wsUrl": "wss://rt.apse1.example.com/v1/ws/ab12"
}
```

### 5.2 Join Room

```
POST /v1/rooms/{roomId}/join
{
  "name": "bene"
}
→ 200
{
  "roomId": "ab12",
  "wsUrl": "wss://rt.apse1.example.com/v1/ws/ab12"
}
```

### 5.3 Leave Room

```
POST /v1/rooms/{roomId}/leave
→ 204
```

### 5.4 Status (regions, load)

```
GET /v1/status
→ 200
{
  "regions": [
    {"id":"ap-southeast-1","pingMs":42,"wsUrlPattern":"wss://rt.apse1.example.com/v1/ws/{roomId}"}
  ],
  "serverPv": 1
}
```

> Note: For MVP you can bypass REST and directly open WSS if room/seed distribution is handled out-of-band.

---

## 6. WebSocket Lifecycle

1. Client opens **WSS**: `wss://rt.<region>.example.com/v1/ws/{roomId}`
2. Client sends **Join** (`C2S_Join`) immediately.
3. Server replies **Welcome** (`S2C_Welcome`) with `playerId`, `seed`, config.
4. When ready, server sends **Start** (`S2C_Start{ startTick }`).
5. Client begins sending **Input** at 20–30 Hz; server sends **Snapshot** at \~10 Hz.
6. Heartbeats: **Ping/Pong** every 5 s.
7. On disconnect, client attempts **Reconnect** (see §13).

Close codes:

* Normal close (1000) when room ends.
* Abnormal (1006) on network failure.

---

## 7. Message Envelope

All realtime messages are wrapped by a common envelope:

```json
{
  "type": "join",            // message discriminator
  "pv": 1,                   // protocol version
  "ts": 1737765432123,       // sender timestamp (ms)
  "payload": { ... }         // message-specific body
}
```

---

## 8. Client → Server Messages

### 8.1 `join`

```json
{
  "type":"join","pv":1,"ts":1737765,
  "payload":{
    "name":"bene",
    "clientVersion":"android-0.1.0",
    "device":"Pixel_6_Pro",
    "capabilities":{"tilt":true,"vibrate":true}
  }
}
```

### 8.2 `input`

Frequency: 20–30 Hz. Send the **latest local input**. Optionally batch multiple frames.

```json
{
  "type":"input","pv":1,"ts":1737766,
  "payload":{
    "tick": 1021,
    "ax": -0.31,           // normalized tilt or left/right axis
    "jump": false,
    "shoot": false,
    "checksum": 289327134  // optional rolling hash of local compact state
  }
}
```

**Constraints**

* `ax` range recommended `[-1.0, 1.0]` (client-side sensitivity applied).
* Rate limit: ≤ 40 msg/sec (see §11).

### 8.3 `ping`

```json
{"type":"ping","pv":1,"ts":1737767,"payload":{"clientTimeMs":1737767}}
```

### 8.4 `reconnect`

When socket drops, client reopens WSS and sends:

```json
{
  "type":"reconnect","pv":1,"ts":1737768,
  "payload":{"playerId":"p_abcd","lastAckTick": 2110}
}
```

---

## 9. Server → Client Messages

### 9.1 `welcome`

```json
{
  "type":"welcome","pv":1,"ts":1737765,
  "payload":{
    "playerId":"p_abcd",
    "roomId":"ab12",
    "seed":7392012341,
    "cfg":{
      "tps":60,
      "snapshotRateHz":10,
      "maxRollbackTicks":120,
      "world":{
        "worldWidth":1080.0,
        "platformWidth":120.0,
        "platformHeight":18.0,
        "gapMin":120.0,
        "gapMax":240.0,
        "gravity":-2200.0,
        "jumpVy":1200.0,
        "springVy":1800.0,
        "maxVx":900.0,
        "tiltAccel":1200.0
      },
      "difficulty":{
        "gapMinStart":120.0,"gapMinEnd":180.0,
        "gapMaxStart":240.0,"gapMaxEnd":320.0,
        "springChanceStart":0.1,"springChanceEnd":0.03
      }
    },
    "featureFlags":{"enemies":false,"movingPlatforms":true}
  }
}
```

### 9.2 `start`

```json
{"type":"start","pv":1,"ts":1737765,"payload":{"startTick": 300}}
```

### 9.3 `snapshot`

Sent at \~10 Hz (every 6 ticks @60 Hz). Represents **authoritative** world view.

```json
{
  "type":"snapshot","pv":1,"ts":1737766,
  "payload":{
    "tick": 1040,
    "players":[
      {"id":"p_abcd","x":120.3,"y":950.1,"vx":12.0,"vy":980.0,"alive":true},
      {"id":"p_efgh","x":420.9,"y":932.7,"vx":-5.5,"vy":1010.0,"alive":true}
    ],
    "events":[
      {"kind":"spring","x":512.0,"y":840.0}
    ]
  }
}
```

### 9.4 `pong`

```json
{"type":"pong","pv":1,"ts":1737767,"payload":{"serverTimeMs":1737767}}
```

### 9.5 `error`

```json
{"type":"error","pv":1,"ts":1737765,"payload":{"code":"BAD_VERSION","message":"Server pv=1, client pv=0"}}
```

### 9.6 `finish`

```json
{"type":"finish","pv":1,"ts":1737799,"payload":{"reason":"room_closed"}}
```

---

## 10. Timing, Ticks & Reconciliation

* **Tick rate**: `tps = 60`.
* **Client loop**: fixed timestep; sim updates are deterministic given input stream & seed.
* **Snapshots**: `snapshotRateHz = 10` (\~100 ms cadence).
* **Prediction**: client runs local sim every tick using recent inputs.
* **Reconciliation**: on receiving a snapshot:

  1. Replace local player state at `snapshot.tick` with authoritative values.
  2. Re-apply buffered inputs from `snapshot.tick + 1` → `currentTick`.
  3. For **remote players**, interpolate positions between snapshots with \~120 ms render delay.

**Time sync**: use `ping/pong` RTT to maintain an **interpolation delay** (e.g., `max(2×RTT, 100ms)`).

---

## 11. Reliability, Acknowledgments & Rate Limits

* WebSocket is ordered & reliable; include `tick` in messages for idempotency.
* Server maintains last processed input tick per player; can include the `ackTick` in each `snapshot` if desired.
* **Rate limits** (per client):

  * `input`: ≤ **40 msg/sec**
  * `ping`: ≤ **1 msg/5s**
  * `join/reconnect`: backoff on failure (1s, 2s, 4s…)
* On limit breach → `S2C_Error{ "RATE_LIMITED" }` and optional disconnect.

---

## 12. Error Codes

| Code             | Meaning                                | Client Action                 |
| ---------------- | -------------------------------------- | ----------------------------- |
| `BAD_VERSION`    | Protocol mismatch                      | Prompt update, refuse to join |
| `ROOM_NOT_FOUND` | Room ID invalid/expired                | Show error, return to menu    |
| `ROOM_FULL`      | Max players reached                    | Retry another room            |
| `NAME_TAKEN`     | Display name already used in room      | Ask user to rename            |
| `CHEAT_DETECTED` | Invalid physics/jump/velocity patterns | Disconnect, show error        |
| `INVALID_STATE`  | Malformed message/payload              | Log and resync                |
| `RATE_LIMITED`   | Exceeded rate limits                   | Backoff                       |
| `INTERNAL`       | Server error                           | Retry/backoff                 |

---

## 13. Reconnect Flow

* Client stores `{ playerId, lastAckTick }`.
* On socket loss, reopen WSS and send `reconnect`.
* Server responds with latest `snapshot` and resumes from there.
* If room already closed → `finish{ reason: "room_closed" }`.

Reconnect timeout default: **10s** grace.

---

## 14. Anti-Cheat & Validation

Server-side:

* Validate **max velocities**, **jump only when feet crossing platform top with `vy<=0`**, **power-up cooldowns**.
* Reject **teleporting** or **impossible accelerations**.
* Compare optional **checksum** from client vs server’s compact state to detect divergence.

Client-side (lightweight):

* Include `checksum` (FNV-1a over compact local state) every \~15–30 ticks.

---

## 15. Security

* **TLS everywhere** (`https://`, `wss://`).
* **Input-only trust**: client sends inputs, never authoritative positions.
* **No secrets** in queries/URLs.
* Future: short-lived **JWT** auth; per-room ACLs; abuse-prevention on room creation.

---

## 16. Compression & Payload Size Targets

* JSON snapshots ≤ **1.0 KB** for 4 players (target).
* Consider gzip/deflate at HTTP level if required.
* Prefer subtle **quantization** (e.g., 1 decimal place) to reduce payload.

---

## 17. Examples

### 17.1 Compact `input` batch (optional)

```json
{
  "type":"input_batch","pv":1,"ts":1737766,
  "payload":{
    "startTick": 1200,
    "frames": [
      {"d":0,"ax":-0.4,"jump":false},
      {"d":1,"ax":-0.2,"jump":true},
      {"d":2,"ax": 0.0,"jump":false}
    ]
  }
}
```

`d` = delta ticks from `startTick`.

### 17.2 Snapshot with ack

```json
{
  "type":"snapshot","pv":1,"ts":1737766,
  "payload":{
    "tick": 2046,
    "ackTick": 2044,
    "players":[{"id":"p_abcd","x":321.1,"y":1120.5,"vx":9.0,"vy":1180.0,"alive":true}]
  }
}
```

---

## 18. Test Matrix (Interoperability)

**Determinism**

* Same `seed` + scripted input sequence (1000 ticks) → identical final state (hash compare) across Android devices and headless server.

**Cadence**

* `input` at 20–30 Hz; `snapshot` at 10 Hz; sustained 10 minutes without drift > 0.5 px at snapshot tick.

**Reconnect**

* Drop connection for 3 s; client reconnects, reconciles within 500 ms visible latency.

**Rate Limits**

* Burst `input` 80/sec → server returns `RATE_LIMITED` without crash.

---

## 19. Change Log

* **v1.0** — Initial protocol with REST bootstrap, WSS realtime, deterministic tick, snapshots, prediction & reconciliation, error handling, and reconnect.

---

### Appendix A — TypeScript-style Schemas (for reference)

```ts
type Pv = 1;

interface Envelope<T> { type: string; pv: Pv; ts: number; payload: T; }

interface C2S_Join { name: string; clientVersion: string; device?: string; capabilities?: { tilt: boolean; vibrate: boolean; }; }
interface C2S_Input { tick: number; ax: number; jump: boolean; shoot?: boolean; checksum?: number; }
interface C2S_Reconnect { playerId: string; lastAckTick: number; }
interface C2S_Ping { clientTimeMs: number; }

interface NetConfig {
  tps: number; snapshotRateHz: number; maxRollbackTicks: number;
  world: { worldWidth: number; platformWidth: number; platformHeight: number; gapMin: number; gapMax: number; gravity: number; jumpVy: number; springVy: number; maxVx: number; tiltAccel: number; };
  difficulty: { gapMinStart: number; gapMinEnd: number; gapMaxStart: number; gapMaxEnd: number; springChanceStart: number; springChanceEnd: number; };
}

interface S2C_Welcome { playerId: string; roomId: string; seed: number; cfg: NetConfig; featureFlags?: Record<string, boolean>; }
interface S2C_Start { startTick: number; }
interface NetPlayer { id: string; x: number; y: number; vx: number; vy: number; alive: boolean; }
type NetEvent = { kind: "spring" | "break"; x: number; y: number; };
interface S2C_Snapshot { tick: number; ackTick?: number; players: NetPlayer[]; events?: NetEvent[]; }
interface S2C_Pong { serverTimeMs: number; }
interface S2C_Error { code: string; message: string; }
interface S2C_Finish { reason: "room_closed" | "timeout" | "error"; }
```

---

**Implementation Notes**

* Client core loop must be **deterministic** (no system time in physics/spawn).
* Use **seeded RNG** for platform generation; server and client must match.
* For MVP, you can omit events list if not used; clients can derive events from state deltas.

---

**End of spec.**
