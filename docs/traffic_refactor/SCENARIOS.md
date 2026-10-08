# Traffic AI refactor — Stage 0 scenario specifications

These are the first three synthetic scenario specifications required by Stage 0. They are
deliberately challenging fixtures, not reproductions of the reported screenshot; they
exercise queue discharge, a blocked junction exit, and shared-stop service. They become
executable in Stage 1 with the headless runner (`A9`) and expand as later stages land.

All bounds below are **provisional**. They are initial expectations used to design the
fixtures and the capture format; numerical tolerances are finalised when the relevant stage
is runnable, and performance budgets are frozen before Stage 9 cutover.

Trace fields referenced as `snapshot.*` / `event.*` are defined in [TRACE_SCHEMA.md](TRACE_SCHEMA.md).

## S1 — Queue discharge at a red signal

| Field | Value |
| --- | --- |
| Purpose | Verify no line crossing during red, safe spacing, and a bounded reaction-wave discharge after green |
| Owning stages | 3, 4, 5 (PR batches F, G, H) |
| Fixture | Single lane, one signal stop line, a long straight tail (~400 m) |
| Initial conditions | 20 AI cars queued at the stop line, bumper-to-bumper at rest, same class, seeded distinct personality traits; signal red with 20 s remaining green time on the cross movement |
| Input timeline | `t=0` red; hold red 20 s; green 30 s |
| Expected behaviour | Cars stop before the line with headway ≥ `min_gap`; none crosses during red/amber-impossible; after green the queue starts and clears with bounded reaction delay per car and no overlapping bodies |
| Provisional bounds | Zero stop-line crossings derived from realised front bumpers during red; first follower moves within 1.5 s of the leader; each car's start delay grows with its stable trait; full queue clears within a deadline derived from length/accel/reaction (recorded, not a global constant) |
| Failure modes captured | Body overlap; crossing on red; brake/accelerate flicker; launch timer reset by brief constraint fluctuation; synchronised (non-varied) launch |
| Required trace fields | `snapshot.signal_state`, `snapshot.pose/footprint/velocity`, `snapshot.constraints` (`RedSignal`, `Leader`), `snapshot.binding`, `snapshot.route_progress` |

## S2 — Blocked junction exit

| Field | Value |
| --- | --- |
| Purpose | Verify a vehicle never enters a junction it cannot clear, and that flow recovers when downstream space opens — with no timer override |
| Owning stages | 3, 4, 5 (PR batches F, G, H) |
| Fixture | Two-lane cross junction; one approach with a stop line; short downstream storage that can hold one vehicle beyond the conflict |
| Initial conditions | Downstream storage fully occupied by a stopped blocker; an AI car approaches the entry with a green/right-of-way movement; a second and third car queue behind |
| Input timeline | Hold the downstream blocker for 50 s (> `GRIDLOCK_WAIT`); remove it at `t=50`; observe recovery |
| Expected behaviour | Entry car waits outside the conflict area for the full blockage; waiting duration never grants entry; on blocker removal the car enters, clears, and the queue discharges |
| Provisional bounds | No entry while all downstream storage is occupied; on removal, first movement within a scenario-derived deadline, full recovery within another deadline; conflict area occupancy never overlaps |
| Failure modes captured | Occupied-exit entry; `GRIDLOCK_WAIT` timer override; speculative claims surviving the block; stale claim after removal; deadlock between two approaches |
| Required trace fields | `snapshot.junction_state` (`Approaching`/`Waiting`/`Admitted`/`Inside`/`Cleared`), `snapshot.constraints` (`OccupiedExit`, `Yield`), `snapshot.occupancy`, `event.*` where applicable |

## S3 — Three buses sharing one curb stop

| Field | Value |
| --- | --- |
| Purpose | Verify one berth occupant, safe upstream waiting, once-only service, and no doors opening far up the queue |
| Owning stages | 3, 6 (PR batches F, I, J) |
| Fixture | Single lane beside a curb stop with one boarding region; a following car lane so merge-back matters |
| Initial conditions | Three scheduled buses with distinct duties arrive within a few seconds of each other; the stop has waiting passengers with stop wishes; a random car follows the third bus |
| Input timeline | Bus 1 arrives and boards for a long dwell; buses 2 and 3 queue; buses depart in order; a passenger in the doorway holds one departure briefly |
| Expected behaviour | Bus 1 occupies the berth and boards only at valid geometry; buses 2 and 3 wait upstream with safe spacing and advance only when a berth clears; each bus serves the stop exactly once and departs only after doorway holds clear and a safe merge; the following car is not passed unsafely |
| Provisional bounds | Longitudinal docking error ≤ 0.5 m, lateral ≤ 0.25 m, speed < 0.1 m/s before boarding permission (provisional, against boarding geometry not the stop origin); exactly one active berth owner at a time; zero boarding-permission events while a bus is upstream |
| Failure modes captured | False arrival from time-in-queue; doors opening in the queue; two berth owners; duplicate duty spawn; departure with a held/unsafe door; merge conflict with the follower |
| Required trace fields | `snapshot.service_phase`, `snapshot.berth_owner`, `snapshot.constraints` (`BerthBusy`, `DoorHold`, `Leader`), `snapshot.route_progress`/stop occurrence, `event.arrival`, `event.boarding_permission`, `event.close_request`, `event.departure`, `event.fault` |

