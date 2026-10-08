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
- `simulation::traffic` was a shim (`pub use ::traffic::*;`) during migration. Stage 9
  migrated every caller to import `traffic` directly and **deleted the shim**; `core` now
  depends on `traffic` directly. `simulation` has no runtime dependency on `traffic`.
  Stage 10 adds a **dev-dependency** for the closed-loop regression that couples the real
  bicycle body to planner feedback; it does not restore the runtime shim.
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

## Current state after Stage 6

- `crates/traffic/src/service.rs` (L3/L4) is the single owner of the scheduled bus service
  state machine and berth capacity, mirroring `junctions`:
  - `BerthGeometry` (stop, occurrence, lane, side, docking `s`/`bay`, `stop_length`,
    `boarding_region`, `approach_distance`, `berths`) keeps the four lengths separate (`D6`);
    `from_target` builds it from a `StopTarget` and the network lane.
  - `ServiceState` (phase, timers, layover, held berth, fault) is written only by
    `ServiceCoordinator::plan`; `ServiceActor`/`ServiceScene` are the frozen per-tick inputs
    (`net`, realized `Occupancy`, actors, clock). `ServiceDecision` carries the stop distance,
    lateral target, blinker, door side, boarding permission, stop advance, berth release and
    the emitted `TraceEvent`s.
  - `ServiceCoordinator::begin_tick` records stable arrival order and releases berths whose
    owner is gone; `plan` advances the explicit phase table, grants/holds/releases the one
    berth per stop, and classifies script feedback; `release`/`retain_on_way`/
    `invalidate_network` mirror the junction hooks.
  - `ScriptFeedback` types the `AI_Scheduled_AtStation` handshake: acknowledged, unsupported
    (fixed-close fallback), stuck-unknown (timeout -> `Fault(StationRelease)`) and
    stuck-open (never departed) (`D5`).
- `crates/core/src/bus_service.rs` is an adapter: it keeps the compiled `StopTarget`s, the
  terminus/displays and the always/early policy, maps script feedback, and projects the phase
  to `at_station`/`at_station_side`. `stop_shift` stays (it needs `simulation::VehicleType`)
  and feeds `stop_correction`.
- `core::Traffic` owns a `ServiceCoordinator`; `Traffic::tick` builds the `ServiceActor` array
  and berth intents once, calls `begin_tick` once and `plan` per bus, and applies the
  decision. Removal, route change, population reset and network invalidation release berths.
  `AiCar::boarding_permission()` is shared with the passenger simulation.
- `ROUTE_WAIT_MAX` and its random-traffic escape are gone; a bus at the loaded frontier waits
  in `RoutePending` and keeps its stops.
- `VehicleSnapshot` gained `service_phase`, `berth_owner` and `service_stop`; `TRACE_VERSION`
  is 4.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `crates/traffic/tests/s6_shared_stop.rs`, `s6_berth_recovery.rs`,
  `s6_optional_stops.rs`, `s6_script_handshake.rs`, `s6_duty_lifecycle.rs` (with the
  `common::service` fixture); unit tests in `service`.
- The replacement reason trail is recorded in `BACKLOG.md` under
  "Stage 6 replacement reason trail".

### Stage 6 remaining (documented, not claimed done)

- Lane changes, passing and parking (the docking S-curve among them) are Stage 7; the service
  owner sets a lateral target but does not own the lateral trajectory.
- Multi-berth stops need validated content geometry; the type is capacity-aware but one berth
  is exercised.
- Population/streaming backpressure, dormant lifecycle and removal notification are Stage 8.
- `JunctionScene`'s caller-built `on_lane`/`coming` index views remain; the berth scene reads
  the id-keyed `Occupancy` directly.

## Current state after Stage 7

- `crates/traffic/src/maneuvers.rs` (L4) is the single owner of every lateral maneuver,
  mirroring `junctions` and `service`:
  - `ManeuverState` (per-vehicle memory: lane-change cooldown, passing plan, park plan,
    pull-out hold, discretionary dwell and the last-change side) is written only by
    `ManeuverCoordinator::plan`; the `Passing` and `ParkPlan` types moved here from core.
  - `ManeuverActor`/`ManeuverScene` are the frozen per-tick inputs (network, id-keyed
    `Occupancy`, realized actors); `ManeuverInputs` carries the requests other owners submit
    (the service berth lateral, the kerb swerve, parking/pull-out and the obstacle ahead).
  - `ManeuverDecision` is the typed output: lateral target, explicit S-curve ramp, lane-change
    command (`Change`/`RouteChange`/`Bypass`), indicator, acceleration cap, stop point,
    `ManeuverPhase` and reasons. `ManeuverCoordinator::service_lateral` approves the service
    owner's docking/merge-out request as a required maneuver.
  - `begin_tick` orders simultaneous lane changes by target lane and stable id (the lowest id
    wins), so the outcome does not depend on container order; `release`/`retain_on_way`/
    `invalidate_network` mirror the junction/service hooks. `required_target` is the shared
    helper core uses to submit the route/turn-lane wish as an intent.
