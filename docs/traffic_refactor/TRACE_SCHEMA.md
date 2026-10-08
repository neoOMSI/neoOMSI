# Traffic AI refactor — decision/blocked-reason capture specification

This specifies the minimum diagnostic capture to add at the Stage 1 seam, and the first
extraction boundary. The capture must explain every vehicle decision without changing it.
The existing `OMSI_TRACE_AI` frame dump and `AiCar` hints (see
[TEST_INVENTORY.md](TEST_INVENTORY.md)) are the starting point; this schema is versioned and
replayable, unlike those ad-hoc dumps.

## Versioning

- `TRACE_VERSION` is a single integer. Any field addition, removal, or semantic change bumps
  it. It is currently **5**: Stage 7 added the per-vehicle `maneuver_phase`
  (`Idle`/`RouteChange`/`LaneChange`/`Passing`/`PassingAbort`/`Parking`/`PullOut`/`Docking`/
  `Departing`) and `maneuver_target` (the lane a lane change is moving over to), the single
  writer being `traffic::maneuvers`; Stage 6 had added `service_phase`, `berth_owner` and
  `service_stop`; Stage 5 had added `junction_state`/`junction_blocker`; Stage 4 had added the
  `motion_feedback` fields.
- A capture writes a header record containing `trace_version`, `source_revision`, `platform`,
  `seed`, `tick_hz`, `network_version`, and the ordered-input digest.
- Unknown fields are read as absent; readers reject a mismatched major `trace_version`.

## Ordered inputs (replay boundary)

A capture is replayable only if the inputs are ordered and timestamped before they mutate
domain state. Capture these once per tick, before the snapshot is frozen:

| Field | Meaning |
| --- | --- |
| `tick` | Monotonic simulation tick index |
| `sim_time` | Elapsed simulation time (s), distinct from calendar/service time |
| `calendar_time` | `day_time` used by schedules |
| `network_update` | Ordered map/tile add/remove with `network_version` |
| `script_feedback` | Timestamped vehicle script outputs (door/station/release) |
| `passenger_requests` | Stop wishes and door requests with stable person IDs |
| `schedule_commands` | Duty/trip assignments and handovers |
| `external_actors` | Player, remote/LAN, parked, trailer, rail, pedestrian inputs with stable IDs |
| `player_remote_input` | Timestamped player/remote control state for deterministic replay |

## Per-tick snapshot record

One record describes the frozen world for a tick. Spatial indexes are derived from these
realised bodies; they are not stored.

| Field | Type | Notes |
| --- | --- | --- |
| `tick`, `sim_time`, `network_version` | u64, f64, u64 | Identity of the tick |
| `signal_state[]` | list | Each signal: stable id, aspect, remaining/phase, request state |
| `vehicles[]` | list | See below |

Per vehicle:

| Field | Type | Notes |
| --- | --- | --- |
| `id` | `VehicleId` | Stable, survives container reordering |
| `class`, `role` | enum | Physical class; bus service role and duty identity separate |
| `pose` | pos + yaw/pitch/bank | Realised physical pose (single pose owner) |
| `velocity` | v + yaw rate | Realised, not controller-desired |
| `footprint[]` | list | Swept/extent segments including trailers/articulation |
| `route` | `(TripId, route occurrence, progress)` | Lane plus distance along the directed occurrence |
| `constraints[]` | list | **All** active causes, not just the nearest |
| `binding_constraint` | ref/id or none | The constraint that currently binds |
| `service_phase` | enum | See below |
| `junction_state` | enum | See below |
| `junction_blocker` | id or none | The vehicle it currently waits for at a junction |
| `service_phase` | enum | See below |
| `berth_owner` | id or none | Who owns the berth the vehicle is at or waiting for |
| `service_stop` | `StopId` or none | The stop whose berth it is at or waiting for |
| `maneuver_phase` | enum | See below; written only by `traffic::maneuvers` |
| `maneuver_target` | `LaneId` or none | The lane a lane change is moving over to this tick |
| `motion_feedback` | | Commanded vs realised accel/speed, applied steering/speed bounds |
| `why` | reason + gap | Convenience projection of the binding constraint |

## Constraint / reason candidates

A constraint carries `reason`, `owner` (blocker/claim id), `validity`, `provenance`, and
either a route-relative stopping location or a speed bound. Preserve every active cause and
identify the binding one separately. Initial reason set:

```text
RedSignal, Amber, Yield, OccupiedExit, JunctionClaim, Leader, Pedestrian,
BerthBusy, DoorHold, StationRelease, RoutePending, InvalidRoute, SpeedLimit,
Curvature, StopTarget, MissedStop, ScriptTimeout, Parking, PullOut, Passing,
Emergency, StaleClaim, Unknown(u16)
```

`Unknown(..)` exists so unresolved legacy mechanisms (for example the exact
`[crossingproblem]` / `[blockpath]` semantics) are captured as data with provenance rather
than discarded or guessed.

## Junction and service state enums