## Stage 5 scenarios (headless, at the coordinator level)

Stage 5 makes the junction half of S1/S2 executable against the real
`traffic::junctions` coordinator (no renderer, no OMSI assets). The fixtures live under
`crates/traffic/tests/`:

| File | Required behaviour |
| --- | --- |
| `s5_four_way.rs` | Conflicting claims admit exactly one movement; the outcome is id-keyed, not container-order dependent. |
| `s5_priority_turns.rs` | A `[rule] priority` road goes before an unmarked side road; a left turn waits for the oncoming traffic. |
| `s5_blocked_exit_recovery.rs` | A full downstream exit holds the vehicle out for a full minute (no `GRIDLOCK_WAIT`); removing the blocker resumes flow and the claim is taken. |
| `s5_wait_for_graph.rs` | A real hold is recorded in the wait-for graph and is not mislabelled a deadlock; the classification itself is unit-tested in `junctions` (stale claim vs legal congestion vs full capacity). |
| `s5_crossing_blocks.rs` | `[crossingproblem]` refuses entry; `[blockpath]` `Reserve` refuses a reservation that `Occupy` ignores; `Oncoming` waits for the other side to commit. |
| `s5_crossings.rs` | A pedestrian on a crossing and a train on a level crossing hold the street until they clear. |

## Stage 6 scenarios (headless, at the service coordinator level)

Stage 6 makes S3 executable against the real `traffic::service` coordinator (no renderer, no
OMSI assets). The fixtures live under `crates/traffic/tests/` with the `common::service`
kinematic bus fixture: a `ServiceCoordinator`, one straight lane, and buses that obey the
`stop_at` the coordinator hands back while the coordinator reads their realized occupancy.

| File | Required behaviour |
| --- | --- |
| `s6_shared_stop.rs` | Three buses share one berth: safe upstream queueing, exactly one berth owner and boarder at a time, each serves the stop once, and a follower does not dock through a departing bus. |
| `s6_berth_recovery.rs` | A stop occupied by the player (or another body) is waited for and served only once it clears; an overshoot records a missed/faulted stop and never opens the doors up the queue. |
| `s6_optional_stops.rs` | Optional/request stops, timing points, early/late service, long boarding held at the door, and a layover waiting out its departure in the bay. |
| `s6_script_handshake.rs` | Acknowledged departure after the minimum close, unsupported scripts on the fixed fallback, a stuck script timing out with a fault, a script reporting open doors never driven off, and a doorway hold deferring the close request. |
| `s6_duty_lifecycle.rs` | Terminus handover to `NextTrip`, `OutOfService`, a diagnosed `RoutePending`, a clean next-trip reset, and two different duties departing in parallel. |

## Provisional measurement targets

NeoOMSI targets, not constants established by the reference report. Each is calibrated as
its stage becomes runnable; performance and soak budgets are frozen before Stage 9 cutover.

| Area | Provisional target | Finalised at |
| --- | --- | --- |
| Safety | Zero AI-created body overlaps and conflicting grants in feasible scenarios; zero red-entry, prohibited-direction, and prohibited-movement violations (swept geometry, trailers included) | Stages 3–5 |
| Docking | Longitudinal error ≤ 0.5 m, lateral ≤ 0.25 m, speed < 0.1 m/s before boarding permission, measured against boarding geometry | Stage 6 |
| Progress | Each clearance scenario has a deadline from blocker removal to first movement and to full discharge, derived from route length, acceleration, and reaction limits; no single global timeout | Stages 4–5 |
| Service | Exactly one physical/logical owner per duty instance; once-only served/skipped stop events; zero false arrivals and duplicate trip spawns; zero departures with known unsafe door/doorway state | Stage 6 |
| Natural motion | Record acceleration, jerk, lateral acceleration, steering rate, headway, emergency-brake frequency, unnecessary stop/restart cycles, manoeuvre reversals, and delay after a usable gap; normal driving within the comfort envelope, emergency exceptions carry a reason | Stages 4, 9 |
| Replay | Seeded repeat runs preserve decision/event hashes on the same platform; reordered storage or worker count has no semantic effect; cross-platform uses documented float tolerances | Stage 1, 9 |
| Performance | Benchmarks at 100, 500, and 1000 active road vehicles under ordinary and congested junction loads; record p50/p95/p99 tick cost, allocation rate, memory, streaming cost on named hardware | Stage 9 (no harness exists yet) |
| Soak | 60-minute seeded dense mixed-traffic run plus repeated streaming/time-reset cycles; no unresolved claim/berth/duty leak, no unbounded pending-queue growth, no unexplained persistent stall | Stage 9 |

## Exit gate

The three scenario specifications and their required trace fields are defined; Stage 1 can
make S1–S3 executable without a live reproduction of the original queue.
