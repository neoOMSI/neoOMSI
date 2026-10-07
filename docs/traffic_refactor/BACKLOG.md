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