```text
JunctionState = Approaching | Waiting | Admitted | Inside | Cleared
ServicePhase  = EnRoute | Approach | WaitingForBerth | Docking | Boarding
              | ClosingDoors | WaitingToMerge | Departing
              | Layover | NextTrip | OutOfService | RoutePending | ServiceFault(reason)
ManeuverPhase = Idle | RouteChange | LaneChange | Passing | PassingAbort
              | Parking | PullOut | Docking | Departing
```

Some phases may share code, but their transition conditions must remain explicit. A free
curb stop may pass through `WaitingForBerth` immediately. `ManeuverPhase` is the lateral half
of a maneuver and is owned by `traffic::maneuvers`; `Docking`/`Departing` are the service
owner's berth lateral request as the maneuver owner approves it.

## Transition events

Emit exactly once per transition, each with `tick`, `sim_time`, and the stable IDs involved:

```text
StopArrival, BoardingPermission, CloseRequest, Departure, TripComplete,
DutyHandover, Fault(reason), Removal(reason), ClaimGranted, ClaimReleased,
BerthGranted, BerthReleased, SpawnAdmitted, SpawnDenied(reason)
```

`schedule`, passengers, scripts, and LAN adapters consume these through typed boundaries.
Removal must notify schedule and passengers and release resources exactly once.

## Valid waits vs errors

Classify waits to avoid treating legitimate service as failure:

| Class | Examples |
| --- | --- |
| Valid wait | Red/amber signal, boarding/dwell, timing-point or layover, physically full road, player obstruction |
| Suspect | Stationary or crawling with no valid-wait reason; cyclic blockers; contradictory claims; invalid route; impossible service transition |
| Error | Body overlap, conflicting grants, red entry, boarding permission away from a valid berth, departure with a held/unsafe door |

## Rolling trace and automatic capture

- Keep a bounded rolling buffer of the last N ticks (N set so a capture covers at least the
  longest scenario deadline; provisional default recorded with `TRACE_VERSION`).
- Automatically persist the buffer plus surrounding ordered inputs when a trigger fires:
  unexplained stationary/crawling, cyclic blocker detection, body overlap, contradictory
  claims, invalid route, or impossible service transition. `Emergency` reasons are also
  captured with context but classified separately.
- A capture is self-contained: header + ordered inputs + snapshot records + events. It must
  replay to the same decision/event hash on the same platform.

## Replay determinism

- Replay consumes the ordered inputs at the recorded ticks and must reproduce the same
  sequence of decisions and events. Compute a rolling **decision/event hash** over granted
  actions and emitted events; seed repeat runs must match on the same platform.
- Reordering storage or changing worker count must not change the hash. Cross-platform
  comparison uses documented floating-point tolerances, not bit-for-bit physics.
- Shadow comparison of the old runtime must be read-only: no scripts, events, spawns, or
  reservations. Once trajectories diverge, compare invariants and outcomes rather than
  exact trace equality.

## First extraction boundary

The first boundary, specified now and created in Stage 1, is a small headless
`crates/traffic` domain crate plus a `crates/core/src/traffic_runtime/` integration facade.
Dependency direction: `traffic` depends only on basic math and justified geometry
utilities; both `simulation` and `core` may depend on `traffic`; `traffic` never depends on
`core`, `simulation`, rendering, audio, map loading, scripting, or network runtime. The
crate name and layout below are proposals to check against workspace conventions before
creation.

```text
crates/traffic/src/
  lib.rs                  small public API; no gameplay implementation
  ids.rs                  VehicleId, LaneId, StopId, TripId, duty identity
  world.rs                authoritative domain state and step orchestration
  network/                topology, geometry queries, conflict areas, validation
  rules.rs                content-derived legal rules and movement permissions
  signals.rs              deterministic signal programs and requests
  perception.rs           occupancy indexes and route-relative observations
  routing.rs              planned route, progress, incremental extension
  following.rs            speed constraints, following, longitudinal control
  junctions.rs            admission, commitments, fairness, release
  maneuvers.rs            lane changes, docking, passing, parking transitions
  service.rs              scheduled stop and trip state transitions
  population.rs           demand/admission bookkeeping, not asset loading
  diagnostics.rs          typed reasons, trace records, health classification
crates/traffic/tests/     synthetic multi-vehicle behaviour scenarios
crates/core/src/traffic_runtime/
  mod.rs                  integration facade
  content.rs              map/route/vehicle capability translation
  vehicles.rs             motion realisation and script handshake
  passengers.rs           stop wishes, doorway holds, service events
  presentation.rs         rendering and audio synchronisation
  replication.rs          host snapshots and client presentation
```

Migration rule: keep `simulation::traffic` re-exports where necessary so scenery,
passengers, rail, and existing tests migrate separately. Shared network primitives retain
`LaneKind` for street, sidewalk, rail, and air; road decisions must not run on every path
kind. Keep specialised rail/air motion in `simulation` and adapt its occupancy rather than
forcing it through car following. Delete compatibility facades once callers have migrated.

## Exit gate

The capture schema, its reason/state enums, the automatic-capture triggers, and the first
extraction boundary are specified. Stage 1 can implement all three Stage 0 scenarios
against this schema without replay tooling or a live reproduction.
