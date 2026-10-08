# Traffic AI refactor — initial backlog

This is the starting backlog adopted by Stage 0. It carries forward the behaviour goals
from [section 1 of the plan](../TRAFFIC_AI_REFACTOR_PLAN.md#1-outcome-and-scope), the source
findings from section 2, and the targeted OMSI coverage audit. It is not a new
investigation; it turns those findings into individually trackable items with an owning
stage and PR batch.

Status values: `open` (not started), `in-progress`, `done`, `unknown` (semantics still to
be established), `uncertain` (source data or behaviour not proven). "Evidence" is where the
claim comes from; a `neoOMSI` reference is a source location, not a parity proof.

## A. Behaviour improvement workstreams

These are the mandatory Section 1 qualities, tracked for the whole refactor rather than a
final tuning pass.

| ID | Item | Owning stage | PR batch |
| --- | --- | --- | --- |
| `B1` | Anticipation: recognise queues, bends, speed reductions, stop approaches, merges early enough to brake smoothly | 3–4 | F, G |
| `B2` | Continuous control: bound jerk, steering rate, lateral acceleration; no flicker, crawling, or sideways gliding | 4 | G |
| `B3` | Credible variation: persistent seeded headway/reaction/comfort traits and launch waves without per-frame wobble | 4 | G |
| `B4` | Consistent intentions: signal before manoeuvres, commit when safe, no lane/go-wait oscillation | 5, 7 | H, K |
| `B5` | Social interaction: feasible gaps, useful space, cooperative merges; no waiting forever on phantom threats | 3, 5 | F, H |
| `B6` | Convincing bus service: deliberate docking, door/platform alignment, dwell, safe close and merge-out | 6 | I, J |
| `B7` | Honest congestion: queue only for real causes, recover naturally; no forced movement, overlap, or teleport | 5, 8 | H, L |

## B. Content and reference gaps

Derived from the Stage 0 coverage matrix in the plan (section 2) and its cited reference
dossiers under `H:/marcel_omsi/`. Each item preserves input information or replaces unsafe
behaviour; the exact semantics of unresolved fields stay `unknown` until their owning stage.

| ID | Item | Evidence | Owning stage | Status |
| --- | --- | --- | --- | --- |
| `D1` | Retain the per-path `[crossingproblem]` flag through network compilation and establish its decision semantics | [functions/007b432c.md](H:/marcel_omsi/functions/007b432c.md), [assembly/007b432c.asm](H:/marcel_omsi/assembly/007b432c.asm) `007b788a`/`007b78c6` | 2, 5 | done (Stage 5: keep-clear entry refusal) |
| `D2` | Retain both `[blockpath]` values as a typed block rule with mode; determine directional/admission/occupancy meaning | [functions/007b432c.md](H:/marcel_omsi/functions/007b432c.md), `007b78d3`/`007b79cb`/`007b7a4a` | 2, 3, 5 | done (Stage 5: `BlockMode` Occupy/Reserve/Oncoming) |
| `D3` | Audit and import all five `[ai_brakeperformance]` values; only element 4 is consumed today | [`crates/vehicle/src/vehicle/parse.rs`](../../crates/vehicle/src/vehicle/parse.rs), [`bus_service::stop_shift`](../../crates/core/src/bus_service.rs) | 2, 4, 6 | partial |
| `D4` | Add the `ev_AI_Horn` behaviour event through the script adapter with cooldown and diagnostics | `007db679`/`007db683`; [Traffic AI guide](H:/marcel_omsi/subsystems/traffic_ai.md) | 7, 9 | deferred (Stage 7: the reference proves the event, not its trigger; presentation feedback must not resolve a blocked maneuver — Stage 9) |
| `D5` | Honour script-facing station state (`AI_Scheduled_AtStation`) and separate safe fallback from unsafe doors | [functions/007eab20.md](H:/marcel_omsi/functions/007eab20.md), `007eb4bc` | 6 | done (Stage 6: typed `ScriptFeedback`) |
| `D6` | Re-establish stop length, boarding region, docking reach (`BAY_REACH`) and lateral placement as separate concepts | `00620004.asm` shows `00620058` is list traversal, not metadata; [`bus_service`](../../crates/core/src/bus_service.rs) | 2, 6 | done (Stage 6: `BerthGeometry`) |
| `D7` | Remove the unconditional full-exit override after `GRIDLOCK_WAIT`; models admission, occupancy, recovery separately | [`Traffic::junction_stop`](../../crates/core/src/traffic.rs) | 5 | done (Stage 5) |
| `D8` | Replace the queued/crept-past arrival shortcut and fixed early-wait caps with explicit berth/door geometry and service policy | [`BusService::approach/arrive/step`](../../crates/core/src/bus_service.rs) | 6 | done (Stage 6: `ServiceCoordinator`) |

## C. Target architecture and contracts

Carried from section 3 of the plan; specified (not implemented) in Stage 0 through
[TRACE_SCHEMA.md](TRACE_SCHEMA.md).

| ID | Item | Owning stage | PR batch |
| --- | --- | --- | --- |
| `A1` | Headless `crates/traffic` domain crate with the L0–L6 layer ownership | 1 | B, C |
| `A2` | Stable identities (`VehicleId`, `LaneId`, `StopId`, `TripId`, duty) surviving reordering | 1 | C |
| `A3` | Validated vehicle capabilities and route-progress/stop-coordinate types | 1–2 | C, E |
| `A4` | Typed blockers/constraints with owner, validity and binding cause | 1, 3 | C, F |
| `A5` | Fixed simulation clock and tick pipeline independent of redraw | 1 | D |
| `A6` | Immutable per-tick snapshot and deterministic arbitration | 3 | F |
| `A7` | Single physical-pose owner with realised-motion feedback | 3–4 | F, G |
| `A8` | Explicit service state machine, berth coordinator, duty lifecycle | 6 | I, J |
| `A9` | Diagnostics, rolling trace, automatic failure capture, headless scenario runner | 1, continuous | D |
| `A10` | Bounded demand/admission and streamed/dormant lifecycle with LAN host authority | 8 | L |

## D. Continuous validation track

Not a stage gate; runs alongside Stages 1–9.

- Short build → run scenario → inspect decisions → adjust cycles with saved seed/config/input.
- Bounded rolling trace plus automatic capture for unexplained stationary/crawling queues,
  cyclic blockers, overlaps, contradictory claims, invalid routes, and impossible service
  transitions.
- If the rare reported failure is captured during development, convert it into a
  regression case. Manual reproduction stays optional; a failure that becomes reproducible
  must be resolved before it is declared fixed.
- Focused OMSI comparison only when relevant content is available or a static inference
  needs clarification, recording source certainty and intended improvement separately.
- Progressive cost measurement from the first runnable seam; freeze acceptance envelopes
  before a behaviour's rollout and final budgets before Stage 9 cutover.

## Exit gate

The extraction boundary (`A1`) and the scenarios in [SCENARIOS.md](SCENARIOS.md) are
specified. No item above requires a live queue reproduction to begin Stage 1.

## Stage 1 progress

- `A1` extraction (batches B, split) done; `A2` stable ids introduced; `A3` capabilities +
  route/stop types introduced with the content adapter; `A4` typed reasons introduced; `A5`
  fixed clock done and frame-partition independent (tested); `A9` runner + trace schema +
  `Capture` + automatic capture wired + S1/S2/S3 done.
- `Traffic` fields are private behind query/command accessors; caller groups migrated
  (C5a–C5d, C6); the LAN mirror applies host state via a command.
- Runtime selector added at session start (`OMSI_TRAFFIC_RUNTIME`, only `current`).
- The fixed 20 ms clock now runs in the window and offscreen paths (`traffic::scenario::SIM_DT`
  / `MAX_SIM_STEPS`, `advance_fixed_clock`), and `s1_replay_partitions.rs` checks that
  15/30/60/144 FPS partitions decision-for-decision agree.
- Contract adoption done: `AiCar.id` and the id-typed traffic state are `VehicleId`;
  `AiCar.why` is a typed `Reason` (projected to the `OMSI_TRACE_AI` label); `trip_route`
  returns a `RouteStatus` (`Complete`/`PendingTiles`/`Invalid`).
- Remaining (documented in [DEPENDENCIES.md](DEPENDENCIES.md)): `AiCar` asset fields still
  public (presentation store is Stage 3/6); scheduler/passenger/time-reset internal redesign
  (Stages 5–6); the timetable route compiler still lives in `schedule.rs` and is wired to the
  new status rather than moved into `traffic::routing`.
- `A6`–`A8` (snapshot, single pose owner, service machine) remain Stage 3/6 targets; their
  contracts are seeded by `diagnostics.rs` and `service.rs`. Full berth arbitration for S3
  is Stage 6.

## Stage 2 progress

- `D1` `[crossingproblem]` is carried into `Lane::crossing_problem`; its decision semantics
  stay unestablished and are reported by `Network::validate` (`UnresolvedCrossingProblem`).
- `D2` `[blockpath]` is a typed `BlockRule { path, mode }`; both values reach the network,
  the mode is kept as data and a nonzero mode is reported (`UnresolvedBlockMode`).
- Conflict compilation is height-aware (`MEET_CLEARANCE`): a bridge no longer conflicts with
  the road below only because their plan views cross.
- `Network::validate` (`traffic::validation`) reports empty/zero-length lanes, bad widths and
  speeds, duplicate keys, ambiguous joins, and unresolved content flags. Wired to
  `OMSI_DEBUG_NETWORK` in `core`.
- `Network::version` is bumped whenever lanes or links change.
- `[ai_brakeperformance]` keeps all five values with the vehicle as provenance; only element
  4 (stop shift) is consumed so far.
- Stop targets compile through `traffic::service::compile_stop_target`, which validates the
  serviceable platform side, the docking position, and keeps the route occurrence separate
  from the geometry; malformed berths return `StopTargetError`. `StopTarget` has replaced
  `bus_service::Stop` at the boundary, carrying the route index and typed platform side.
- The route compiler now lives in `traffic::routing` (`compile_route`, `RouteStepState`,
  `RouteCompilation`, `joins`, `bridge_gaps`, `way_between`); `schedule` builds the map keys
  and a tile-state closure and no longer owns the direction selection, detour skipping, or
  gap bridging. `trip_route` and `trip_route_in` both go through it.

### Stage 2 remaining

- Spline speed limits now honour `[rule] kill`, but the remaining normalization/provenance
  for traffic-light associations and vehicle restrictions is still reported rather than
  enforced. This is a Stage 5 concern.
- The `Traffic` orchestrator in `core/src/traffic.rs` is still large: its behaviour layers
  (world/perception/junctions/maneuvers/service/population/presentation) are the Stages 3–7
  extraction, not Stage 2.

## Stage 3 progress

- `A4`/`A6` shared perception and snapshot substrate: `traffic::perception` adds
  `BodyFootprint` (owner id, part index, realized centre/axes/half extents, height range
  `z0..z1`, speed/acceleration, and lane placements for now/previous/crossing/passing),
  `LaneInterval`, and `Occupancy`. Both the lane-interval index and a spatial grid are built
  once per tick; intervals are sorted by position so decisions do not depend on container
  order. Trailers and articulated rear sections share the towing `VehicleId` and differ by
  `part`.
- Route-relative observations use one convention (metres from the observer origin to the
  blocker's rear along the planned way): `nearest_ahead`, `crossing_approach`,
  `berth_occupancy`, `swept_clearance`, `pedestrian_clearance`, and `project_on_route_local`
  (a projection off to the side or facing another way is rejected rather than snapped onto a
  nearby parallel road).
- `traffic::world` adds the immutable per-tick `Snapshot` (occupancy + previous `Commit`),
  `Commit` (blocker/claims keyed by `VehicleId`), and `Arbiter` (deterministic claims,
  simultaneous-merge winner by stable id, and exit storage reserved for all admitted
  vehicles out of one shared free distance).
- `core::Traffic` builds the occupancy and `by_lane` view from it; `body_in_way` sweeps
  through `Occupancy::swept_clearance`; junction reservations and exit storage go through
  `Arbiter`; `geo_prev`, the merge tie-break, and `break_lead_pairs` are id-keyed. LAN
  remotes' trailers are fed to perception with their owner id.
- Exit gate covered by headless tests (no renderer, no OMSI assets):
  `tests/s3_reorder_invariance.rs` (container order and scheduling partitions),
  `tests/s3_bus_rear_junction.rs` (an articulated bus rear blocks the junction until clear),
  `tests/s3_external_trailers.rs` (player/LAN trailers and bridge height separation), and
  `tests/s3_arbitration.rs` (exit storage and simultaneous merges). The perception and world
  modules also carry their own unit tests.

### Stage 3 replacement reason trail

Each previous-frame/ad-hoc check removed here was replaced only because an equivalent
scenario passes:

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| `by_lane: HashMap<lane, Vec<(index, s, lat, foreign)>>` built in `tick` | `Occupancy` lane intervals + `Occupancy::lane_view` | Same data, but intervals are position-sorted and keyed from stable ids; reordering the cars cannot change a query. |
| `geo_prev: Vec<Option<VehicleId>>` | `Commit::blocker_of` / `HashMap<VehicleId, Option<VehicleId>>` | Previous-frame mutual-wait memory is now addressed by id, so a container reorder keeps the same pairing. |
| `reservations: HashMap<lane, Vec<index>>` | `Arbiter` claims keyed by `VehicleId` | Same claim semantics, deterministic by id; claims sort by id. |
| Exit-full test per vehicle against the empty exit | `Arbiter::reserve_storage` | Free exit storage is shared, so two admitted vehicles cannot each be promised the same space. |
| `body_in_way` core geometry over `Footprint` | `Occupancy::swept_clearance` | Same corridor sweep, now height-aware and covering external/trailer parts; ids not indices. |
| Merge tie-break `j < i` in `obstacle_ahead` | `other.id < self.cars[i].id` | The tie now follows stable ids instead of storage order. |
| `break_lead_pairs` pair de-dup by index (`b <= a`) | Pair chosen by id (`c.id < bid`) | Same "further along/lower id goes" outcome without container-order dependence. |

### Stage 3 remaining

- Junction admission, priority and `GRIDLOCK_WAIT` recovery are still the Stage 5 target;
  this commit only routes reservations and exit storage through the arbiter.
- The service state machine and full berth arbitration are Stage 6; `berth_occupancy` is
  available but not yet the owner of stop phase.
- Motion realization still advances controller progress rather than reconciling it from body
  feedback (`project_on_route_local` is provided and tested but not yet the pose owner);
  single-pose ownership is Stage 4.

## Stage 4 progress

- `A7` single physical-pose owner is done. `traffic::following` adds `RealizedMotion` and
  `AiState::commit_feedback`, which projects the realized body onto the planned route with
  `perception::project_on_route_indices` (the allocation-free form of
  `project_on_route_local`) and adopts the projected lane, distance and realized speed.
  `core::Traffic::tick` reads every road vehicle's realized pose and speed back after the
  body step, so `state.s` - and every stop distance derived from it - is committed from
  realized movement, never a planner coordinate alone. `simulation::ai_motion::AiBody`
  exposes its realized travel (`realized_speed`).
- Reusable longitudinal controller and calibrated envelopes (`B1`, `B2`): `BehaviorEnvelope`
  names the comfort acceleration/service braking/jerk, the emergency ceiling and the default
  headway/gap/reaction with units; `LongitudinalDemand { comfort, emergency, reason }`
  separates the comfort command from collision prevention. The comfort channel is
  jerk-limited and collision prevention is not held back by it. A lower limit ahead is met
  with a feasibility correction rather than approached asymptotically.
- `D3` `[ai_brakeperformance]`: `BrakingCapability` consumes the whole array. Element 4 stays
  the verified stop-holding correction, all five raw values are preserved, and the braking
  strength is an explicit provisional class fallback (`BrakeSource::ClassFallback`) because
  the other values' meanings stay unresolved. `core` gives `-1` timetable buses their real
  physical class so their fallback is right.
- Launch traits (`B3`): only *entering* a hold sets the launch timer; a re-hold after a
  flickering constraint keeps the count instead of resetting it, so a stop/go junction can no
  longer hold a queue from moving off.
- Diagnostics: `VehicleSnapshot` carries commanded and realized speed, applied acceleration,
  and the `emergency`/`reconciled` flags; `TRACE_VERSION` is 2.
- Exit gate covered by headless tests (no renderer, no OMSI assets):
  `tests/s4_leader_brake.rs`, `s4_launch_waves.rs`, `s4_frame_rate_motion.rs`,
  `s4_stop_anticipation.rs`, plus unit tests in `following`, `capabilities` and `perception`.

### Stage 4 replacement reason trail

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| Controller advanced `state.s` while the body tracked `state.way_point` (two integrators) | `AiState::commit_feedback` projects the realized body and adopts its lane/distance/speed | One pose owner; progress cannot drift from the body, and a rejected projection (parallel road, wrong heading) leaves the planner untouched rather than teleporting it. |
| A single `out.clamp(-MAX_BRAKE, a)` braking channel | `LongitudinalDemand` comfort vs emergency | Ordinary driving is bounded by the comfort envelope; hard braking is only the explicit emergency channel. |
| A lower limit ahead only softened `v0` (asymptotic IDM) | Speed-proportional feasibility term | The car actually meets a new limit at the lane joint instead of entering it several m/s high. |
| `start_timer = (start_timer + 3*dt).min(reaction)` on every hold | Only entering a hold sets `reaction`; a re-hold keeps the count | A constraint that flickers can no longer reset the launch timer; a queue still launches on its own reaction. |
| `[ai_brakeperformance]` element 4 only | `BrakingCapability` keeps all five values and names the strength's provenance | The stop correction is unchanged, the raw values survive for calibration, and the provisional class fallback is explicit instead of an invented meaning. |

### Stage 4 remaining

- Junction admission/`GRIDLOCK_WAIT` (Stage 5) and berth/service ownership (Stage 6) are
  unchanged.
- `commit_feedback` is best-effort while a lane change puts the body outside the projection
  envelope, and `ai_motion`'s small `along` catch-up term remains until lateral maneuvers
  move into the domain (Stage 7).
- The braking strength stays a provisional class fallback until the remaining
  `[ai_brakeperformance]` values are established (kept as data; see `D3`).

## Stage 5 progress

- `traffic::junctions` (L4) is the single owner of junction admission, commitments,
  fairness, release and the wait-for graph. `JunctionCoordinator` plans from one frozen
  `JunctionScene` and returns a `JunctionDecision` (signal hold, right-of-way hold,
  `JunctionState`, reasons). `Movement`/`junction_ahead` are the explicit conflict areas
  (grouped by crossing object); `BlockMode`/`block_mode_between` type the content rules.
- Core is an adapter: `Traffic::tick` builds the actor view and signal aspects once, calls
  `JunctionCoordinator::begin_tick`/`plan` per vehicle, and applies the decision. Junction
  claims/storage are owned by the coordinator; `AiCar::reserved`/`amber` are gone.
- `D7` is done: `LONG_WAIT_CLAIM` and `GRIDLOCK_WAIT` and both timer escapes are removed.
  Waiting duration never grants entry; a clock timeout cannot erase a body.
- `D1`/`D2` established: `[crossingproblem]` is a keep-clear path (entry refusal), and
  `[blockpath]` is a typed `BlockMode { Occupy, Reserve, Oncoming }` (reservation refusal and
  oncoming commitment). Provenance: the reference parser proves only that the boolean flag
  and the two values are stored; the decision meanings are the documented Stage 5 neoOMSI
  interpretation, tested with reservation-vs-entry-vs-oncoming scenarios.
- Admission waits for a legal movement, clear conflicting bodies/commitments, and full
  downstream storage; storage is reserved across simultaneous admissions through `Arbiter`.
  Once `Inside`, the entry light is ignored and the vehicle clears safely.
- Release conditions are explicit: tail clearance, route change (`retain_on_way`), removal
  (`release`), and network invalidation (`invalidate_network`, called from `add_tiles`,
  `reset_population`, `remove_car`).
- Signal entry stays feasibility-based: red/red-yellow stop, amber stops when it can and is
  remembered when it cannot, green/green-yellow/dark/inactive and request phases covered;
  scripted lamp feedback stays in `TrafficLightController::lamps`.
- The wait-for graph classifies persistent holds: a cyclic set of speculative claims is a
  stale-claim deadlock (`CancelStaleClaim` + `RetrySafeManeuver`), a cycle held by a red
  signal is legal congestion (`WaitLegal`), and a cycle with no claims behind it is
  `FullCapacity`. Recovery never crosses a conflicting body or a red signal.
- Deterministic priority (emergency `TrafficPriority` → path `priority` → arrival/wait →
  lower id) and bounded fairness (the longest legal waiter proceeds when no body/priority
  forbids it); emergency priority cannot authorize a collision or an impossible exit.
- Diagnostics: `VehicleSnapshot` gained `junction_state` and `junction_blocker`;
  `TRACE_VERSION` is 3 and both are in the rolling decision/event hash.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`
  (`cargo tree -p traffic`).
- Exit gate covered by headless tests (no renderer, no OMSI assets) under
  `crates/traffic/tests/`: `s5_four_way.rs`, `s5_priority_turns.rs`,
  `s5_blocked_exit_recovery.rs`, `s5_wait_for_graph.rs`, `s5_crossing_blocks.rs`,
  `s5_crossings.rs`, plus unit tests in `junctions` (block modes, stale/legal/capacity
  cycles, invalidation and on-way release).

### Stage 5 replacement reason trail

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| `LONG_WAIT_CLAIM` kept a claim while blocked after 45 s | Removed; claims are released on block and the wait-for graph detects a real stale cycle | Waiting longer cannot create road space; a legal hold is diagnosed, not forced. |
| `GRIDLOCK_WAIT` squeezed into a full exit after 45 s | Removed; the full exit keeps the vehicle out until storage is free | A timer can no longer authorize entry the capacity does not allow. |
| `Traffic::junction_stop` inline decision + `AiCar::reserved`/`amber` | `JunctionCoordinator::plan` with commitments owned by the coordinator | One writer for junction state; previous-frame claims are keyed by stable id. |
| `[blockpath]` flattened to a symmetric whole-path conflict | `BlockMode` from the stored mode, honored before geometry | Reservation refusal (`Reserve`) and oncoming commitment (`Oncoming`) are distinct from body occupancy (`Occupy`). |
| Silent gridlock escape with a debug log | `classify_waits` categories and typed `Recovery` | A cyclic stale claim is cancelled and retried; legal congestion and full capacity wait. |

### Stage 5 remaining (documented, not claimed done)

- The service/berth ownership and the full `ServicePhase` machine are Stage 6; the
  coordinator already reserves exit storage but does not assign berths.
- Lateral maneuvers (lane changes, passing, parking) are Stage 7; the coordinator does not
  own lateral intent.
- Population/streaming backpressure and dormant lifecycle are Stage 8; `invalidate_network`
  only releases junction claims.
- `plan` still reads caller-supplied `on_lane`/`coming` index views; moving them fully onto
  the perception `Occupancy` (id-keyed) is a later cleanup.

## Stage 6 progress

- `traffic::service` (L3/L4) is the single owner of the scheduled service state machine and
  berth capacity, mirroring `traffic::junctions`.
  - `BerthGeometry` keeps the stop's own length, the boarding region, the approach distance
    and the vehicle stop correction separately named (`D6`); the stop length is provisional
    until a content length is imported, and one berth per stop is the validated default.
  - `ServiceState` (the writer is `ServiceCoordinator::plan`) holds the explicit `ServicePhase`
    and its timers. `ServiceScene`/`ServiceActor` are the frozen per-tick inputs, built from
    the realized `Occupancy` (id-keyed), so the berth scene uses the same substrate without
    migrating `JunctionScene`'s index views.
  - Berths are granted in stable arrival order (the tick a bus first comes within `STOP_REACH`,
    then stable id), held through `Docking`/`Boarding`/`ClosingDoors`/`WaitingToMerge`, and
    released only when the rear clears the berth point, the route changes, the vehicle is
    removed, or the network is invalidated.
- The transition table is explicit: `EnRoute -> Approach -> WaitingForBerth -> Docking ->
  Boarding -> ClosingDoors -> WaitingToMerge -> Departing -> EnRoute`; trip completion ->
  `NextTrip`/`OutOfService`; any applicable state -> `RoutePending` or `ServiceFault(reason)`.
  A free curb stop passes through `WaitingForBerth` immediately. Boarding is permitted only
  with low speed, longitudinal error <= 0.5 m, lateral error <= 0.25 m, a valid berth and the
  permitted door side; `AiCar::boarding_permission()` is shared with passenger registration so
  the bus and the people cannot disagree.
- `D8` is done: `BusService::approach/arrive/step` and `Ctx` are gone; the queued/crept-past
  arrival shortcut and the `CLOSE_MAX` self-departure are replaced by the berth/boarding
  geometry and the typed handshake. An overshoot records a missed/faulted stop
  (`Reason::MissedStop`) instead of opening the doors up the queue.
- `D5` is done: `ScriptFeedback` separates acknowledged, unsupported (validated fixed-close
  fallback), stuck-with-unknown-doors (timeout -> `Fault(StationRelease)`) and
  stuck-with-open-doors (never departed). `AI_Scheduled_AtStation`/`_Side` keep the existing
  script contract; the `at_station` projection now also sends -1 while closing.
- `ROUTE_WAIT_MAX` and its escape are removed: an open route that reaches the loaded frontier
  becomes `RoutePending` and keeps its remaining stops (a diagnosed content/service
  limitation), rather than being cleared into random traffic.
- Core is an adapter: `Traffic::tick` builds the `ServiceActor` array and berth intents once,
  calls `ServiceCoordinator::begin_tick` once and `plan` per bus, and applies the
  `ServiceDecision` (stop distance, lateral target, blinker, door side, stop cursor). Berth
  release runs on removal (`remove_car`, the tick removal loop), route change (`reroute`,
  `extend_scheduled_route`), population reset and network invalidation (`invalidate_network`).
- Diagnostics: `VehicleSnapshot` gained `service_phase`, `berth_owner` and `service_stop`;
  `TRACE_VERSION` is 4 and all three feed the rolling decision/event hash.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`
  (`cargo tree -p traffic`).
- Exit gate covered by headless tests (no renderer, no OMSI assets) under
  `crates/traffic/tests/`: `s6_shared_stop.rs`, `s6_berth_recovery.rs`, `s6_optional_stops.rs`,
  `s6_script_handshake.rs`, `s6_duty_lifecycle.rs`, plus the `common::service` kinematic
  fixture and unit tests in `service`.

### Stage 6 replacement reason trail

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| `BusService::Phase` (5 variants) and `step`/`arrive`/`approach` | `traffic::service::ServiceCoordinator` + `ServiceState` + `ServiceDecision` | One writer of the service transitions; the explicit phase table replaces the ad-hoc `Running/Boarding/Waiting/Closing/TripDone` shortcuts. |
| `queued` serve after 6 s stopped within 45 m, and `crept_past` serve when 12 m past the stop | Berth occupancy + boarding-region check; a real overshoot becomes `ServiceFault(MissedStop)` | A queue can no longer make a stop "arrive"; doors open only at a valid berth, and a missed stop is recorded. |
| Constant `BAY_REACH = 30` used as both pull-in and docking reach | `BerthGeometry` with named `stop_length` (provisional), `boarding_region`, `approach_distance`, `stop_correction` | Each length is measured on its own; the misleading `0x620058`/`BAY_REACH` pairing is dropped. |
| `CLOSE_MAX` self-departure in `Phase::Closing` | Typed `ScriptFeedback`: unsupported -> fixed close; stuck -> timeout `Fault`; open doors -> never depart | A timer can no longer drive a bus off with its doors provably open; a timeout is reported. |
| `Phase::TripDone` set inline, and the `ROUTE_WAIT_MAX` random-traffic escape | `NextTrip`/`OutOfService` lifecycle plus `RoutePending` | A scheduled bus is never silently emptied into random traffic; missing tiles/capacity are diagnosed. |
| Stop distance `d + front - 0.3` with the origin rest position already carrying the stop correction | `d + front + STOP_LINE_GAP` so the origin rests at the berth point, and boarding requires a 0.5 m longitudinal error | The documented 0.5 m docking target is meaningful rather than absorbed by a standoff. |

### Stage 6 remaining (documented, not claimed done)

- Lateral maneuvers (docking S-curve, lane changes, passing, parking) are Stage 7; the service
  owner sets a lateral target but does not own the trajectory. `approach_distance`/
  `junction_first` approximate the old bay pull-in.
- Multi-berth stops need validated content geometry; `BerthGeometry` carries a `berths` count
  but only one is exercised.
- Population/streaming backpressure and dormant lifecycle are Stage 8; `RoutePending` waits
  for tiles, and removal/notification is still the timetable's job.
- `JunctionScene`'s `on_lane`/`coming` index views are still caller-built; the berth scene
  reads the id-keyed `Occupancy` directly, and migrating the junction views is a later cleanup.

## Stage 7 progress

- `traffic::maneuvers` (L4) is the single owner of every lateral maneuver, mirroring
  `junctions` and `service`: route-required lane changes, discretionary changes, overtaking,
  curb avoidance/bypass, passing, parking arrival, pull-out and the lateral half of service
  docking. Conflicting behaviors submit `ManeuverInputs` instead of overwriting
  `lateral_target`; `ManeuverDecision` is the typed output and core is the adapter.
- `ManeuverCoordinator::begin_tick` orders simultaneous lane changes by target lane and stable
  id, so `B4`'s "no lane/go-wait oscillation under unchanged conditions" is decided by the
  single owner instead of container order; `required_target` is the shared route/turn-lane
  wish. `DISCRETIONARY_DWELL`, `CHANGE_COOLDOWN` and `OSCILLATION_WINDOW` commit with
  hysteresis and de-oscillate.
- Required maneuvers (route change, docking/departure, a committed park) precede discretionary
  (overtake/keep-right), which precede optional passing. A required change that is impossible
  waits legally before the lane end (`commit_or_wait`, a `Yield` stop) rather than cutting the
  queue or jumping lanes.
- Passing (`B5`) is optional and only starts with the whole outbound+return trajectory checked:
  the oncoming gap over the whole maneuver, the return room, an `Occupancy::swept_clearance` of
  the ghost path including the return (which carries the swept body/trailer width), and
  pedestrians. An abort returns along its S-curve while abortable and otherwise holds the
  committed portion, never snapping laterally.
- `D4` (`ev_AI_Horn`) is deferred to Stage 9 and recorded here: the reference proves the event
  exists but not its trigger, and it is presentation feedback that must not become a
  deadlock-resolution mechanism.
- `A9` trace schema: `VehicleSnapshot` gained `maneuver_phase`/`maneuver_target`;
  `TRACE_VERSION` is 5 (see [TRACE_SCHEMA.md](TRACE_SCHEMA.md)).
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `tests/s7_route_turn_lane.rs`, `s7_simultaneous_lane_change.rs`,
  `s7_blocked_bay.rs`, `s7_parking_pullout.rs`, `s7_oncoming_abort.rs`,
  `s7_articulated_clearance.rs`, `s7_no_oscillation.rs`, the `common::maneuver` fixture and
  unit tests in `maneuvers`.

### Stage 7 replacement reason trail

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| `Passing`/`ParkPlan` state and the `plan_pass`/`guard_pass`/`oncoming_block`/`light_wait` functions in core | `traffic::maneuvers::{Passing, ParkPlan, ManeuverState, ManeuverCoordinator}` | One writer of the lateral decision and its commitments; core submits requests and applies one typed `ManeuverDecision`. |
| `plan_lane_change`/`plan_bypass`/`plan_route_change` each calling `start_change`/`start_bypass`/`start_route_change` | `required_target` intents + `ManeuverCoordinator::begin_tick` + `commit_or_wait` | Simultaneous changes are ordered by target lane and stable id, so container order cannot change the outcome. |
| Inline `lateral_target` writes for passing, parking, kerb swerve and service docking | `ManeuverDecision` applied by core | A single owner; a maneuver cannot overwrite another function's lateral intent. |
| Passing feasibility by `car.body.sweep_clearance` over the outbound path only | L2 `Occupancy::swept_clearance` over the outbound **and** return ghost path | The whole maneuver (return included, trailer/part bodies and pedestrians) is checked in the same frozen snapshot; the physical `AiBody` sweep stays for realization. |
| `plan_lane_change`'s immediate overtake/keep-right | `DISCRETIONARY_DWELL` + `OSCILLATION_WINDOW` + `change_cooldown` in the coordinator | A flickering local condition cannot make a car jerk between lanes; `B4` oscillation is bounded by construction. |
| `const CREEP_PAST` and the `accel_cap` derived in core each tick | `ManeuverDecision::accel_cap` from the passing plan | The edge-out accel cap belongs to the maneuver that needs it, and it is reset when no maneuver wants it. |

## Stage 8 progress

- `traffic::population` (L3/L4) is the single owner of demand, eligibility/admission, the
  dormant lifecycle and topology demand, mirroring `junctions`, `service` and `maneuvers`.
  It keeps four things the legacy pass mixed together distinct: **demand**, **admission**
  (valid path, feasible continuation, loaded ground, physical gap, presentation visibility),
  **physical occupancy** (read from the frozen `Occupancy`, never written) and **presentation
  visibility**. A population target is not an order to fill every free-looking lane.
- `PopulationCoordinator` owns the bounded request queue, per-entrance backpressure, retry
  timers, the dormant registry (identity/class/duty/progress) and the bounded topology demand.
  `SpawnDecision` (`Admit`/`Deny(reason)`/`Retry`) and `DormantDecision` are the typed outputs;
  `SpawnAdmitted`/`SpawnDenied`/`SpawnRetried`/`DormantEntered`/`DormantReactivated`/
  `TopologyRequested` are the events. `begin_tick`/`plan`/`plan_dormant`/`release`/
  `retain_on_way`/`invalidate_network` mirror the other owners' hooks.
- `A10` is done: admission is a deterministic bounded queue (`QUEUE_MAX`, `ADMIT_PER_PASS`);
  a busy entrance backs off (`ENTRANCE_BACKOFF`) and retries instead of stacking vehicles, and
  over-budget demand is denied with `AtCapacity` rather than growing the queue. Two placements
  admitted in the same pass cannot overlap (same-pass placement set). Scheduled duty capacity
  and unscheduled demand are distinct budgets (`scheduled_admission` vs the random queue).
- Dormant lifecycle: identity, class and duty ownership are kept by the coordinator while a
  vehicle is out of the active area; the kinematic step and assets stay in the adapter.
  Reactivation validates ground, visibility and gap before any body is rebuilt, so dormant
  motion cannot create an overlap; `release` is once-only for a known dormant actor.
- Topology vs active ground: the loaded network is separate from `has_ground`/collision
  availability. A scheduled bus on an unloaded route tile asks for that tile
  (`request_topology_tile`); core appends the wanted frontier centres to `Streamer::update`
  (the streaming owner is unchanged) and validates re-entry before physical placement.
- Recovery by cause: stale claims (junctions), pending content (schedule `waiting`/`retry_at`),
  a legally rerouted random car, an explicitly faulted scheduled route (`RoutePending` /
  `InvalidRoute`), and suspend/remove through one documented transition. Removal releases
  junction/service/maneuver/population state and renders exactly once; the streaming removal
  branch that used to skip `junctions`/`services`/`maneuvers` release now releases them.
- `B7` is updated: congestion is honest — over-capacity demand is diagnosed, not bypassed; a
  full scheduled budget makes the duty retry (`Placed::Busy`) instead of silently dropping it.
- LAN: the host remains the single authority; a mirror client never requests or admits
  (it only presents replicated committed state), and an authority change or time reset keeps
  duty ownership without duplicating it. No replication schema change.
- `A9` trace schema: `VehicleSnapshot` gained `lifecycle`; `TRACE_VERSION` is 6.
- `traffic` still depends only on `glam`, `hashbrown` (+leaves) and `log`.
- Exit-gate scenarios: `tests/s8_overload_backpressure.rs`, `s8_streaming_identity.rs`,
  `s8_dormant_reactivation.rs`, `s8_lan_authority.rs`, the `common::population` fixture and
  unit tests in `population`.

### Stage 8 replacement reason trail

| Replaced check | Replacement | Why it is equivalent or better |
| --- | --- | --- |
| `populate_kind`'s inline `create_car` loop with per-attempt `may_appear`/ground/14 m checks | `traffic::population::PopulationCoordinator::plan` over a bounded request queue | One owner of admission and its budget; a blocked entrance retries instead of consuming the attempt budget, and denials are typed. |
| `wake_dormant`'s inline `has_ground`/`may_appear`/14 m/`spawn_clear` gate | `PopulationCoordinator::plan_dormant` (identity kept) + adapter `spawn_clear` | Identity, duty and once-only removal live in the domain; the generic gap is checked in the same frozen snapshot, the asset-specific clearance stays with the adapter. |
| `spawn_bus`'s silent `None` on `[AIMaxCountScheduled]` | Typed `Result<usize, Reason>` + `scheduled_admission`; schedule maps `AtCapacity`/`EntranceBusy` to `Placed::Busy` | A full scheduled budget is a diagnosed wait with a retry, not a duty that quietly disappears. |
| The streaming removal branch skipping `junctions`/`services`/`maneuvers` release | `population.release` plus the existing coordinator releases on every removal path | Resources and duty notifications are released exactly once, including a car removed by tile unload. |
| `fill_map`'s inline `MAP_POPULATION_FACTOR` cap with unregistered dormant cars | `PopulationCoordinator::dormant_has_room` + `enter_dormant` | The whole-map dormant population is bounded by the owner and its identity/duty are registered for reactivation. |
| No tile demand ahead of a route frontier | `request_topology_tile` + `topology_centers` fed to the streamer | A bus reaches loaded ground where feasible instead of stopping at the loaded edge. |