- Priorities: safety/finish-or-abort first, then required maneuvers (route change, docking/
  departure, a committed park), then discretionary (overtake/keep-right), then optional
  passing. A discretionary change commits only after `DISCRETIONARY_DWELL` and is held to the
  `OSCILLATION_WINDOW` against flipping sides; a required change that cannot be taken waits
  legally before the lane end (`commit_or_wait`).
- Passing is optional and gated on the whole outbound+return trajectory: the oncoming gap over
  the whole maneuver (`oncoming_soon`), the return room (`merge_room`/`back_in_ramp`), a swept
  `Occupancy::swept_clearance` of the ghost path including the return, and pedestrians. An
  abort returns along the S-curve while `abortable`; otherwise it holds the committed portion
  and stops short of the obstruction; it never snaps laterally.
- Core is the adapter: `Traffic::tick` builds the `ManeuverActor` array and intents once, calls
  `begin_tick` once and `plan` per vehicle, and applies the single decision. `plan_pass`/
  `guard_pass`/`oncoming_block`/`light_wait`/`plan_lane_change`/`plan_bypass`/
  `plan_route_change` and the inline passing/parking/kerb lateral writes are gone;
  `obstacle_ahead`/`obstacle_from` stay for longitudinal following.
- `VehicleSnapshot` gained `maneuver_phase` and `maneuver_target`; `TRACE_VERSION` is 5.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios under `crates/traffic/tests/`: `s7_route_turn_lane.rs`,
  `s7_simultaneous_lane_change.rs`, `s7_blocked_bay.rs`, `s7_parking_pullout.rs`,
  `s7_oncoming_abort.rs`, `s7_articulated_clearance.rs`, `s7_no_oscillation.rs`, with the
  `common::maneuver` fixture and unit tests in `maneuvers`.
- The replacement reason trail is recorded in `BACKLOG.md` under
  "Stage 7 replacement reason trail".

### Stage 7 remaining (documented, not claimed done)

- `ev_AI_Horn` (`D4`) stays deferred to Stage 9: the reference proves only that the event
  exists, not its trigger, and presentation feedback must never resolve a blocked maneuver.
  `TrafficPriorityWarningNeeded`/`TrafficPriority` remain the script-facing warning path.
- The passing feasibility port keeps the existing algorithm's structure with an L2
  `Occupancy` sweep rather than the physical `AiBody::sweep_clearance` that stays in
  `simulation::ai_motion` for realization; calibrating the two against recorded scenes is
  Stage 9 work.
- Multi-lane parking and multi-berth stops still need validated content geometry; the
  maneuver owner parks in one space per lane.
- Population/streaming backpressure and dormant lifecycle are Stage 8.

## Current state after Stage 8

- `crates/traffic/src/population.rs` (L3/L4) is the single owner of demand, eligibility/
  admission, the dormant lifecycle and topology demand, mirroring `junctions`, `service` and
  `maneuvers`:
  - `PopulationCoordinator` holds the bounded request queue, per-entrance backpressure and
    retry timers, the dormant registry (identity, class, duty, progress), the topology demand
    and the capacity/backpressure counters. All fields are private.
  - `PopulationScene` (network, frozen `Occupancy`, `PopulationDemand`, `initial`, tick) and
    `DormantView` are the frozen per-pass inputs; `SpawnDecision`
    (`Admit`/`Deny(Reason)`/`Retry`) and `DormantDecision` are the typed outputs. `SpawnFacts`
    carries the content/runtime facts the domain cannot know (path, continuation, ground,
    visibility). `RemovalCause` classifies recovery by cause.
  - `begin_tick`/`plan`/`plan_dormant`/`release`/`retain_on_way`/`invalidate_network`/`clear`
    mirror the other owners' hooks. Admission is deterministic FIFO, bounded by `QUEUE_MAX`
    and `ADMIT_PER_PASS`; a busy entrance backs off (`ENTRANCE_BACKOFF`) and over-budget
    demand is denied with `AtCapacity`. Same-pass admissions are checked against each other.
  - The dormant registry owns identity/class/duty; the kinematic step and `Arc<VehicleType>`
    assets stay in the adapter. Reactivation validates ground, visibility and gap before any
    body is rebuilt; `release` is once-only for a known dormant actor.
