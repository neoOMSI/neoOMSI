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
| `D1` | Retain the per-path `[crossingproblem]` flag through network compilation and establish its decision semantics | [functions/007b432c.md](H:/marcel_omsi/functions/007b432c.md), [assembly/007b432c.asm](H:/marcel_omsi/assembly/007b432c.asm) `007b788a`/`007b78c6` | 2, 5 | gap |
| `D2` | Retain both `[blockpath]` values as a typed block rule with mode; determine directional/admission/occupancy meaning | [functions/007b432c.md](H:/marcel_omsi/functions/007b432c.md), `007b78d3`/`007b79cb`/`007b7a4a` | 2, 3, 5 | gap |
| `D3` | Audit and import all five `[ai_brakeperformance]` values; only element 4 is consumed today | [`crates/vehicle/src/vehicle/parse.rs`](../../crates/vehicle/src/vehicle/parse.rs), [`bus_service::stop_shift`](../../crates/core/src/bus_service.rs) | 2, 4, 6 | partial |
| `D4` | Add the `ev_AI_Horn` behaviour event through the script adapter with cooldown and diagnostics | `007db679`/`007db683`; [Traffic AI guide](H:/marcel_omsi/subsystems/traffic_ai.md) | 7, 9 | gap |
| `D5` | Honour script-facing station state (`AI_Scheduled_AtStation`) and separate safe fallback from unsafe doors | [functions/007eab20.md](H:/marcel_omsi/functions/007eab20.md), `007eb4bc` | 6 | present, unsafe fallback |
| `D6` | Re-establish stop length, boarding region, docking reach (`BAY_REACH`) and lateral placement as separate concepts | `00620004.asm` shows `00620058` is list traversal, not metadata; [`bus_service`](../../crates/core/src/bus_service.rs) | 2, 6 | uncertain |
| `D7` | Remove the unconditional full-exit override after `GRIDLOCK_WAIT`; models admission, occupancy, recovery separately | [`Traffic::junction_stop`](../../crates/core/src/traffic.rs) | 5 | present |
| `D8` | Replace the queued/crept-past arrival shortcut and fixed early-wait caps with explicit berth/door geometry and service policy | [`BusService::approach/arrive/step`](../../crates/core/src/bus_service.rs) | 6 | present |

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
