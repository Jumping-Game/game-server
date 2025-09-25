# NETWORK_PROTOCOL.md

**Project:** Vertical Jumper — Web/Android, Multiplayer
**Scope:** Client ↔ Server networking for realtime gameplay + supporting APIs
**Audience:** Game client devs (TS/Kotlin), Game server devs (Rust)
**Status:** **Final v1.2** (compatible with Client Protocol `pv=1`)
**Compat:** v1.1 → v1.2 is **backward compatible** for core gameplay; lobby/start flow is additive.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Transport & Environment](#2-transport--environment)
3. [Versioning](#3-versioning)
4. [Data Types & Conventions](#4-data-types--conventions)
5. [REST APIs (Matchmaking & Lobby)](#5-rest-apis-matchmaking--lobby)
6. [WebSocket Lifecycle](#6-websocket-lifecycle)
7. [Message Envelope](#7-message-envelope)
8. [Client → Server Messages](#8-client--server-messages)
9. [Server → Client Messages](#9-server--client-messages)
10. [Timing, Ticks & Reconciliation](#10-timing-ticks--reconciliation)
11. [Reliability, Acks, Backpressure & Rate Limits](#11-reliability-acks-backpressure--rate-limits)
12. [Room/Match Rules](#12-roommatch-rules)
13. [Error Codes](#13-error-codes)
14. [Reconnect & Lifecycle](#14-reconnect--lifecycle)
15. [Security](#15-security)
16. [Compression & Payload Targets](#16-compression--payload-targets)
17. [Determinism & World Generation](#17-determinism--world-generation)
18. [Examples](#18-examples)
19. [Test Matrix](#19-test-matrix)
20. [Change Log](#20-change-log)
21. [Appendix A — TypeScript-style Schemas](#appendix-a--typescript-style-schemas)

---

## 1. Overview

Multiplayer **vertical jumper** with:

* **HTTPS REST** for matchmaking/lobby.
* **WSS** for realtime (inputs/snapshots, heartbeats, lobby updates).
* **Deterministic simulation** @ **60 Hz** with **client prediction + server reconciliation** and **delta snapshots**.
* **Lobby with Room Master**: the **creator is master** and controls when the game starts (countdown → synchronized start).

---

## 2. Transport & Environment

**Realtime**

* Protocol: **WebSocket over TLS** (`wss://`); local dev `ws://` allowed.
* Framing: **JSON text** (binary reserved for future).
* WS extension: **permessage-deflate** (negotiate; strongly recommended).

**REST**

* Protocol: **HTTPS**; local dev `http://localhost` allowed.
* Auth: **`wsToken`** (short-lived) for WSS; REST endpoints that issue tokens are authenticated/limited per deployment.

**Regions**

* `GET /v1/status` advertises regions/WS URLs; client selects nearest/lowest RTT.

---

## 3. Versioning

* **Protocol version `pv`** (integer) in **every frame**.
* Mismatch → `error{code:"BAD_VERSION"}` and close.
* Current: `pv = 1`.

---

## 4. Data Types & Conventions

* **Tick**: integer @ 60 Hz (`0,1,2,…`).
* **Coords**: float pixels; origin **bottom-left**; **+Y up**.
* IDs: **string** (`playerId`, `roomId`).
* 64-bit ints (e.g., seeds, checksums): **decimal strings**.
* Timestamps: **Unix ms** (`number`).
* Field naming: **camelCase**.

---

## 5. REST APIs (Matchmaking & Lobby)

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
  "wsToken": "…",                // binds {roomId, playerId}
  "role": "master",              // "master" | "member"
  "state": "lobby",              // "lobby" | "starting" | "running" | "finished"
  "maxPlayers": 4
}
```

### 5.2 Join Room

```http
POST /v1/rooms/{roomId}/join
{ "name": "ally" }
→ 200
{
  "roomId": "ab12",
  "wsUrl": "wss://rt.apse1.example.com/v1/ws",
  "wsToken": "…",
  "role": "member",
  "state": "lobby"
}
```

### 5.3 Leave Room

```http
POST /v1/rooms/{roomId}/leave
{ "playerId": "p01", "reason": "left voluntarily" }  // reason optional
→ 204
```

*Request body*

| Field | Type   | Required | Notes |
|-------|--------|----------|-------|
| `playerId` | `string` | ✅ | Must match the authenticated player for the supplied `wsToken`. |
| `reason` | `string` | ❌ | Optional free-form context (e.g., `"left voluntarily"`, `"disconnected"`). |

Clients must always include the `playerId`. The server validates it against the room membership that was bound when the `wsToken` was issued. Omitting or mismatching the `playerId` results in `422 Unprocessable Entity` with an error such as `missing field "playerId"`.

### 5.4 Status

```http
GET /v1/status
→ 200
{
  "regions": [
    { "id":"ap-southeast-1", "pingMs":42, "wsUrl":"wss://rt.apse1.example.com/v1/ws" }
  ],
  "serverPv": 1
}
```

### 5.5 Start Game (master only)

```http
POST /v1/rooms/{roomId}/start
{ "countdownSec": 3 }  // optional, clamp e.g., 0–5
→ 202
{ "state":"starting","startAtMs":1737765123456 }
```

### 5.6 Ready Toggle (optional rule)

```http
POST /v1/rooms/{roomId}/ready
{ "ready": true }
→ 204
```

> NOTE: If you prefer WS control messages instead of REST for start/ready, see §8.

---

## 6. WebSocket Lifecycle

1. Client opens `wss://rt.<region>/v1/ws?token={wsToken}` (or `ws://` local).
2. Client sends **`join`** immediately.
3. Server replies **`welcome`** (includes `role`, `roomState`, lobby snapshot).
4. While `roomState="lobby"`: members may toggle **ready**; master may **start**.
5. Server broadcasts **`start_countdown`**, then **`start`** (tick/time anchor).
6. During `running`: client streams **`input`/`input_batch`**; server streams **`snapshot`**.
7. Heartbeats: **`ping`/`pong`** every 5 s.
8. Disconnects: use **`reconnect`** (§14).

Close codes: 1000 normal; 1001 going away; 1006 abnormal.

---

## 7. Message Envelope

```json
{
  "type": "string",            // discriminator
  "pv": 1,                     // protocol version
  "seq": 184,                  // uint32, per-connection, wraps
  "ts": 1737765432123,         // sender timestamp (ms)
  "payload": {}                // message-specific body
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
    "clientVersion":"web-0.1.0",
    "device":"Chrome_128",
    "capabilities":{"tilt":false,"vibrate":false}
  }
}
```

### 8.2 `input` (single)

Frequency: 20–30 Hz; server accepts `tick` in window `[currentTick - maxRollbackTicks, currentTick + inputLeadTicks]`.

```json
{
  "type":"input","pv":1,"seq":182,"ts":1737766,
  "payload":{"tick":1021,"axisX":-0.31,"jump":false,"checksum":"289327134"}
}
```

### 8.3 `input_batch` (preferred on mobile)

```json
{
  "type":"input_batch","pv":1,"seq":183,"ts":1737766,
  "payload":{"startTick":1200,"frames":[{"d":0,"axisX":-0.4},{"d":1,"axisX":-0.2,"jump":true},{"d":2,"axisX":0.0}]}
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

### 8.6 (Optional WS controls) `ready_set`, `start_request`

```json
{"type":"ready_set","pv":1,"seq":9,"ts":1737765,"payload":{"ready":true}}
{"type":"start_request","pv":1,"seq":10,"ts":1737765,"payload":{"countdownSec":3}}
{"type":"character_select","pv":1,"seq":11,"ts":1737765,"payload":{"characterId":"violet"}}
```

---

## 9. Server → Client Messages

### 9.1 `welcome` (adds lobby info, role, roomState)

```json
{
  "type":"welcome","pv":1,"seq":1,"ts":1737765,
  "payload":{
    "playerId":"p_abcd",
    "resumeToken":"r_opaque_128b",
    "roomId":"ab12",
    "seed":"7392012341",
    "role":"master",
    "roomState":"lobby",
    "lobby":{
      "players":[{"id":"p_abcd","name":"bene","ready":true,"role":"master","characterId":"aurora"}],
      "maxPlayers":4
    },
    "cfg":{ "tps":60,"snapshotRateHz":10,"maxRollbackTicks":120,"inputLeadTicks":2,
            "world":{"worldWidth":1080,"platformWidth":120,"platformHeight":18,"gapMin":120,"gapMax":240,
                     "gravity":-2200,"jumpVy":1200,"springVy":1800,"maxVx":900,"tiltAccel":1200},
            "difficulty":{"gapMinStart":120,"gapMinEnd":180,"gapMaxStart":240,"gapMaxEnd":320,
                          "springChanceStart":0.1,"springChanceEnd":0.03}},
    "featureFlags":{"enemies":false,"movingPlatforms":true}
  }
}
```

### 9.2 `lobby_state` (membership/ready/role changes)

```json
{
  "type":"lobby_state","pv":1,"seq":50,"ts":1737766,
  "payload":{
    "roomState":"lobby",
    "players":[
      {"id":"p_abcd","name":"bene","ready":true,"role":"master","characterId":"aurora"},
      {"id":"p_efgh","name":"ally","ready":false,"role":"member","characterId":"cobalt"}
    ]
  }
}
```

### 9.3 `start_countdown`

```json
{
  "type":"start_countdown","pv":1,"seq":60,"ts":1737767,
  "payload":{"startAtMs":1737765123456,"serverTick":12000,"countdownSec":3}
}
```

### 9.4 `start` (authoritative tick/time anchor)

```json
{
  "type":"start","pv":1,"seq":61,"ts":1737767,
  "payload":{"startTick":300,"serverTick":12042,"serverTimeMs":1737765123456,"tps":60}
}
```

### 9.5 `snapshot` (delta by default)

```json
{
  "type":"snapshot","pv":1,"seq":220,"ts":1737766,
  "payload":{
    "tick":2046,"ackTick":2044,"lastInputSeq":512,"full":false,
    "players":[{"id":"p_abcd","x":321.1,"y":1120.5,"vx":9.0,"vy":1180.0,"alive":true}],
    "events":[{"kind":"spring","x":512.0,"y":840.0,"tick":2044}],
    "stats":{"droppedSnapshots":0}
  }
}
```

### 9.6 `role_changed`

```json
{"type":"role_changed","pv":1,"seq":70,"ts":1737768,"payload":{"newMasterId":"p_efgh"}}
```

### 9.7 `pong`

```json
{"type":"pong","pv":1,"seq":201,"ts":1737767,"payload":{"t0":1737767123456,"t1":1737767123470}}
```

### 9.8 `player_presence` (optional)

```json
{"type":"player_presence","pv":1,"seq":300,"ts":1737770,"payload":{"id":"p_efgh","state":"disconnected"}}
```

### 9.9 `finish`

```json
{"type":"finish","pv":1,"seq":999,"ts":1737799,"payload":{"reason":"room_closed"}}
```

### 9.10 `error`

```json
{"type":"error","pv":1,"seq":9,"ts":1737765,"payload":{"code":"BAD_VERSION","message":"Server pv=1, client pv=0"}}
```

---

## 10. Timing, Ticks & Reconciliation

* **Tick rate**: `tps = 60`.

* **Client loop**: fixed timestep; deterministic given `(seed, inputs)`.

* **Snapshots**: ~10 Hz (100 ms cadence).

* **Prediction**: simulate each tick locally.

* **Reconciliation** on snapshot(t):

  1. Replace local authoritative state at `t`.
  2. Re-apply buffered inputs `t+1…currentTick`.
  3. Remote players: **interpolate** with render delay `≈ max(2×RTT, 100ms)`.

* **Clock sync**: `ping/pong` `(t0,t1,t2)` to derive RTT & skew; maintain mapping from server wallclock ↔ tick.

---

## 11. Reliability, Acks, Backpressure & Rate Limits

* WS ordered/reliable; include `seq` + `tick` for idempotency.
* Server echoes **`ackTick`** and **`lastInputSeq`**.
* **Input acceptance window**: `[currentTick - maxRollbackTicks, currentTick + inputLeadTicks]` (typ. `120`, `2`).
* **Backpressure**: per-socket send queue cap (e.g., 3 snapshots). Drop oldest, keep latest, increment `droppedSnapshots`; persistent overflow → `SLOW_CONSUMER` then close.
* **Client rate limits** (guidance):

  * `input`/`input_batch`: ≤ **40 msg/s** (burst 60 for 1 s)
  * `ping`: ≤ **1 / 5 s**
  * Max message size: **4 KB**
  * Max outbound bandwidth: **64 KB/s** over 10 s
  * REST overuse: respond `429` + `Retry-After`.

---

## 12. Room/Match Rules

* On create: creator is **`role="master"`**; room `state="lobby"`.
* Members can join while `state="lobby"`.
* **Start authority**: only **master** can start (REST `/start` or WS `start_request`).
* **Countdown**: broadcast `start_countdown` with `startAtMs`; server schedules **`start`** at anchor time.
* **Ready gate (optional)**: if enabled, server rejects start unless all members are `ready=true` or a minimum quorum is met (`ROOM_NOT_READY`).
* **Master transfer**: if master leaves in lobby, server promotes **oldest** or **priority** member and broadcasts `role_changed`.
* During `running`, late joiners rejected (`ROOM_STATE_INVALID`) or placed in spectate (out of scope).

---

## 13. Error Codes

| Code                     | Meaning                              | Client Action                     |
| ------------------------ | ------------------------------------ | --------------------------------- |
| `BAD_VERSION`            | Protocol mismatch                    | Prompt update                     |
| `ROOM_NOT_FOUND`         | Room invalid/expired                 | Back to menu                      |
| `ROOM_FULL`              | Max players reached                  | Try another room                  |
| `NAME_TAKEN`             | Display name already used in room    | Rename                            |
| `INVALID_STATE`          | Malformed message/payload            | Log; resync/retry                 |
| `INVALID_TICK`           | Input tick outside acceptance window | Resync clock; drop late inputs    |
| `RATE_LIMITED`           | Exceeded rate limits                 | Backoff                           |
| `UNAUTHORIZED`           | Missing/invalid token                | Re-auth/join again                |
| `SLOW_CONSUMER`          | Client cannot keep up                | Lower graphics; reconnect         |
| `ROOM_CLOSED`            | Room ended                           | Exit                              |
| `INTERNAL`               | Server error                         | Retry/backoff                     |
| **`NOT_MASTER`**         | Member attempted master-only action  | Show error; disable start button  |
| **`ROOM_STATE_INVALID`** | Action not allowed in current state  | Stay in lobby or handle per state |
| **`ROOM_NOT_READY`**     | Ready rule unmet                     | Show missing-readiness UI         |
| **`START_ALREADY`**      | Start already triggered              | Ignore duplicate                  |
| **`COUNTDOWN_ACTIVE`**   | Start countdown in progress          | Show countdown; wait              |

---

## 14. Reconnect & Lifecycle

* Client stores `{ playerId, resumeToken, lastAckTick }`.
* On loss:

  1. Reopen WSS (new `wsToken` if expired).
  2. Send `reconnect`.
  3. Server replies with latest **full** snapshot; resume.
* **Grace**: server keeps player alive for **10 s**.
* Backgrounding (mobile): pause inputs; auto-resume ≤ 10 s.

---

## 15. Security

* **TLS** for prod (`https://`, `wss://`).
* Clients send **inputs only**; never authoritative positions.
* Short-lived **`wsToken`**; don’t log secrets.
* `resumeToken` opaque ≥128 bits.
* Abuse prevention on room creation (rate limits, CAPTCHA as needed).

---

## 16. Compression & Payload Targets

* Negotiate **permessage-deflate**.
* Quantize floats to **1 decimal** on the wire; keep higher precision in sim.
* Target snapshot (delta, 4 players) **≤ 1.0 KB**; periodic full **≤ 2.0 KB**.
* Prefer `input_batch` to cut packet rate.

---

## 17. Determinism & World Generation

* Deterministic arithmetic: fixed-point **Q16.16** or consistent **double**; avoid platform intrinsics.
* RNG: **SplitMix64** or **Xoroshiro128**** seeded from `seed` (decimal string).
* World gen depends **only** on `(seed, tick)`.
* Moving platforms use **closed-form** per tick:
  `x(t)=x0 + A * sin((2π/periodTicks)*t + phase)` (no drift).
* Collision order/broadphase identical on client & server.

---

## 18. Examples

### 18.1 Minimal input batch

```json
{"type":"input_batch","pv":1,"seq":183,"ts":1737766,
 "payload":{"startTick":1200,"frames":[{"d":0,"axisX":-0.4},{"d":1,"axisX":-0.2,"jump":true},{"d":2,"axisX":0.0}]}}
```

### 18.2 Snapshot (delta)

```json
{"type":"snapshot","pv":1,"seq":220,"ts":1737766,
 "payload":{"tick":2046,"ackTick":2044,"lastInputSeq":512,"full":false,
            "players":[{"id":"p_abcd","x":321.1,"y":1120.5,"vx":9.0,"vy":1180.0}]}}
```

### 18.3 Lobby → Countdown → Start

```json
{"type":"lobby_state","pv":1,"seq":50,"ts":1737766,"payload":{"roomState":"lobby","players":[{"id":"p1","name":"bene","ready":true,"role":"master"},{"id":"p2","name":"ally","ready":true,"role":"member"}]}}
{"type":"start_countdown","pv":1,"seq":60,"ts":1737767,"payload":{"startAtMs":1737765123456,"serverTick":12000,"countdownSec":3}}
{"type":"start","pv":1,"seq":61,"ts":1737767,"payload":{"startTick":300,"serverTick":12042,"serverTimeMs":1737765123456,"tps":60,"players":[{"id":"p1","name":"bene","role":"master","ready":true,"characterId":"aurora"},{"id":"p2","name":"ally","role":"member","ready":true,"characterId":"cobalt"}]}}
```

---

## 19. Test Matrix (key cases)

* **Determinism:** same `seed` + scripted inputs for 10k ticks (ARM64 vs x86_64) → **bit-equal** world hash every 1k ticks.
* **Cadence:** `input_batch` 20–30 Hz; `snapshot` 10 Hz; 10 min run; drift ≤ 0.5 px at snapshot tick.
* **Clock jitter:** ±200 ms skew; 0–120 ms jitter; 2% dup frames → no rubber-banding > 1.5 tiles.
* **Reconnect:** drop 3 s; reconnect; reconcile < 500 ms visual latency.
* **Slow consumer:** throttle downstream to 8 KB/s for 5 s → maintained; latest snapshot delivered; no crash.
* **Lobby start:** two clients, master starts; both receive countdown & start; member cannot start (gets `NOT_MASTER`).
* **Master transfer:** master disconnects in lobby → `role_changed`; new master can start.

---

## 20. Change Log

* **v1.2**

  * Added **Room Master + Lobby** flow (REST `/start`, optional WS `start_request`).
  * New S2C: `lobby_state`, `start_countdown`, `role_changed`; `welcome` extended with `role`, `roomState`, `lobby`.
  * New C2S: `character_select`; lobby players now advertise optional `characterId`.
  * `start` frames include the full lobby roster (id, name, role, ready, `characterId`).
  * Added errors: `NOT_MASTER`, `ROOM_STATE_INVALID`, `ROOM_NOT_READY`, `START_ALREADY`, `COUNTDOWN_ACTIVE`.
  * Clarified local `ws://`/`http://` allowances; compression “recommended” vs “required”.

* **v1.1**

  * Core realtime (input/snapshot), reconciliation, determinism contract.

---

## Appendix A — TypeScript-style Schemas

```ts
type Pv = 1;

interface Envelope<T> {
  type: string; pv: Pv; seq: number; ts: number; payload: T;
}

// ----- REST responses (subset) -----
interface RestCreateRoomRes {
  roomId: string; seed: string; region: string;
  wsUrl: string; wsToken: string;
  role: "master" | "member"; state: "lobby" | "starting" | "running" | "finished";
  maxPlayers: number;
}
interface RestJoinRoomRes extends Omit<RestCreateRoomRes, "seed"|"maxPlayers"> {}

// ----- Client → Server -----
interface C2S_Join { name: string; clientVersion: string; device?: string;
  capabilities?: { tilt: boolean; vibrate: boolean }; }

interface C2S_Input { tick: number; axisX: number; jump?: boolean; shoot?: boolean; checksum?: string; }
interface C2S_InputBatch { startTick: number; frames: Array<{ d: number; axisX: number; jump?: boolean; shoot?: boolean }>; }
interface C2S_Ping { t0: number; }
interface C2S_Reconnect { playerId: string; resumeToken: string; lastAckTick: number; }
interface C2S_CharacterSelect { characterId: string; }
interface C2S_ReadySet { ready: boolean; }               // optional
interface C2S_StartRequest { countdownSec?: number; }    // optional

// ----- Server → Client -----
interface LobbyPlayer {
  id: string;
  name: string;
  ready: boolean;
  role: "master" | "member";
  characterId?: string;
}

interface NetWorldCfg {
  worldWidth: number; platformWidth: number; platformHeight: number;
  gapMin: number; gapMax: number; gravity: number; jumpVy: number; springVy: number; maxVx: number; tiltAccel: number;
}
interface NetDifficultyCfg {
  gapMinStart: number; gapMinEnd: number; gapMaxStart: number; gapMaxEnd: number; springChanceStart: number; springChanceEnd: number;
}
interface NetConfig {
  tps: number; snapshotRateHz: number; maxRollbackTicks: number; inputLeadTicks: number;
  world: NetWorldCfg; difficulty: NetDifficultyCfg;
}

interface S2C_Welcome {
  playerId: string; resumeToken: string; roomId: string; seed: string;
  role: "master" | "member";
  roomState: "lobby" | "starting" | "running" | "finished";
  lobby?: { players: LobbyPlayer[]; maxPlayers: number; };
  cfg: NetConfig; featureFlags?: Record<string, boolean>;
}
interface S2C_LobbyState {
  roomState: "lobby" | "starting" | "running" | "finished";
  players: LobbyPlayer[];
  maxPlayers?: number;
}
interface S2C_StartCountdown { startAtMs: number; serverTick: number; countdownSec: number; }
interface S2C_Start {
  startTick: number;
  serverTick: number;
  serverTimeMs: number;
  tps: number;
  players: LobbyPlayer[];
}

interface NetPlayer { id: string; x: number; y: number; vx: number; vy: number; alive: boolean; }
type NetEvent = { kind: "spring" | "break"; x: number; y: number; tick: number; };

interface S2C_Snapshot {
  tick: number; ackTick?: number; lastInputSeq?: number; full: boolean;
  players: Partial<NetPlayer>[] & { [0]: { id: string } };
  events?: NetEvent[]; stats?: { droppedSnapshots?: number };
}

interface S2C_Pong { t0: number; t1: number; }
interface S2C_Error { code: string; message?: string; }
interface S2C_Finish { reason: "room_closed" | "timeout" | "error"; }
interface S2C_PlayerPresence { id: string; state: "active" | "disconnected" | "left"; }
interface S2C_RoleChanged { newMasterId: string; }
```