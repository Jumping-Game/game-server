# NETWORK_PROTOCOL v1.2.1 Alignment Report

## Deviations Observed
- **Post-start snapshots** were delayed until the next 100 ms tick and could be deltas; reconnect-triggered snapshots were also delayed. This violated the spec requirement for an immediate full snapshot after `start` and on reconnect.
- **Snapshot payload shape** omitted `characterId`, so characters chosen in the lobby were not preserved into authoritative state updates.
- **Input acceptance window** was not enforced in the simulation loop, allowing arbitrarily old or far-future inputs.

## Fixes Applied
- Reworked the simulation broadcaster to emit an immediate full snapshot when the room transitions to `running`, to repeat full snapshots at least once per second, and to push an immediate full snapshot on reconnect while tracking dropped frames.
- Extended snapshot construction to carry each player’s `characterId`, ensuring parity with lobby/start rosters.
- Added input window validation (`[tick - maxRollbackTicks, tick + inputLeadTicks]`) so the server discards invalid inputs per spec.
- Expanded integration coverage to lock character selection, assert the start roster, verify full snapshots (including reconnect), and ensure non-masters receive `NOT_MASTER`.

