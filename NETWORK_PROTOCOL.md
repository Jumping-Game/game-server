# NETWORK\_PROTOCOL.md

**Project:** Vertical Jumper (Android-first, multiplayer)
**Scope:** Client ↔ Server networking for realtime gameplay + supporting APIs
**Audience:** Android game devs, Game server devs
**Status:** **Final v1.1** (compatible with Client Protocol `pv=1`)

---

## Table of Contents

1. [Overview](#1-overview)
2. [Transport & Environment](#2-transport--environment)
3. [Versioning](#3-versioning)
4. [Data Types & Conventions](#4-data-types--conventions)
5. [REST APIs (Matchmaking & Metadata)](#5-rest-apis-matchmaking--metadata)
6. [WebSocket Lifecycle](#6-websocket-lifecycle)
7. [Message Envelope](#7-message-envelope)
8. [Client → Server Messages](#8-client--server-messages)
9. [Server → Client Messages](#9-server--client-messages)
10. [Timing, Ticks & Reconciliation](#10-timing-ticks--reconciliation)
11. [Reliability, Acks, Backpressure & Rate Limits](#11-reliability-acks-backpressure--rate-limits)
12. [Error Codes](#12-error-codes)
13. [Reconnect & Mobile Lifecycle](#13-reconnect--mobile-lifecycle)
14. [Anti-Cheat & Validation](#14-anti-cheat--validation)
15. [Security](#15-security)
16. [Compression & Payload Targets](#16-compression--payload-targets)
17. [Determinism & World Gen Contract](#17-determinism--world-gen-contract)
18. [Examples](#18-examples)
19. [Test Matrix](#19-test-matrix)
20. [Change Log](#20-change-log)
21. [Appendix A — TypeScript-style Schemas](#appendix-a--typescript-style-schemas)

---

## 1. Overview

Vertical jumper with optional realtime multiplayer using:

* **HTTPS REST** for matchmaking/bootstrap.
* **WSS** for realtime game data (inputs/snapshots).
* **Deterministic simulation** at 60 Hz with **client prediction + server reconciliation** and **delta snapshots**.

---

## 2. Transport & Environment

**Realtime**

* Protocol: **WebSocket over TLS** (`wss://`)
* Framing: **JSON text** (compact); binary is future-compatible
* WS extensions: **permessage-deflate** (required)

**REST**

* Protocol: **HTTPS**
* Auth: **wsToken** (short-lived) for WSS; REST endpoints may be unauthenticated for MVP except where issuing tokens.

**Regions**

* Server advertises POP/region in `GET /status`; client picks nearest.

---

## 3. Versioning

* **Protocol version `pv`** (integer) increments on breaking changes.
* Every message includes `type`, `pv`, `ts` (ms), and `seq`.
* Mismatch → `error{code:"BAD_VERSION"}`.

Current: `pv = 1`.

---

## 4. Data Types & Conventions

* **Tick**: 60 Hz discrete step (`0,1,2,…`).
* **Coords**: float pixels in logical world units; origin **bottom-left**; **+Y up**.
* **Player ID** / **Room ID**: strings.
* **Seed** & other 64-bit values: **decimal strings** (avoid IEEE-754 loss).
* **Time**: Unix epoch **milliseconds** (number).
* Field naming: **camelCase**.

---

## 5. REST APIs (Matchmaking & Metadata)

Base: `https://api.example.com`

### 5.1 Create Room

```http
POST /v1/rooms
Content-Type: application/json
{
  "name": "bene",
  "region": "ap-southeast-1",
  "maxPlayers": 4,
  "mode": "endless"
}
→ 201
{
  "roomId": "ab12",
  "seed": "7392012341",
  "region": "ap-southeast-1",
  "wsUrl": "wss://rt.apse1.example.com/v1/ws",
  "wsToken": "eyJhbGciOi..."  // short-lived, binds {roomId, playerId}
}
```

### 5.2 Join Room

```http
POST /v1/rooms/{roomId}/join
{
  "name": "bene"
}
→ 200
{
  "roomId": "ab12",
  "wsUrl": "wss://rt.apse1.example.com/v1/ws",
  "wsToken": "eyJhbGciOi..."
}
```

### 5.3 Leave Room

```http
POST /v1/rooms/{roomId}/leave
→ 204
```

### 5.4 Status (regions, load)

```http
GET /v1/status
→ 200
{
  "regions": [
    {
      "id":"ap-southeast-1",
      "pingMs":42,
      "wsUrl":"wss://rt.apse1.example.com/v1/ws"
    }
  ],
  "serverPv": 1
}
```

---

## 6. WebSocket Lifecycle

1. Client opens `wss://rt.<region>.example.com/v1/ws?token={wsToken}`.
2. Client sends **`join`** immediately.
3. Server replies **`welcome`** with `playerId`, `resumeToken`, `seed`, config.
4. Server sends **`start`** with tick/time anchor.
5. Client sends **`input`** (20–30 Hz, or `input_batch`); server sends **`snapshot`** (\~10 Hz).
6. Heartbeats: **`ping`** / **`pong`** every 5 s (echo times).
7. On disconnect: use **`reconnect`** (see §13).

Close codes: 1000 normal end; 1001 going away; 1006 abnormal.

---

## 7. Message Envelope

```json
{
  "type": "string",       // discriminator
  "pv": 1,                // protocol version
  "seq": 184,             // uint32, per-connection, wraps
  "ts": 1737765432123,    // sender timestamp (ms)
  "payload": { }          // message-specific body
}
```

---

## 8. Client → Server Messages

### 8.1 `join`

```json
{
  "type":"join","pv":1,"seq":1,"ts":1737765,
  "payload":{
    "name":"bene",
    "clientVersion":"android-0.1.0",
    "device":"Pixel_6_Pro",
    "capabilities":{"tilt":true,"vibrate":true}
  }
}
```

### 8.2 `input` (single)

Frequency: 20–30 Hz; last known local input.
Server accepts ticks in window `[currentTick - maxRollbackTicks, currentTick + inputLeadTicks]`.

```json
{
  "type":"input","pv":1,"seq":182,"ts":1737766,
  "payload":{
    "tick": 1021,
    "axisX": -0.31,        // normalized [-1..1]
    "jump": false,
    "shoot": false,
    "checksum": "289327134" // optional 64-bit decimal string
  }
}
```

### 8.3 `input_batch` (recommended on mobile)

```json
{
  "type":"input_batch","pv":1,"seq":183,"ts":1737766,
  "payload":{
    "startTick": 1200,
    "frames": [
      {"d":0,"axisX":-0.4,"jump":false},
      {"d":1,"axisX":-0.2,"jump":true},
      {"d":2,"axisX": 0.0,"jump":false}
    ]
  }
}
```

### 8.4 `ping`

```json
{"type":"ping","pv":1,"seq":200,"ts":1737767,"payload":{"t0":1737767123456}}
```

### 8.5 `reconnect`

```json
{
  "type":"reconnect","pv":1,"seq":2,"ts":1737768,
  "payload":{"playerId":"p_abcd","resumeToken":"r_opaque_128b","lastAckTick":2110}
}
```

---

## 9. Server → Client Messages

### 9.1 `welcome`

```json
{
  "type":"welcome","pv":1,"seq":1,"ts":1737765,
  "payload":{
    "playerId":"p_abcd",
    "resumeToken":"r_opaque_128b",
    "roomId":"ab12",
    "seed":"7392012341",
    "cfg":{
      "tps":60,
      "snapshotRateHz":10,
      "maxRollbackTicks":120,
      "inputLeadTicks":2,
      "world":{
        "worldWidth":1080.0,
        "platformWidth":120.0,
        "platformHeight":18.0,
        "gapMin":120.0,"gapMax":240.0,
        "gravity":-2200.0,
        "jumpVy":1200.0,"springVy":1800.0,
        "maxVx":900.0,"tiltAccel":1200.0
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

### 9.2 `start` (authoritative tick/time anchor)

```json
{
  "type":"start","pv":1,"seq":2,"ts":1737765,
  "payload":{
    "startTick":300,              // earliest tick client should simulate from
    "serverTick":12042,           // tick at serverTimeMs sample
    "serverTimeMs":1737765123456, // wallclock anchor
    "tps":60
  }
}
```

### 9.3 `snapshot` (delta by default)

```json
{
  "type":"snapshot","pv":1,"seq":220,"ts":1737766,
  "payload":{
    "tick": 2046,
    "ackTick": 2044,            // last input tick processed
    "lastInputSeq": 512,        // last client seq processed
    "full": false,              // delta from last full snapshot for this client
    "players":[
      {"id":"p_abcd","x":321.1,"y":1120.5,"vx":9.0,"vy":1180.0,"alive":true}
    ],
    "events":[{"kind":"spring","x":512.0,"y":840.0,"tick":2044}],
    "stats":{"droppedSnapshots":0}
  }
}
```

Server MUST send a **full** snapshot at least every 1 s or upon resync.

### 9.4 `pong`

```json
{"type":"pong","pv":1,"seq":201,"ts":1737767,"payload":{"t0":1737767123456,"t1":1737767123470}}
```

### 9.5 `error`

```json
{"type":"error","pv":1,"seq":9,"ts":1737765,"payload":{"code":"BAD_VERSION","message":"Server pv=1, client pv=0"}}
```

### 9.6 `finish`

```json
{"type":"finish","pv":1,"seq":999,"ts":1737799,"payload":{"reason":"room_closed"}}
```

### 9.7 `player_presence` (optional)

```json
{"type":"player_presence","pv":1,"seq":300,"ts":1737770,"payload":{"id":"p_efgh","state":"disconnected"}}
```

---

## 10. Timing, Ticks & Reconciliation

* **Tick rate**: `tps = 60`.
* **Client loop**: fixed timestep; deterministic given input stream + seed.
* **Snapshots**: `snapshotRateHz ≈ 10` (100 ms cadence).
* **Prediction**: client simulates every tick locally.
* **Reconciliation** on snapshot:

  1. Replace local player state at `snapshot.tick`.
  2. Re-apply buffered inputs from `snapshot.tick+1…currentTick`.
  3. For remote players: **interpolate** between snapshots with \~`max(2×RTT, 100ms)` render delay.
* **Clock sync**: use `ping/pong` `(t0,t1,t2)` to compute RTT & skew; maintain mapping from server wallclock to tick.

---

## 11. Reliability, Acks, Backpressure & Rate Limits

* WebSocket is ordered/reliable; include `tick` + `seq` for idempotency.
* Server keeps **last processed input tick** and **last input seq** per player; echoes as `ackTick` / `lastInputSeq`.
* **Input acceptance window**: `[currentTick - maxRollbackTicks, currentTick + inputLeadTicks]` (typ. `120`, `2`).
* **Backpressure / slow consumer**:

  * Per-socket send queue cap (e.g., 3 snapshots). On overflow → drop older, keep latest, increment `droppedSnapshots`.
  * Persistent overflow → `error{code:"SLOW_CONSUMER"}` then close.
* **Rate limits (per client)**:

  * `input` or `input_batch`: ≤ **40 msg/s** (burstable to 60 for 1 s)
  * `ping`: ≤ **1 / 5 s**
  * Max message size: **4 KB**
  * Max outbound bandwidth: **64 KB/s** averaged over 10 s
  * REST: respond `429` with `Retry-After` when exceeded.

---

## 12. Error Codes

| Code             | Meaning                                | Client Action                       |
| ---------------- | -------------------------------------- | ----------------------------------- |
| `BAD_VERSION`    | Protocol mismatch                      | Prompt update                       |
| `ROOM_NOT_FOUND` | Room invalid/expired                   | Return to menu                      |
| `ROOM_FULL`      | Max players reached                    | Retry another room                  |
| `NAME_TAKEN`     | Display name already used in room      | Ask user to rename                  |
| `CHEAT_DETECTED` | Invalid physics/jump/velocity patterns | Disconnect; show error              |
| `INVALID_STATE`  | Malformed message/payload              | Log, resync                         |
| `INVALID_TICK`   | Input tick outside acceptance window   | Resync clock; drop late inputs      |
| `RATE_LIMITED`   | Exceeded rate limits                   | Backoff                             |
| `UNAUTHORIZED`   | Missing/invalid token                  | Re-auth, retry join                 |
| `SLOW_CONSUMER`  | Client cannot keep up with stream      | Reduce graphics latency / reconnect |
| `ROOM_CLOSED`    | Room ended                             | Exit to menu                        |
| `INTERNAL`       | Server error                           | Retry/backoff                       |

---

## 13. Reconnect & Mobile Lifecycle

* Client stores `{ playerId, resumeToken, lastAckTick }`.
* On socket loss:

  1. Reopen WSS with **fresh wsToken** (if expired, re-hit REST).
  2. Send `reconnect` with `resumeToken` & `lastAckTick`.
  3. Server responds with latest **full** snapshot; resume.
* **Grace**: server keeps player alive for **10 s** by default.
* **Backgrounding** (Android):

  * On background: pause sending inputs; optionally send `pause`.
  * On resume ≤ 10 s: auto-reconnect and reconcile < 500 ms.

---

## 14. Anti-Cheat & Validation

Server validates:

* Max velocities/accelerations; **jump only** when feet crossing platform top with `vy ≤ 0`.
* Power-up cooldowns; event usage constraints.
* Teleporting / impossible acceleration patterns.
* Optional **checksum** (client → server) vs compact state to detect divergence.

---

## 15. Security

* **TLS everywhere** (`https://`, `wss://`).
* Clients send **inputs only**, never authoritative positions.
* **No secrets** in URLs besides short-lived **wsToken**.
* **Resume security**: `resumeToken` (opaque ≥128 bits) required for `reconnect`.
* Abuse prevention on room creation (IP rate limits, CAPTCHAs if needed).

---

## 16. Compression & Payload Targets

* **permessage-deflate** MUST be negotiated.
* Quantize floats to **1 decimal** on the wire.
* Target snapshot size: **≤ 1.0 KB** for 4 players (delta); periodic full snapshot ≤ 2.0 KB.
* Prefer `input_batch` to reduce radio wakes.

---

## 17. Determinism & World Gen Contract

* **Arithmetic** MUST be deterministic:

  * Prefer **fixed-point Q16.16** for simulation state & integrator, **or**
  * Use **double precision** consistently; forbid platform intrinsics.
* **RNG**: **SplitMix64** (or Xoroshiro128\*\*) with seed from `seed` (string).
* **World generation** derives **only** from `(seed, tick)`—no wallclock.
* Moving platforms use **closed-form** positions per tick (no accumulated float drift):
  `x(t) = x0 + A * sin((2π/periodTicks) * t + phase)`.
* Collision order/broadphase MUST be identical on client & server.

---

## 18. Examples

### 18.1 Compact input batch

```json
{
  "type":"input_batch","pv":1,"seq":183,"ts":1737766,
  "payload":{"startTick":1200,"frames":[{"d":0,"axisX":-0.4},{"d":1,"axisX":-0.2,"jump":true},{"d":2,"axisX":0.0}]}
}
```

### 18.2 Snapshot with ack/delta

```json
{
  "type":"snapshot","pv":1,"seq":220,"ts":1737766,
  "payload":{"tick":2046,"ackTick":2044,"lastInputSeq":512,"full":false,"players":[{"id":"p_abcd","x":321.1,"y":1120.5,"vx":9.0,"vy":1180.0}]}
}
```

### 18.3 Ping/Pong time echo

```json
{"type":"ping","pv":1,"seq":200,"ts":1737767,"payload":{"t0":1737767123456}}
{"type":"pong","pv":1,"seq":201,"ts":1737767,"payload":{"t0":1737767123456,"t1":1737767123470}}
```

---

## 19. Test Matrix

**Determinism**

* Same `seed` + scripted inputs for 10 k ticks on ARM64 Android vs x86\_64 Linux → **bit-equal** world hash every 1 k ticks.

**Cadence**

* `input_batch` at 20–30 Hz; `snapshot` at 10 Hz; 10 min run; drift ≤ 0.5 px at snapshot tick.

**Clock skew & jitter**

* ±200 ms skew; jitter 0–120 ms; 2% duplicate frames → no rubber-banding > 1.5 tiles.

**Reconnect**

* Drop for 3 s; reconnect; reconcile < 500 ms visual latency.

**Slow consumer**

* Throttle downstream to 8 KB/s for 5 s → connection maintained; latest snapshot delivered after stall; no crash.

**Abuse / security**

* Invalid `resumeToken` 10×/30 s → exponential backoff / temporary IP ban (server-side).

**Backgrounding**

* App background 7 s → auto-reconnect; state consistent; peers see `player_presence`.

---

## Appendix A — TypeScript-style Schemas

```ts
type Pv = 1;

interface Envelope<T> {
  type: string;
  pv: Pv;
  seq: number;        // uint32 wraps
  ts: number;         // ms
  payload: T;
}

// ----- Client → Server -----
interface C2S_Join {
  name: string;
  clientVersion: string;
  device?: string;
  capabilities?: { tilt: boolean; vibrate: boolean; };
}

interface C2S_Input { tick: number; axisX: number; jump?: boolean; shoot?: boolean; checksum?: string; }
interface C2S_InputBatch { startTick: number; frames: Array<{ d: number; axisX: number; jump?: boolean; shoot?: boolean; }>; }

interface C2S_Ping { t0: number; }
interface C2S_Reconnect { playerId: string; resumeToken: string; lastAckTick: number; }

// ----- Server → Client -----
interface NetWorldCfg {
  worldWidth: number; platformWidth: number; platformHeight: number;
  gapMin: number; gapMax: number;
  gravity: number; jumpVy: number; springVy: number; maxVx: number; tiltAccel: number;
}

interface NetDifficultyCfg {
  gapMinStart: number; gapMinEnd: number;
  gapMaxStart: number; gapMaxEnd: number;
  springChanceStart: number; springChanceEnd: number;
}

interface NetConfig {
  tps: number;
  snapshotRateHz: number;
  maxRollbackTicks: number;
  inputLeadTicks: number;
  world: NetWorldCfg;
  difficulty: NetDifficultyCfg;
}

interface S2C_Welcome { playerId: string; resumeToken: string; roomId: string; seed: string; cfg: NetConfig; featureFlags?: Record<string, boolean>; }
interface S2C_Start { startTick: number; serverTick: number; serverTimeMs: number; tps: number; }

interface NetPlayer { id: string; x: number; y: number; vx: number; vy: number; alive: boolean; }
type NetEvent = { kind: "spring" | "break"; x: number; y: number; tick: number; };

interface S2C_Snapshot {
  tick: number;
  ackTick?: number;
  lastInputSeq?: number;
  full: boolean;               // false = delta from last full snapshot
  players: Partial<NetPlayer>[] & { [0]: { id: string } }; // id required; others partial when delta
  events?: NetEvent[];
  stats?: { droppedSnapshots?: number };
}

interface S2C_Pong { t0: number; t1: number; }
interface S2C_Error { code: string; message?: string; }
interface S2C_Finish { reason: "room_closed" | "timeout" | "error"; }
interface S2C_PlayerPresence { id: string; state: "active" | "disconnected" | "left"; }
```

---

**Implementation Notes**

* Simulation MUST avoid wallclock; rely on `(seed, tick)` only.
* Prefer `input_batch` to reduce radio wakeups & bandwidth.
* When closing after `error`, delay \~100–200 ms so the frame is received.
* Quantize floats to **1 decimal** when serializing; preserve higher precision in simulation state.