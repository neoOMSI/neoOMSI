# Traffic AI refactor — maintainer guide (Stage 9)

Concise walkthrough of the replacement traffic system: who owns what, how to explain a
vehicle's behaviour, where the tuning parameters live and what they mean, and the known
remaining limitations. Read with [DEPENDENCIES.md](DEPENDENCIES.md) (layer ownership) and
[TRACE_SCHEMA.md](TRACE_SCHEMA.md) (decision/event capture).

## One runtime

There is exactly **one** production road-AI implementation: the headless `traffic` domain
crate driven by `core::Traffic` (the adapter). Stage 9 removed the `simulation::traffic`
re-export shim and the `RuntimeKind`/`OMSI_TRAFFIC_RUNTIME` migration selector, so there is no
second engine to shadow or roll back to. `traffic` depends only on `glam`, `hashbrown`, `log`
(verify with `cargo tree -p traffic`).

## Ownership map

| Layer | Owner | State it owns |
| --- | --- | --- |
| L1 network/rules/signals | `traffic::{network, rules, signals}` | Lanes, links, conflicts, `BlockMode`/`crossing_problem`, signal programs, version |
| L2 world/perception | `traffic::{perception, world}` | `BodyFootprint`, `Occupancy`, `Snapshot`, `Commit`, `Arbiter` |
| L3 intent/service | `traffic::{routing, service, population}` | Route progress, service phase + berths, demand/admission + dormant registry |
| L4 interaction | `traffic::{junctions, maneuvers}` | Junction admission/claims/wait-for graph; every lateral maneuver |
| L5 control/motion | `traffic::following` + `simulation::ai_motion` | Longitudinal command + envelope; steering/pose realization |
| L6 integration | `core::Traffic` + `core::traffic_runtime::content` | `VehicleInstance`, scripts, assets, audio, passengers, LAN |

A private module state has one writer: only `ServiceCoordinator` changes the service phase,
only `ManeuverCoordinator` owns lateral intent, only `JunctionCoordinator` creates/releases
junction commitments. Presentation reads committed state; debugging never grants movement.

## Explaining a vehicle

Every owner reads a frozen per-tick scene and returns a typed decision, and diagnostics
project it into a `VehicleSnapshot` (see [TRACE_SCHEMA.md](TRACE_SCHEMA.md)). For a selected
vehicle you can see:

- **route progress** — lane plus distance along the directed route occurrence
  (`AiState::lane`/`s`, `RouteProgress`);
- **actual footprints** — `Occupancy`/`BodyFootprint`, including trailers/rear sections that
  share the owner id and differ by `part`;
- **intended maneuver** — `maneuver_phase` / `maneuver_target` from `ManeuverCoordinator`;
- **applicable rules and all blockers** — the `Reason` list, with the **binding constraint**
  identified separately (`VehicleSnapshot::constraints`/`binding`, `AiCar::why`);
- **berth/claim owner** — `service_phase`/`berth_owner`/`service_stop`, and
  `JunctionCoordinator::claims_of`/`state_of`/`blocked_by`;
- **next transition guard** — the explicit phase table in `service.rs`, the
  `Approaching → Waiting → Admitted → Inside → Cleared` junction states, and the maneuver
  start/abort conditions.

`OMSI_TRACE_AI=<file.csv>` dumps the per-frame projection; `OMSI_CAPTURE=<file>` persists a
rolling decision/event capture on the first trigger.

## Parameters (units and provenance)

Every tuning constant carries its unit in the name and its rationale on the item; the owner
module's header names the provenance class:

- **content** — imported from map/vehicle data (e.g. `[ai_brakeperformance]` element 4, path
  priority, signal programs);
- **observed** — kept for OMSI compatibility;
- **improvement** — a deliberate neoOMSI change (plan section 1);
- **provisional** — unverified tuning, expected to be recalibrated.

| Owner | Examples (unit) | Provenance |
| --- | --- | --- |
| `following` | `MAX_BRAKE` (m/s²), `STOP_LINE_GAP` (m), `PLAN_AHEAD` (m), `BehaviorEnvelope` (m/s², m/s³, s) | improvement/provisional |
| `service` | `DOCK_LONG_TOL` 0.5 (m), `DOCK_LAT_TOL` 0.25 (m), `DOCK_SPEED` 0.1 (m/s), `CLOSE_MIN`/`CLOSE_MAX` (s), `EARLY_WAIT`/`LAYOVER_WAIT` (s) | docking tolerances = plan target; service lengths provisional; timing rules improvement |
| `maneuvers` | `ONCOMING_ROOM` (m), `CHANGE_COOLDOWN`/`DISCRETIONARY_DWELL`/`OSCILLATION_WINDOW` (s), `TURN_LANE_LOOKAHEAD` (m) | improvement/provisional |
| `population` | `QUEUE_MAX` 256, `ADMIT_PER_PASS` 64, `ENTRANCE_BACKOFF`/`RETRY_MIN`/`RETRY_MAX` (s), `GAP_MARGIN` (m), `DORMANT_CAP_FACTOR` | provisional |
| `junctions` | `DECIDE_MIN`/`DECIDE_MAX`/`DECIDE_RELEASE` (m), `AT_LINE` (m) | provisional; legality is content |
| `network` | `MEET_CLEARANCE` (m, height-aware conflicts), `GRID_CELL` (m) | improvement |
| `perception` | `CELL` (m, spatial grid) | improvement |

Shared physical bounds and driver diversity live in `BehaviorEnvelope` and the seeded
per-vehicle traits (`desire`, `headway`, `min_gap`, `accept_gap`, `reaction`), which are
persistent, not per-frame random.

## Running the checks

```text
cargo tree -p traffic                       # only glam/hashbrown/log (+leaves)
cargo bench -p traffic --bench domain       # 100/500/1000 headless domain tick cost
cargo test  -p traffic --test s9_soak
cargo test  -p traffic --test s9_soak -- --ignored   # 60-minute accelerated soak
cargo test --workspace
```

See [PERFORMANCE.md](PERFORMANCE.md) for the measured numbers and budgets.

## Recovery rule

Recovery never changes legality, erases physical occupancy, or silently abandons a scheduled
service. A stale claim is cancelled and retried; legal congestion and a physically full road
wait and are diagnosed; an invalid scheduled route is faulted and reported. A timeout never
authorizes crossing a conflicting body or a red signal.

## Remaining limitations (documented, not hidden)

- **`[ai_brakeperformance]`** — only element 4 (the stop-holding correction) is verified; the
  braking strength uses an explicit provisional class fallback (`BrakingCapability`,
  `BrakeSource::ClassFallback`). The raw values are preserved.
- **Multi-berth / multi-lane parking** — one berth/space per stop/lane is the validated
  default; `BerthGeometry::berths` is capacity-aware but needs content geometry to exercise.
- **Dormant kinematics** — the logical dormant lifecycle is in `traffic::population`, but the
  kinematic step stays in `Traffic::advance_dormant` because it needs the AI-list type pools.
- **`AiCar` encapsulation** — the struct's fields are still public and `cars_mut`/`car_mut`
  remain; full encapsulation is deferred (Stage 10+).
- **`ev_AI_Horn`** — re-added through the script adapter with a cooldown and a `Horn` trace
  event. The reference proves the event exists but not its original trigger, so the trigger
  is a documented provisional neoOMSI improvement; it is presentation feedback only and never
  resolves a blocked maneuver.
- **Screenshot reproduction** — the originally reported rare queue was never reproduced; it is
  recorded as unknown and is not claimed fixed.