- Core is the adapter: `Traffic` owns a `PopulationCoordinator`; `populate_seen` builds one
  frozen `Occupancy` per pass and registers sleeping cars and `fill_map` dormant cars;
  `populate_kind` submits bounded demand and applies the typed decisions (`pick_type` +
  `spawn_clear` stay asset-side); `wake_dormant` validates through `plan_dormant`; `spawn_bus`
  returns a typed `Result<usize, Reason>` and schedule maps capacity/entrance denials to
  `Placed::Busy`. `add_tiles`/`reset_population`/`remove_car`/the tick removal loop and the
  streaming removal branch all release through the coordinators exactly once.
- Topology demand: `Traffic::request_topology_tile`/`topology_centers`; schedule asks for the
  tile a waiting bus needs next and `app::drive_streaming` appends the wanted centres to
  `Streamer::update` (the streaming owner is unchanged).
- LAN stays host-authoritative with no replication-schema change: a mirror client makes no
  population decisions, and an authority change or time reset preserves duty ownership.
- `VehicleSnapshot` gained `lifecycle`; `TRACE_VERSION` is 6.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `crates/traffic/tests/s8_overload_backpressure.rs`,
  `s8_streaming_identity.rs`, `s8_dormant_reactivation.rs`, `s8_lan_authority.rs`, with the
  `common::population` fixture and unit tests in `population`.
- The replacement reason trail is recorded in `BACKLOG.md` under
  "Stage 8 replacement reason trail".

### Stage 8 remaining (documented, not claimed done)

- The dormant kinematic advance still runs in the adapter (`Traffic::advance_dormant`) because
  it needs the AI-list type pools; the domain owns the logical lifecycle and validation. Moving
  the kinematics into the domain would need the type-pool/lane data pushed into a scene.
- Performance and soak budgets (100/500/1000 vehicles, 60-minute streaming/time-reset runs)
  have no harness yet and stay provisional for Stage 9.

## Current state after Stage 9

- The replacement is the **only** production road-AI runtime. The `simulation::traffic`
  re-export shim and the `pub mod traffic` declaration are gone; the unused `traffic`
  dependency was removed from `simulation`, and `core` depends on `traffic` directly.
- The `RuntimeKind` / `selected()` / `OMSI_TRAFFIC_RUNTIME` session selector and the empty
  `core::traffic_runtime::{vehicles, passengers, presentation, replication}` stubs were
  removed; `core::traffic_runtime::content` remains as the content-to-capability adapter.
- No persisted or network traffic schema changed: `PROTOCOL` stays 7 and a `.osn` situation
  never stores AI traffic (it is rebuilt on load). Only the diagnostic capture schema changed
  (`TRACE_VERSION` 7, the `Horn` event).
- Parameter provenance is centralized; the units/rationale/provenance table and the
  remaining content limitations are in [MAINTAINER_GUIDE.md](MAINTAINER_GUIDE.md).
- Performance and soak evidence is in [PERFORMANCE.md](PERFORMANCE.md): the domain benchmark
  (`crates/traffic/benches/domain.rs`) and the accelerated 60-minute soak
  (`crates/traffic/tests/s9_soak.rs`).
- Stage 9b decomposed the L6 adapter: `core/src/traffic.rs` keeps the `Traffic` struct, its
  shared types and helpers (now ~1,180 lines from ~7,750), and the method groups live in
  `core/src/traffic/{car,viewer,loading,population,perception,presentation,network,lan,
  lifecycle,diagnostics,tick}.rs`. `Traffic::tick` is a named phase pipeline over an owned
  `TickFrame`. `AiCar` fields and the fleet accessors are `pub(crate)`, with `car_mut_by_id`
  for one-vehicle commands; broad mutation stays with the schedule/LAN adapter.
- Moving the dormant kinematics into the domain and full per-field `AiCar` command methods
  remain documented, deferred work (Stages 10+).

### Deletion rule (retired)

The Stage 1 rule — do not remove a shim/compatibility re-export until its callers migrate —
has been satisfied: every caller now imports `crates/traffic` directly, so the shim and the
migration-only runtime selector were deleted. New temporary re-exports should repeat the same
rule: a named migration target, removed once the callers move.
