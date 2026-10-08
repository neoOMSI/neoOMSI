# Traffic AI refactor — dependency and ownership

Stage 1 (PR batch B) creates the headless `traffic` domain crate and keeps a temporary
re-export shim in `simulation`. This note records the dependency direction and where each
responsibility is meant to live. It is a migration map, not a claim that every module below
already exists.

## Dependency direction

```text
core ─┐
      ├──> simulation ──> traffic ──> { glam, hashbrown, log }
core ─┘                          (never the reverse)
```

- `traffic` is a domain crate. It must **never** depend on `core`, `simulation`, `render`,
  `audio`, `map`, `scenery`, `content`, `legacy-script`, `network`, or any asset/OS runtime.
- Both `simulation` and `core` may depend on `traffic`.
- `simulation::traffic` is currently a shim (`pub use ::traffic::*;`) so existing callers
  keep working. The shim is deleted only after all callers import `traffic` directly.
- Check the invariant with `cargo tree -p traffic`: only `glam`, `hashbrown`, `log` and
  their transitive leaves may appear.

## Layer ownership (plan section 3)

| Layer | Owner | Responsibility |
| --- | --- | --- |
| L0 content integration | `core::traffic_runtime::content` (to create) | Translate scenery/spline/AI-list/timetable data into typed network/rule/service updates |
| L1 network and rules | `traffic` (`network/`, `rules`, `signals`) | Topology, lane geometry, legal movements, conflicts, signal programs |
| L2 world and perception | `traffic` (`world`, `perception`) | Entity identity, occupancy, local neighbors, route-relative observations |
| L3 intent and service | `traffic` (`routing`, `service`, `population`) | Route progress, stop service, maneuvers, admission requests |
| L4 interaction decisions | `traffic` (`junctions`, `maneuvers`) | Rules, arbitration, berth assignment, safe merges/passing |
| L5 control and motion | `traffic::following` + `simulation::ai_motion` | Longitudinal command; steering/pose realization |
| L6 engine integration | `core::traffic_runtime` (to create) | `VehicleInstance`, scripts, assets, rendering, audio, passengers, LAN |
| Cross-cutting diagnostics | `traffic::diagnostics` | Typed reasons, trace records, metrics, capture |

## Current state after Stage 1

- `crates/traffic/src/` is split by responsibility: `network.rs` (topology/geometry,
  `BlockRule`/`crossing_problem` content rules, versioned updates), `rules.rs` (path priority
  and per-group density), `signals.rs` (light programs), `following.rs` (`AiState`, `Lead`,
  lane-change/route state), `validation.rs` (`Network::validate`/`NetworkDefect`),
  `tests.rs` (the inline algorithm tests), plus the contract modules `ids.rs`,
  `capabilities.rs`, `routing.rs` (`RouteStatus`), `service.rs`
  (`compile_stop_target`/`StopTargetError`), `diagnostics.rs`. `lib.rs` is declarations and
  re-exports.
- `crates/simulation/src/traffic.rs` is a re-export shim (`pub use ::traffic::*;`).
- `crates/core/src/traffic_runtime/` holds `content.rs` (capability adapter) and the adapter
  modules `vehicles.rs`, `passengers.rs`, `presentation.rs`, `replication.rs`.
- `Traffic` state is **private**: every field is accessed through query/command methods
  (`car_count`, `cars`, `car`, `net`, `target`, `set_*`, `take_removed_scheduled`,
  `extend_scheduled_route`, `set_world_inputs`, `set_presentation`-style setters, …).
  Caller groups (read-only, presentation/audio, population/schedule, LAN) were migrated in
  batches C5a–C5d; no module outside `traffic` writes a `Traffic` field directly.
- Headless scenarios: `crates/traffic/tests/s1_red_queue.rs`, `s2_blocked_exit.rs`, and
  `s3_shared_stop.rs` run with no renderer or OMSI assets. `traffic::scenario` provides the
  runner; `advance_fixed_clock` makes the fixed tick frame-partition independent (tested).
- Automatic failure capture: `Traffic::enable_capture(path, capacity)` (from `OMSI_CAPTURE`)
  builds a `traffic::Capture`, samples each tick, and persists a self-contained text trace
  on the first trigger (stationary without a reason).
