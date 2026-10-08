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

## Current state after Stage 4

- L5 control/motion is split as the plan intends: `traffic::following` owns the longitudinal
  command (`BehaviorEnvelope`, `LongitudinalDemand`, the IDM/ACC following, curvature, stop
  and limit composition) and the realized-motion reconciliation; `simulation::ai_motion`
  still owns steering, articulation and ground contact and now exposes realized travel
  (`AiBody::realized_speed`).
- `A7` single physical-pose owner: `traffic::perception::project_on_route_indices` is the
  allocation-free form of `project_on_route_local`; `AiState::commit_feedback(&Network,
  RealizedMotion)` projects the realized body on the planned route and commits lane, distance
  and realized speed, rejecting off-route projections. `core::Traffic::tick` runs one
  sequential read-back pass after the Rayon body/script block, so planner progress (and the
  stop distance derived from it) follows the body.
- `VehicleCapabilities::braking()` returns a `BrakingCapability`: the verified element-4 stop
  correction plus a provisional class braking strength with explicit provenance
  (`BrakeSource`); all five raw values are preserved. `core` sets the physical class of `-1`
  timetable buses so the fallback matches the vehicle.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `crates/traffic/tests/s4_leader_brake.rs`, `s4_launch_waves.rs`,
  `s4_frame_rate_motion.rs`, `s4_stop_anticipation.rs`; `VehicleSnapshot` gained
  commanded/realized feedback and `TRACE_VERSION` is 2.
- The Stage 4 replacement reason trail is recorded in `BACKLOG.md`.

### Stage 4 remaining (documented, not claimed done)

- Junction admission (Stage 5) and bus service/berth ownership (Stage 6) are unchanged;
  because `state.s` is now realized, their existing stop/distance use is realized-based but
  they have not been re-architected.
- `commit_feedback` is best-effort during a lane change and `ai_motion` keeps a small `along`
  catch-up term; narrowing that belongs with the lateral-maneuver owner (Stage 7).
- Braking strength stays a provisional class fallback until `[ai_brakeperformance]`'s other
  values are established.

## Current state after Stage 5

- `crates/traffic/src/junctions.rs` (L4): the single owner of junction admission,
  commitments, fairness, release and the wait-for graph.
  - `Movement` + `junction_ahead` (the explicit conflict areas of a crossing object),
    `light_at_entry`, `time_to`, `crossing_arrival`.
  - `JunctionActor` / `JunctionScene`: the frozen per-tick inputs; the coordinator never
    touches another vehicle.
  - `JunctionCoordinator`: `begin_tick`, `plan -> JunctionDecision { light, yield_at, state,
    reasons, binding }`, `restore_claim`/`release`/`retain_on_way`/`invalidate_network`,
    `blocked_by`/`wait_for_graph`, and `classify_waits -> (WaitDiagnosis, Vec<Recovery>)`.
  - `BlockMode { Occupy, Reserve, Oncoming }` and `block_mode_between`: the typed
    `[blockpath]` semantics, honored before the geometric convention; `Lane::crossing_problem`
    is a keep-clear path.
- `core::Traffic::tick` is the adapter: it builds the actor array and signal-aspect map once
  per tick, calls `begin_tick` once, and calls `plan` per vehicle with the frozen scene. It
  applies `light`/`yield_at`/`JunctionState`/`blocked_by`; junction claims and exit storage
  live in the coordinator, so `AiCar::reserved`/`amber` were removed.
- `LONG_WAIT_CLAIM`/`GRIDLOCK_WAIT` and their escape branches are gone (`D7`). Release runs
  on removal (`remove_car`, the tick removal loop), route change (`retain_on_way`),
  population reset and network growth (`invalidate_network`).
- `VehicleSnapshot` gained `junction_state` and `junction_blocker`; `TRACE_VERSION` is 3 and
  both feed the rolling decision/event hash.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `crates/traffic/tests/s5_four_way.rs`, `s5_priority_turns.rs`,
  `s5_blocked_exit_recovery.rs`, `s5_wait_for_graph.rs`, `s5_crossing_blocks.rs`,
  `s5_crossings.rs`; unit tests in `junctions`.
- The replacement reason trail is recorded in `BACKLOG.md` under
  "Stage 5 replacement reason trail".

### Stage 5 remaining (documented, not claimed done)

- Bus service and berth ownership (`ServicePhase`) are Stage 6; exit storage is reserved, but
  no berth is assigned.
- Lane changes, passing and parking are Stage 7; the coordinator does not own lateral intent.
- Population/streaming backpressure and dormant lifecycle are Stage 8.
- `JunctionScene` still carries caller-built `on_lane`/`coming` index views; migrating them
  onto the id-keyed `Occupancy` is a later cleanup.

## Deletion rule

Do not remove the shim or any compatibility re-export until its callers have migrated to
`crates/traffic` directly (see `TEST_INVENTORY.md` for the caller groups). A temporary
re-export must have a named migration target in the Stage 1 backlog.