- Session-start runtime selector: `core::traffic_runtime::selected()` reads
  `OMSI_TRAFFIC_RUNTIME` once when `Traffic::new` runs; only `current` exists during
  migration. A live vehicle is never switched between state models.

### Stage 1 remaining (documented, not claimed done)

- `AiCar` still bundles behaviour with assets (`vehicle`, `render`, `trailer_renders`,
  `sounds`, `body`) and those fields are still public. Moving them behind a presentation
  store is Stage 3/6 work; the LAN mirror now applies host state through
  `Traffic::apply_host_car` instead of writing fields.
- Contract adoption: `AiCar.id` and the id-typed traffic state are `VehicleId`; `AiCar.why`
  is a typed `Reason` projected to the `OMSI_TRACE_AI` label; `trip_route` returns a
  `RouteStatus`; `StopTarget` has replaced `bus_service::Stop`; the timetable route compiler
  (`compile_route`/`bridge_gaps`/`way_between`) lives in `traffic::routing`.
- Scheduler, passenger exchange, pause, and time-reset are aligned at the clock boundary
  (the fixed tick `traffic::scenario::SIM_DT` + an explicit accumulator reset on time jumps)
  in both the window and offscreen paths; their internal redesign belongs to Stages 5–6.
- `simulation` keeps `LaneKind` (street/sidewalk/rail/air); rail/air motion stays in
  `simulation` and is adapted later rather than forced through car following.
- `following.rs` still bundles routing/maneuver state; finer `routing.rs`/`maneuvers.rs`/
  `junctions.rs` extraction belongs to Stages 3/5/7, not this move.

## Current state after Stage 3

- `crates/traffic/src/perception.rs` (L2): `BodyFootprint` (owner id, part, realized
  geometry, height range, lane placements), `LaneInterval`, `Occupancy` (lane-interval and
  spatial indexes built once per tick), route-relative observations (`nearest_ahead`,
  `crossing_approach`, `berth_occupancy`, `swept_clearance`, `pedestrian_clearance`), and
  `project_on_route_local` (local route reconciliation that refuses a nearby parallel road).
- `crates/traffic/src/world.rs` (L2/L4 substrate): immutable per-tick `Snapshot`,
  `Commit` (previous blocker/claims keyed by `VehicleId`), and `Arbiter` (deterministic
  claims, simultaneous-merge winner by id, exit storage reserved for all admitted vehicles).
- `core::Traffic::tick` builds the `Occupancy` from `body_feet` (AI bodies, trailers/rear
  sections sharing the owner id, player/LAN outlines) and derives the id-keyed `by_lane`
  view from it. `body_in_way` sweeps through `Occupancy::swept_clearance`; junction claims
  and exit storage go through `Arbiter`; `geo_prev`, the merge tie-break, and
  `break_lead_pairs` are id-keyed. `lan_outlines` now carries remote trailers.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves), and `log` (`cargo tree -p
  traffic`). The `simulation::traffic` shim is unchanged and still used by `core`.
- Exit-gate scenarios: `crates/traffic/tests/s3_reorder_invariance.rs`,
  `s3_bus_rear_junction.rs`, `s3_external_trailers.rs`, `s3_arbitration.rs`.
- The replacement reason trail for every removed previous-frame/ad-hoc check is recorded in
  `BACKLOG.md` under "Stage 3 replacement reason trail".

### Stage 3 remaining (documented, not claimed done)

- Junction admission (priority, `GRIDLOCK_WAIT`, per-path semantics) is Stage 5; the
  arbiter only owns reservation bookkeeping and exit storage here.
- The service state machine and berth ownership are Stage 6; `berth_occupancy` exists but is
  not yet the stop-phase owner.
- Motion realization is not yet reconciled from body feedback; `project_on_route_local` is
  tested but single-pose ownership is Stage 4.
- The `Traffic` orchestrator is still large; `junctions.rs`/`maneuvers.rs`/`service.rs`
  extraction remains Stages 5–7.

## Deletion rule

Do not remove the shim or any compatibility re-export until its callers have migrated to
`crates/traffic` directly (see `TEST_INVENTORY.md` for the caller groups). A temporary
re-export must have a named migration target in the Stage 1 backlog.
