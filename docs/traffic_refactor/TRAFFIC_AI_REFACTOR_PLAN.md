# Traffic AI refactor plan

Status: proposed implementation roadmap; no runtime changes in this document.
Prepared: 2026-10-07. Source inspection baseline: `35a460a`.
Revised: Stage 0 is lightweight preparation, not a requirement to reproduce the reported screenshot. Targeted reference/source audit added on 2026-10-07.

## 1. Outcome and scope

The primary goal is a maintainable traffic AI that behaves **more correctly and more naturally than OMSI 2**, not merely a cleaner reproduction of OMSI's traffic. Replace the current traffic orchestration with a system that is understandable to a human developer, independently testable, and predictable under load. Cars should follow street rules and react smoothly to other road users. Scheduled buses should reach a valid boarding position, serve the correct stop, and depart safely without creating artificial lines of buses.

Use OMSI 2.2.032 as the content and script compatibility baseline. Preserve supported map paths, traffic rules, signal programs, vehicle configuration, AI lists, timetable duties, HOF/IBIS behavior, and passenger interfaces. Improve traffic decisions where reproducing an OMSI bug would conflict with the requested behavior: safe junction admission, valid bus docking, reliable queues, and natural motion. These are intentional neoOMSI traffic improvements and must be documented as such during implementation. This request establishes that direction; it does not require reproducing every OMSI AI bug or adding a second permanent behavior engine.

The priorities are: **correct and safe decisions, natural observable behavior, and clear maintainable ownership**, while preserving existing content/script contracts. Matching OMSI's trajectory or a problematic decision is not an acceptance criterion. If a verified OMSI behavior is jerky, needlessly hesitant, unsafe, or causes artificial blocking, specify the improved behavior and test it. Compatibility evidence explains legacy inputs and interfaces; it does not veto the explicitly requested improvements.

### What "more natural than OMSI 2" means

- **Anticipation:** recognize queues, bends, speed reductions, stop approaches, and likely merge needs early enough to brake and position smoothly, instead of reacting at the last path sample.
- **Continuous control:** limit ordinary jerk, steering rate, and lateral acceleration; avoid abrupt heading changes, sideways gliding, repeated brake/accelerate flicker, and unexplained crawling toward a reachable stop. Reserve emergency braking for actual emergencies.
- **Credible variation:** stable differences in headway, reaction, preferred speed, and comfort produce varied drivers and queue launch waves without per-frame random wobble or identical synchronized movements. Variation stays inside legal and physical limits.
- **Consistent intentions:** indicate before a maneuver, commit when safe, and complete it smoothly; avoid oscillating between lanes or between "go" and "wait" under effectively unchanged conditions.
- **Social interaction:** choose feasible gaps, leave useful space, and cooperate at merges where legal. Vehicles do not wait forever for phantom threats or invent priority solely because they are impatient.
- **Convincing bus service:** approach and dock deliberately, align doors with the platform, dwell for passengers and service needs, close safely, and merge back smoothly. Queued buses remain queued until they can actually serve the stop.
- **Honest congestion:** queue where demand or a real obstruction requires it, then recover naturally when space opens. Avoid forced movement, sudden disappearance, overlapping spawn, or teleportation as a substitute for a driving decision.

These qualities are mandatory throughout the behavior stages and final rollout, not optional polish after parity. Use quantitative motion/progress/service measurements together with recorded representative sessions: numerical safety alone cannot demonstrate convincing driving, and visual smoothness alone cannot demonstrate correctness. Comparisons with OMSI should show concrete improvements in relevant scenes rather than an unsupported claim that every possible scene is better.

Success does **not** mean removing all queues. Red lights, occupied bus stops, player obstructions, and demand above road capacity legitimately create queues. Success means that queues have a valid cause, remain physically safe, and discharge when the cause clears. Actual service bunching caused by late buses is different from duplicate duties, false stop arrivals, and indefinitely stuck vehicles.

Road traffic is the refactor's main scope. Rail, aircraft, parking, articulated vehicles, passenger crossings, streamed maps, save/time-reset behavior, and LAN traffic are compatibility boundaries that must remain supported. A new pedestrian planner, a wholesale vehicle physics rewrite, and traffic demand optimization for an entire city are separate projects.

## 2. What the inspection establishes

These are source observations, not a runtime diagnosis of the attached screenshot. The screenshot shows a long queue, but does not establish its map, lead vehicle, blocking reason, or reproduction steps.

| Current location | Observation | Refactor consequence |
| --- | --- | --- |
| [`core/src/traffic.rs`](../crates/core/src/traffic.rs), about 8,400 lines | `Traffic` owns the network, vehicles, population, junction decisions, passing, parking, lights, scripts, presentation, audio, and LAN mirroring. `AiCar` contains both behavior and render resources. | Separate the traffic domain from engine integration; give each decision one owner. |
| [`simulation/src/traffic.rs`](../crates/simulation/src/traffic.rs), about 3,900 lines | Geometry, topology, rules, signals, routing, maneuver state, following, and integration coexist. There are useful regression tests already. | Extract by responsibility and retain tested algorithms until evidence justifies replacing them. |
| [`core/src/schedule.rs`](../crates/core/src/schedule.rs), about 5,900 lines | Duty allocation and spawning also resolve routes, project stops, add connectors/reversed paths, and extend running trips after streaming. | Separate service intent from route compilation and physical placement. |
| [`core/src/bus_service.rs`](../crates/core/src/bus_service.rs), `BusService::approach` | A stopped bus can call `arrive` after more than six seconds of queuing while its stop is between 2 and 45 metres ahead. | Replace elapsed-wait arrival with verified berth/boarding geometry. |
| Same file, `BusService::step` | `Closing` can depart after `CLOSE_MAX` (12 seconds) without the normal script release acknowledgement. | Distinguish door faults from departure permission; a timer alone cannot prove that departure is safe. |
| Same file, `BusService::arrive` | Ordinary early waiting is capped at 40 seconds; layover waiting has a separate cap. | Make timing-point, optional-stop, terminus, and layover policy explicit instead of using queue-related caps as timetable policy. |
| `Traffic::junction_stop` | The full-exit hold can be overridden after `GRIDLOCK_WAIT` (45 seconds); a long wait can also create claims while a vehicle remains blocked. | Model admission, occupancy, downstream capacity, fairness, and recovery separately. |
| `Traffic::tick` | Lane/approach indexes are built, then a sequential loop changes decisions and vehicle state; body/script work runs in Rayon afterward. | Audit which reads see old versus updated state; use a shared snapshot and explicit arbitration/commit phases. This is an order-dependence risk, not proof that every decision is order-dependent. |
| [`app_events/redraw/ai_traffic.rs`](../crates/core/src/app_events/redraw/ai_traffic.rs), [`setup.rs`](../crates/core/src/app_events/redraw/setup.rs) | Traffic is stepped from redraw with frame-dependent `dt`; setup caps it at 0.1 seconds. Population and schedule work also run from redraw timers. | Introduce a simulation clock boundary and fixed ticks; define catch-up and time-jump behavior. |
| `AiState::desired_accel` and `traffic::personality` | Driver desire scales the lane speed limit; random cars can receive desire above 1.0. | Separate lawful speed bounds from driver variation. |
| [`simulation/src/ai_motion.rs`](../crates/simulation/src/ai_motion.rs) | Bicycle steering, rate limits, ground contact, swept-clearance checks, and rail/air motion already exist. | Preserve this work through an adapter; do not replace natural steering with snapping to lane samples. |
| [`traffic_link.rs`](../crates/core/src/traffic_link.rs), [`lan_world.rs`](../crates/core/src/lan_world.rs), passenger stop/boarding modules | Player vehicles, trailers, remote vehicles, passenger requests, and stop geometry interact with traffic. LAN already uses host authority. | Keep these contracts explicit; test integrations before removing the old controller. |

Other candidates to investigate are unsafe or repeated spawns, duplicate duty assignment, stale claims, geometry blockers, route truncation, insufficient overtaking clearance, and stop coordinate mismatches. They remain hypotheses until a trace demonstrates them. Splitting files alone will not fix them.

The architecture and decision layers are being replaced because their current ownership and interactions are unsuitable for continued development. That work can start without finding this particular rare failure. A replacement is expected to improve structure and behavior, but does not automatically eliminate bugs: wrong imported rules or new decision mistakes can survive a rewrite. Use short implementation/test cycles and automatic failure capture throughout the replacement.

### Using the local OMSI reference

The supplied report is useful navigation evidence:

- [Reviewed findings](H:/marcel_omsi/REVIEWED_FINDINGS.md): traffic decisions, shared path assessment, and the distinction between observed literals and inferred behavior.
- [Traffic AI guide](H:/marcel_omsi/subsystems/traffic_ai.md) and [paths/signals guide](H:/marcel_omsi/subsystems/paths_and_signals.md): areas to compare for red-light holds, reservation refusal, oncoming conflicts, lane changes, and station handling.
- [Timetable guide](H:/marcel_omsi/subsystems/timetable.md): navigation for scheduled services.
- [Coverage and limits](H:/marcel_omsi/COVERAGE_AND_LIMITS.md): static extraction does not establish exact thresholds, transitions, or runtime ordering.

The report's old `omsi-app`/`omsi-sim` paths must be mapped to the current `core`/`simulation` crates. Existing source comments citing executable addresses are not proof of parity. Use the supplied static evidence to identify mechanisms, check assumptions, and prepare independently written behavior specifications; do not copy pseudocode or binary structures into implementation. Follow the repository's [compatibility workflow](COMPATIBILITY.md) when claiming verified runtime equivalence, with intended traffic improvements recorded separately. Runtime comparisons can happen when practical during the relevant stage; lack of a recording of the rare screenshot failure does not block architecture work.

### Targeted OMSI mechanism and coverage audit

The original planning pass inspected the reference guides and reviewed findings. This revision additionally inspected selected function dossiers, pseudocode, and disassembly for `00716a4c`, `007d9398`, `007df1f8`, `007d5374`, `007b432c`, `007eab20`, and the enclosing function at the old `00620058` citation, then searched current neoOMSI consumers. This is a targeted static audit, not a complete reconstruction or a runtime parity certification. The reference manifest identifies OMSI 2.2.032 executable SHA-256 `7dab063d1f62e73b3a2c7a6ac1921d7edf5e5db0fbc731481d117eec8de7d759`.

Status meanings: **present** means corresponding current code exists, not that parity is proven; **gap** means source data/behavior is demonstrably missing from the inspected path; **uncertain** means exact legacy semantics still need to be established. Prioritize data-loss and service safety issues over cosmetic details such as horn behavior.

| Mechanism and reference evidence | Current neoOMSI coverage | Required action / stage |
| --- | --- | --- |
| Shared path assessment: vehicle AI calls `00716a4c` at `007d9b08` and `007da373`. Its branches distinguish red-light holds, refusal to reserve, refusal to enter, and oncoming reservation. See [path-assessment dossier](H:/marcel_omsi/functions/00716a4c.md) and [vehicle-AI disassembly](H:/marcel_omsi/assembly/007d9398.asm). | **Present, fragmented:** `light_stop`, `junction_stop`, `reserved`, `passing`, and geometry checks provide counterparts. They are not absent, but their ownership and composition need replacement. | Preserve distinct request/admission/physical occupancy outcomes in the new coordinator. Verify cancellation and tail clearance rather than treating all blocking as one timer. Stages 3 and 5. |
| Leader/parallel-road checks and automatic lane changes have separate diagnostics within the same path-assessment routine; the dossier records vehicle collision, a vehicle on the preceding spline, parallel blockage, and two automatic lane-change cases. | **Present, parity uncertain:** `obstacle_ahead`, body/player checks, lane-change planning, and passing exist. Searches do not establish that their projection, priority, or transition semantics are equivalent. | Test leaders across lane joints, adjacent/merging vehicles, and simultaneous lateral moves. Replace inconsistent checks with snapshot queries. Stages 3 and 7. |
| `[crossingproblem]`: the [object/parser dossier](H:/marcel_omsi/functions/007b432c.md) names it, and [disassembly](H:/marcel_omsi/assembly/007b432c.asm) at `007b788a`/`007b78c6` shows a per-path flag being recognized and stored. Exact decision-side use remains **uncertain**. | **Gap:** [`scenery/src/sco.rs`](../crates/scenery/src/sco.rs) parses both object and per-path flags, but the workspace search finds no traffic consumer; `Lane` has no corresponding field. Generic keep-clear behavior is not proof that this content flag is honored. | Carry the flag and provenance through network compilation. Establish its decision semantics before translating it into admission policy; cover flagged and unflagged paths. Stages 2 and 5. |
| `[blockpath]`: the same original parser recognizes it at `007b78d3`; `007b79cb` and `007b7a4a` store two values in a block record. This confirms that retaining only the path number loses original input information; the second value's full runtime meaning remains **uncertain**. | **Partial / gap:** the scenery parser retains `(path, mode)`, but `scene::object_lanes` maps `(n, _)` to `Lane::blocks: Vec<u16>`. `Network::conflicts_from` turns a non-geometric block into a symmetric full-path conflict. Mode and any mode-dependent distinction are lost. | Retain a typed block rule including mode, and determine directional/admission/occupancy semantics. Test explicit block paths whose centerlines do not intersect; do not assume every mode means symmetric whole-path exclusion. Stages 2, 3, and 5. |
| Stop selection is coupled to timetable position and passenger state: `007d9398` branches compare first/last route entries and traverse human states at `007dab39` through `007dac91`; the [reviewed findings](H:/marcel_omsi/REVIEWED_FINDINGS.md) establish the human activity names. This supports demand-aware service, not arbitrary arrival anywhere in a queue. Exact thresholds are not certified. | **Present, simplified:** `stop_wishes`, `must_serve`, and optional-stop logic already exist. The broad queued-arrival shortcut and fixed early-wait caps are current design choices whose equivalence has not been established. | Keep demand-aware optional stops and explicit timing-point/terminus rules. Replace false arrival with berth/door geometry and calibrate service during Stage 6. |
| Script-facing station state: [binding dossier](H:/marcel_omsi/functions/007eab20.md) and [disassembly](H:/marcel_omsi/assembly/007eab20.asm) at `007eb4bc` bind `AI_Scheduled_AtStation`. That establishes the interface's existence, not every handshake value or timeout rule. | **Present, unsafe fallback candidate:** `VehicleInstance::update_ai_with` supplies station state and service checks release feedback; `BusService::Closing` can nevertheless depart on its own 12-second timeout. | Preserve the existing script interface, test actual close/release behavior with supported vehicle scripts, and separate unsupported feedback from known unsafe doors. Stage 6. |
| Vehicle-specific braking configuration: the reference traffic guide identifies `[ai_brakeperformance]` as a legacy input. Static navigation alone does not establish the meaning of all five values. | **Partial / gap:** [`vehicle/parse.rs`](../crates/vehicle/src/vehicle/parse.rs) reads all five values. The only consumer found is `bus_service::stop_shift`, using element 4. Acceleration/deceleration is otherwise set by defaults/personality and a hardcoded bus deceleration. | Audit and document all values, import validated braking capabilities and stop correction separately, and test vehicles with differing brake configurations. Do not invent meanings for the unused values. Stages 2, 4, and 6. |
| AI horn event: original disassembly at `007db679` loads the `ev_AI_Horn` event name and dispatches through a virtual call at `007db683`. Surrounding guards include a time comparison; exact gameplay trigger/cooldown semantics remain **uncertain**. | **Gap in inspected engine path:** no `ev_AI_Horn` dispatch was found in current `crates`. Existing sound playback does not by itself implement this event. | Add a documented, rate-limited behavior event after verifying its trigger and script interface. Horns are presentation feedback and never a deadlock resolution mechanism. Stages 7 and 9; lower priority. |
| Signal request/stop/jump programs and emergency interaction are documented reference areas. | **Present:** `TrafficLightController` implements requests, stops/jumps, and state mapping; `TrafficPriority` and `TrafficPriorityWarningNeeded` are already read/written. Exact behavior is still subject to scenario comparison. | Keep existing program tests and script contracts; audit request timing and junction arbitration during Stages 1 and 5. Do not rebuild these as if unsupported. |
| Several stop comments claim that `00620058` initializes station metadata or that `007dac5e` performs bay entry. [Enclosing-function disassembly](H:/marcel_omsi/assembly/00620004.asm) shows `00620058` inside a list traversal/removal routine, not the claimed metadata parser. At `007dac5e`, the inspected vehicle routine compares a human activity byte, not a docking-distance field. | **Uncertain source claims:** stop length/side are imported, but `BusService` still uses constant `BAY_REACH = 30`. These comments do not prove that stop string 4 is OMSI's docking-distance parameter. | Re-establish stop length, boarding region, docking reach, and lateral placement as separate concepts. Verify the actual metadata reader and motion branches; do not automatically replace `BAY_REACH` with `stop_length` based on a misleading comment. Stages 2 and 6. |

These findings make the rewrite more than an organizational cleanup: it must retain content information currently dropped at adapters, preserve already implemented OMSI mechanisms, replace conflicting state ownership, and intentionally improve unsafe traffic decisions. Detailed unverified branch meanings can be resolved within their owning stage instead of delaying the entire project.

## 3. Target architecture: layers and ownership

Create one small, headless `crates/traffic` domain crate. It depends on basic math and justified geometry utilities, not `core`, `simulation`, rendering, audio, map loading, scripting, or network runtime. Both `simulation` and `core` may depend on it; it never depends back on them. The name is proposed and should be checked against workspace conventions before extraction.

Use ordinary Rust structs/enums and narrow module interfaces. Do not introduce an ECS, generic behavior-tree framework, plugin system, or one crate per rule to solve this refactor. A module is justified by state ownership and a testable responsibility, not an arbitrary line-count target.

| Layer | Responsibility and owner | Inputs -> outputs |
| --- | --- | --- |
| L0: Content integration | `core` adapters translate existing scenery, spline, AI-list, and timetable data. Parsing remains in existing content crates. | Loaded content -> typed network/rule/service updates with source provenance. |
| L1: Network and rules | `traffic::network`, `rules`, `signals`: stable topology, lane geometry, legal movements, conflict areas, signal program state. | Network updates + clock/signal requests -> queryable network version and rule/signal state. |
| L2: World and perception | `traffic::world`, `perception`: entity identity, physical occupancy, local neighbors, route-relative obstacles, downstream storage. | Realized poses + external road users -> immutable tick snapshot and local observations. |
| L3: Intent and service | `traffic::routing`, `service`, `population`: destination/route progress, stop service, desired maneuvers, admission requests. | Snapshot + service/demand commands -> intents and candidate actions. |
| L4: Interaction decisions | `traffic::junctions`, `maneuvers`: rules, conflict arbitration, berth assignment, safe merges/passing, commitment/release. | Candidate actions + occupancy -> admitted actions and typed constraints. |
| L5: Control and motion | `traffic::following` selects bounded speed/acceleration; existing `simulation::ai_motion` realizes steering, pose, and articulation. | Admitted trajectory + constraints -> motion command -> realized motion feedback. |
| L6: Engine integration | `core::traffic_runtime` adapters own `VehicleInstance`, scripts, assets, rendering, audio, passenger exchange, and LAN replication. | Domain commands/events -> engine effects; measured feedback -> next domain update. |
| Cross-cutting: Diagnostics | `traffic::diagnostics` and a headless scenario runner explain behavior without changing it. | Decisions/state -> traces, metrics, invariants, replay artifacts. |

Suggested module layout, built incrementally as each stage needs it:

```text
crates/traffic/src/
  lib.rs                  small public API; no gameplay implementation
  ids.rs                  stable entity/lane/stop/trip identifiers
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
  tests/                  module-local tests where appropriate
crates/traffic/tests/      synthetic multi-vehicle behavior scenarios
crates/core/src/traffic_runtime/
  mod.rs                  integration facade
  content.rs              map/route/vehicle capability translation
  vehicles.rs             motion realization and script handshake
  passengers.rs           stop wishes, doorway holds, service events
  presentation.rs         rendering and audio synchronization
  replication.rs          host snapshots and client presentation
```

Initially keep `simulation::traffic` re-exports where necessary so scenery construction, passengers, rail, and existing tests can migrate separately. Shared network primitives retain `LaneKind` support for street, sidewalk, rail, and air; road decisions must not run on every path kind. Keep specialized rail/air motion in `simulation` and adapt its shared occupancy rather than forcing it through car following. Delete compatibility facades when their callers have migrated.

### Core contracts

- **Stable identities:** `VehicleId`, `LaneId`, `StopId`, `TripId`, and duty identity survive container reordering. Existing `LaneKey` content identity is retained, including travel direction. Use generation/version validation when a streamed network change can invalidate handles; vector positions are internal implementation details.
- **Vehicle capabilities:** immutable length, width, bumper/axle offsets, wheelbase, steering limits, braking limits, vehicle/path class, door locations/sides, and trailer geometry. Validate these once; keep reported fallback provenance. Bus service role and physical vehicle class are separate concepts.
- **Route progress:** lane plus distance along the current directed route occurrence; loops cannot be represented by lane ID alone. Stop targets refer to a specific route occurrence, platform side, and explicit bumper/door docking geometry. Normalize the current `Stop::s`, `stop_shift`, origin/front-bumper conventions at the import boundary.
- **Snapshot:** one network version, simulation tick, signal state, physical road-user poses, velocities, footprints, commitments, and service state. It includes AI, player, placed/parked vehicles, remote vehicles, pedestrians, trailers, and rail crossings where relevant.
- **Constraint:** typed reason, owner/blocker ID, route-relative stopping location or speed bound, validity, and provenance. Examples: `RedSignal`, `Yield`, `OccupiedExit`, `Leader`, `Pedestrian`, `BerthBusy`, `DoorHold`, `RoutePending`, `InvalidRoute`. Preserve all active causes and separately identify the binding constraint.
- **Decision:** route/maneuver intent and service transitions proposed from the snapshot. Arbitration grants shared resources; individual planners cannot mutate another vehicle or silently reserve a whole road.
- **Motion command/feedback:** approved trajectory, desired acceleration/speed, steering/speed bounds, signals; followed by actual pose, velocity, swept footprints, route progress, and maneuver completion. Motion realization is the only writer of physical pose. Domain progress is committed from realized movement on the known route, not an independently advancing invisible vehicle.
- **Events:** stop arrival, boarding permission, close request, departure, trip completion, fault, and removal are explicit and emitted once per transition. `schedule`, passengers, scripts, and LAN adapters consume them through typed boundaries.

Private module state should have one writer. For example, only service changes stop phase; only maneuver arbitration owns lateral intent; only junction arbitration creates/releases junction commitments. Presentation reads committed state. Debugging never grants permission to move.

### Tick pipeline and clock rules

1. Apply ordered, timestamped inputs: network updates, script feedback, passenger requests, schedule/demand commands, and external road users. Validate handles. Advance signal programs consistently with the chosen clock policy.
2. Freeze a snapshot; build spatial and route occupancy indexes from realized bodies.
3. Produce route/service/maneuver intents and candidate constraints from that snapshot.
4. Arbitrate conflicts deterministically. Priority rules precede fairness; stable IDs resolve genuine ties. Derive downstream/berth capacity from the same snapshot and admitted actions.
5. Select the final trajectory and longitudinal command. Check the combined proposal for collisions and violated bounds; a hard stop can override comfort limits when physically needed.
6. Realize motion through the vehicle adapter with bounded substeps and swept-body checks. Validate combined results before commit; independent worker motion must not create unreviewed conflicts.
7. Commit feedback/state transitions, update resource occupancy/release, and publish events and a presentation snapshot. Script output becomes explicitly timestamped feedback for the next tick.

Start evaluation at a fixed 20 ms tick, then validate cost and accuracy before freezing the value. Rendering interpolates committed poses; redraw no longer determines AI decisions. Pause must freeze motion and dwell correctly. Distinguish elapsed motion time from calendar/service time, preserving the engine's documented time-speed semantics after verification. A time jump is an explicit reset/rebuild transaction, not one enormous motion update. Bound per-frame catch-up work, retain or explicitly account for remaining simulation debt, and expose overload; do not silently lose time independently in traffic, doors, and schedules. Timestamp/interpolate player and remote inputs at the simulation boundary so identical input streams replay consistently.

## 4. Staged implementation roadmap

Every stage should land through focused PRs with a named reviewer/owner, a documented interface, scenario evidence, and a rollback boundary. Mechanical extraction and intentional behavior changes belong in separate PRs. Passing a stage gate means passing the previous relevant gates too.

### Stage 0 — Lightweight preparation; rare-bug reproduction is optional

**Purpose:** establish enough direction to start the replacement immediately, without asking the user to produce a failure they have never encountered.

- Adopt the behavior goals, current source findings, and OMSI coverage matrix above as the initial backlog. The screenshot remains an example of reported symptoms, not an implementation prerequisite or a proven root cause.
- Record the source revision and inventory existing tests/callers. Define the first synthetic scenario specifications: queue discharge, a blocked junction exit, and several buses sharing a stop. They deliberately create challenging conditions; they need not reproduce the original screenshot's hidden cause.
- Define the minimum decision/blocked-reason capture to add at the Stage 1 seam. Set initial behavior targets as provisional; measure performance and finalize numerical tolerances when the relevant stage can run.

**Deliverables:** a short backlog, integration/test inventory, and first scenario/trace specifications. This plan supplies the starting backlog and contract; no separate lengthy investigation report is needed before code work.

**Stage 0 artifacts:** [docs/traffic_refactor/](traffic_refactor/README.md) holds the adopted backlog, the integration/test inventory, the S1–S3 scenario specifications with provisional targets, and the Stage 1 decision/blocked-reason capture and extraction-boundary specification.

**Exit gate:** the first extraction boundary and scenarios are specified. Stage 1 can start without a live queue reproduction, an OMSI recording session, completed replay tooling, or a benchmark suite. Missing reproduction data is recorded as unknown and never promoted into a claimed diagnosis.

### Continuous validation track — Build, try, capture, refine

This runs alongside Stages 1–9 and is not an extra stage gate before implementation.

- Use short trial-and-error cycles: implement one boundary/behavior, run its synthetic cases and a representative map session, inspect actual decisions, then adjust. Save seed/config/input conditions so a useful experiment can be repeated.
- Build executable queue/stop/junction scenarios with the Stage 1 runner, then add cases when an implementation exposes a new failure. Do not require tests for the new engine to exist before the new engine has a test seam.
- Add a bounded rolling trace and automatic capture for unexplained stationary/crawling queues, cyclic blockers, overlaps, contradictory claims, invalid routes, and impossible service transitions. Include all active reasons, stable IDs, network version, signal/service state, motion feedback, and lifecycle decisions. Classify valid red/boarding/layover waits separately to avoid treating them as errors.
- If the rare reported failure occurs during development or a tester supplies a capture, turn that trace into a regression case. Reproducing it manually is optional; known failures that do become reproducible must be resolved before declaring them fixed.
- Compare selected OMSI behavior when the relevant content is available or a static inference needs clarification. Record source certainty, runtime observations, and intended improvements separately. Do not block unrelated stages on one unresolved legacy detail.
- Measure baseline/replacement cost progressively, beginning with the first runnable seam; freeze acceptance envelopes before that behavior's rollout and final performance budgets before Stage 9 cutover.

The developer/implementation owns this validation work. The user is not required to locate the screenshot map, provoke the rare bug, or manually operate a full reference-comparison suite to make progress.

### Stage 1 — Extract boundaries and establish the simulation seam

**Purpose:** make subsequent fixes reviewable without rewriting every algorithm at once.

- Extract existing network/rule/signal primitives into the headless crate with temporary re-exports. Preserve behavior and existing tests during the move.
- Split asset/presentation/audio/LAN ownership from traffic state. Replace direct external mutation of `Traffic::cars`, network, reservations, and service fields with queries, commands, and events, caller group by caller group.
- Introduce stable IDs, validated vehicle capabilities, route-progress/stop coordinate types, typed blocking reasons, and the adapter interface.
- First reproduce the old stepping contract through the seam; then introduce fixed ticking in a separate behavior PR. Align scheduler, script feedback, passenger exchange, signal updates, pause, and time-reset handling with that boundary.
- Add a minimal headless scenario runner and trace schema/version. Keep the current runtime selectable at session start during migration. Never switch a live vehicle between incompatible state models.
- Implement the first automatic failure capture at this seam, and make the three Stage 0 scenario specifications executable here. Expand the capture and scenarios with each later stage.

**Deliverables:** compiling domain crate, thin integration facade, working replay seam, and dependency/ownership documentation.

**Exit gate:** moved algorithms retain characterization results; headless scenarios require no renderer or OMSI assets; fixed-tick replays match across different render-frame partitions; no circular crate dependency exists.

### Stage 2 — Validate the road network and content semantics

**Purpose:** prevent decision code from compensating for wrong topology or ambiguous rules.

- Compile directed lanes, legal successor movements, lateral neighbors, stop lines, conflicts, stop approaches, and source provenance. Prefer explicit content connections where available; isolate and explain geometric fallback linking.
- Validate direction, endpoint continuity, elevation, vehicle permissions, widths, zero-length paths, ambiguous joins, duplicated paths, and disconnected timetable segments. A bridge cannot conflict with the road below it solely because their XY projections overlap.
- Separate routing permission, movement legality, and random-traffic density weights. Scheduled buses must not inherit random-car density restrictions accidentally.
- Normalize speed limits, path priorities, traffic-light associations, block paths, driving side, and supported vehicle restrictions. Distinguish actual rule data from assumptions inferred from scenery; unsupported semantics produce diagnostics.
- Close the adapter gaps from the coverage audit: retain `[crossingproblem]`, both `[blockpath]` values, and the full braking configuration with confidence/provenance. Preserve unknown semantics as data plus diagnostics rather than discarding fields or guessing their meaning. Behavior implementation follows once each interpretation is supported.
- Consolidate timetable route compilation currently spread across `slots`, `skip_detours`, `bridge_gaps`, connectors, and reversed paths. Return `Complete`, `PendingTiles`, or `Invalid` with an explanation; artificial connectors require validated direction, clearance, and provenance.
- Compile stop targets against the correct route occurrence and serviceable platform side. Use vehicle-aware door/bumper offsets, stop length, and a feasible approach; report an unreachable or malformed berth rather than generating an impossible docking target.
- Version network updates atomically and invalidate affected route/conflict caches. Unloaded geometry and permanently missing topology are distinct states.

**Deliverables:** validated network/route compiler and diagnostics identifying the originating map/path/stop data.

**Exit gate:** looped routes, adjacent opposite-direction stops, left-hand traffic, shallow conflicts, bridges, streaming joins, and malformed synthetic content have deterministic expected outcomes. Existing useful network tests remain intact.

### Stage 3 — Unify occupancy, perception, and snapshot decisions

**Purpose:** make every vehicle reason about the same physical world.

- Build lane-interval and spatial indexes once per tick. Track vehicles spanning multiple lanes, lane transitions, articulated rear sections, trailers, docking/passing footprints, and height ranges.
- Provide route-relative leader, crossing approach, downstream storage, swept-clearance, berth occupancy, and pedestrian observations with consistent coordinate conventions.
- Use realized geometry as occupancy truth; route projections accelerate queries but do not replace body checks. Reconcile controller progress with body feedback locally on the planned route; never snap to a nearby parallel road.
- Route all candidate decisions through immutable snapshots. Add an explicit deterministic arbiter for reservations and simultaneous merges; reserve enough exit space for all admitted vehicles, not each vehicle independently against the same empty space.
- Remove ad hoc previous-frame blocker/leader exceptions only after equivalent scenarios pass. Preserve a reason trail that explains how the original check was replaced.

**Deliverables:** shared perception API, occupancy/resource model, snapshot planning/commit flow.

**Exit gate:** entity-container reordering and worker scheduling do not change decisions for the same IDs and inputs; a bus rear still blocks a junction until clear; player/LAN trailers and bridge separation are correctly perceived.

### Stage 4 — Following, stopping, and motion correctness

**Purpose:** establish safe, natural basic movement before adding complicated maneuvers.

- Retain the existing IDM/ACC-style following and bicycle motion as initial candidates. Tune or replace them only against scenarios and measured limitations.
- Compose speed bounds from legal limits, vehicle capability, curvature, visibility, stop targets, and current maneuver. Vary preferred speed below the legal bound; personality must not grant permission to ignore rules.
- Use bumper-to-bumper gaps and actual leader speed/acceleration. Handle standstill, cut-ins, hard braking, uphill/downhill cases, and tiny lane segments without negative progress or numerical instability.
- Separate comfortable acceleration/deceleration/jerk from emergency collision prevention. Check braking feasibility ahead of a new speed limit or stop line; do not impose a hard clamp that creates visible teleportation.
- Incorporate verified vehicle-specific braking configuration instead of using only its stop offset. If a field's meaning remains unresolved, expose the explicit provisional fallback and keep the original value for later calibration.
- Keep persistent, seeded driver headway/reaction/comfort traits. Avoid random per-frame decisions and recurring reset of a launch timer when a constraint briefly fluctuates.
- Evaluate anticipation, continuous control, and varied launch waves against the natural-behavior contract in Section 1. Do not postpone natural movement to a final tuning pass.
- Reconcile commanded speed and realized body motion. Preserve steering limits, axle geometry, articulation, and ground contact; never mark a stop reached solely from a planner coordinate.

**Deliverables:** reusable longitudinal controller, motion/feedback adapter, calibrated behavior envelopes.

**Exit gate:** stopped and braking leader scenarios remain collision-free within their specified feasible initial conditions; red-light queues start and discharge smoothly with credible variation; stops and speed changes are anticipated without unexplained creep or brake/accelerate flicker; stopped buses meet docking tolerances; behavior is stable across frame rates and lane joints.

### Stage 5 — Junction admission, street rules, and deadlock diagnosis

**Purpose:** remove false blocking and unsafe attempts to escape gridlock.

- Centralize signal entry, right-of-way, turning conflicts, pedestrian protection, merges, emergency requests, and rail-crossing restrictions. Honor content-derived rules before using fallback road conventions.
- Implement the established semantics of per-path crossing flags and block-rule modes carried from Stage 2. Add scenarios proving how reservation refusal differs from entry refusal and how oncoming commitments affect each.
- Represent junction movements/conflict areas explicitly. Distinguish `Approaching`, `Waiting`, `Admitted`, `Inside`, and `Cleared`; occupied areas are different from speculative future requests.
- Admit only when the movement is legal, conflicting bodies/commitments are clear, and downstream space can store the full vehicle/consist. Reserve that storage against other simultaneous admissions. Once inside, clear the junction safely instead of reacting to an entry light as if still behind the line.
- Commit decisions with hysteresis and clear cancellation conditions. Release capacity/claims on tail clearance, route change, removal, or network invalidation; a clock timeout cannot erase a body that still occupies a conflict.
- Handle amber using stopping feasibility and the documented signal interpretation. Red, red/amber, inactive signals, request phases, and scripted lamp feedback need explicit coverage.
- Use deterministic priority and bounded fairness where the rules permit it. Remove the unconditional full-exit escape path; waiting longer cannot create road space. Emergency priority cannot authorize a collision or an impossible exit.
- Build a wait-for graph for persistent unexplained holds. Distinguish a cyclic stale-claim deadlock from legal congestion or physically full roads. Cancel stale speculative claims, retry a valid safe maneuver, or request route/lifecycle recovery. Never drive through a conflicting body or red signal just because a timer expired.

**Deliverables:** junction coordinator, traceable admission decisions, safe recovery categories.

**Exit gate:** four-way interactions, priority turns, roundabouts, conflicting claims, pedestrian/rail crossings, and blocked exits pass. When a downstream blocker is removed, admitted flows resume within the scenario's measured bound. If capacity remains physically unavailable, the system waits and explains why.

### Stage 6 — Bus service, docking, stop capacity, and duty lifecycle

**Purpose:** eliminate artificial bus rows without destroying legitimate shared-stop queues.

Implement explicit service transitions:

```text
EnRoute -> Approach -> WaitingForBerth -> Docking -> Boarding
        -> ClosingDoors -> WaitingToMerge -> Departing -> EnRoute
Trip completion -> Layover / NextTrip / OutOfService
Any applicable state -> RoutePending or ServiceFault (with reason)
```

Some states may be combined in code if they share ownership, but their transition conditions must remain explicit. A free curb stop can pass through `WaitingForBerth` immediately.

- Treat a stop as physical boarding geometry with capacity, not a point plus a wait timer. Start with one berth unless content/validated geometry supports more; model queues without inventing extra platforms.
- Assign berths in stable arrival order while respecting geometry and service constraints. Buses wait upstream with safe spacing, advance when a berth clears, and hold/release assignments by actual occupancy. A bus waiting 30 metres away has not served the stop.
- Permit boarding only after actual low speed, correct longitudinal/lateral placement, door/platform alignment, a valid berth, and the permitted door side are confirmed. Share this eligibility with passenger registration so service and passengers cannot disagree about arrival.
- Replace broad queued/crept-past arrival shortcuts with explicit overshoot handling. Use a feasible safe correction or record a missed/faulted stop; never open doors anywhere in the queue to make it disappear.
- Compute dwell from passenger exchange, door handshake, configured service/timing rules, and minimum service requirements. Optional stops use explicit demand and timetable semantics; timing points, termini, and layovers have distinct early-departure rules.
- Verify stop metadata and docking reach independently; the current comments' executable addresses do not establish them. Keep stop length, boarding region, approach distance, and vehicle stop correction separately named and measured.
- Adapt `AI_Scheduled_AtStation`, `AI_Scheduled_AtStation_Side`, station release, indicators, door requests, IBIS progression, and destination changes to the existing script contract. Treat acknowledgement, absent feedback, unsupported feedback, and proven unsafe doors distinctly. A timeout reports a fault; safe fallback for unsupported scripts requires a validated adapter rule.
- Keep the berth occupied during closing and merge waiting. Depart only with doorway holds cleared and a safe merge trajectory; release capacity once the rear clears. A following bus must not dock through a departing bus.
- Keep timetable assignment separate: one physical vehicle per active duty/service-day identity, explicit next-trip handover, no second spawn because a trip is late or temporarily outside loaded tiles. Preserve legitimate simultaneous departures from different duties.
- Keep terminus layovers out of moving lanes where valid standing space exists. Missing capacity remains a diagnosed content/service limitation. Do not silently clear remaining stops and turn a scheduled bus into random traffic after `ROUTE_WAIT_MAX`.

**Deliverables:** service state machine, berth coordinator, passenger/script adapter, explicit duty/spawn lifecycle.

**Exit gate:** at least three buses sharing a stop queue safely and each serves it once; following buses board only at a valid berth. Early/late service, optional stops, request stops, long boarding, failed door feedback, player-occupied stops, articulated buses, terminus handover, midnight/service-day rollover, and pending route tiles pass.

### Stage 7 — Lane changes, merges, passing, and parking

**Purpose:** add credible interaction without reintroducing independent lateral controllers.

- Use one maneuver owner for route-required lane changes, discretionary changes, stop docking, departure, overtaking, curb avoidance, and parking. Conflicting behaviors submit requests instead of overwriting `lateral_target` in different functions.
- Evaluate legal permission, forward/rear gaps, predicted occupancy, route commitment, available road width, steering feasibility, and abort/return space. A lane change must account for vehicles that are also changing lanes this tick.
- Plan route-required changes early and discourage repeated left/right oscillation. If a necessary change is impossible, choose a legal reroute/wait outcome rather than cutting through a queue or jumping to another lane.
- Distinguish stopped obstruction, short service dwell, moving leader, and parking departure. Passing a bus is optional and permitted only with a legal, fully checked trajectory; waiting is correct when passing cannot be safe.
- For oncoming-lane passing, check the whole maneuver including return clearance, pedestrians, intersections, visibility, and the vehicle's swept body/trailers. Retain useful `sweep_clearance`, pull-out, and return-ramp code behind the new interface.
- Handle aborts explicitly: safe return when feasible, otherwise complete the currently safe committed portion. Do not instantly snap back to the original lane.
- Restore verified traffic behavior events such as `ev_AI_Horn` through the script adapter, with appropriate cooldown and diagnostics. This is lower priority than safe movement and must not become a substitute for fixing a blocked maneuver.
- Bring parked-car pull-out and parking arrival under the same occupancy/admission system; static scenery parking and live moving vehicles cannot both own the same space.

**Deliverables:** unified maneuver arbitration and continuous trajectory execution.

**Exit gate:** simultaneous lane changes, required turn lanes, blocked bus bays, parking pull-outs, narrow lanes, articulated clearance, left-hand traffic, and oncoming aborts have safe deterministic outcomes. No indefinite maneuver oscillation appears under steady inputs.

### Stage 8 — Population, streaming, and recovery

**Purpose:** prevent lifecycle behavior from continuously recreating congestion or erasing evidence.

- Separate traffic demand, eligibility/admission, physical occupancy, and presentation visibility. A population target is not an order to put vehicles onto every available-looking lane immediately.
- Spawn only on valid paths with physical gaps and a feasible immediate continuation. Apply backpressure and bounded retry to busy entrances; never compensate for denied spawns by stacking vehicles or growing an unbounded pending queue.
- Preserve AI-list weights, group density/default semantics, day curves, parked-vehicle behavior, and scheduled-count options. Scheduled duty capacity and unscheduled demand are distinct budgets.
- Separate loaded topology from active collision/ground availability. Request needed tiles before a bus reaches a route frontier where feasible. Preserve duty, passengers, stop progress, and ownership while waiting or suspending safely.
- On leaving the active area, preserve identity and logical lifecycle. Simplified distant simulation must obey capacity and event rules; dormant motion cannot produce overlap when vehicles reactivate. Validate re-entry before physical placement.
- Define recovery by cause: invalidate stale claims; retry pending content; legally reroute random traffic; explicitly fault an invalid scheduled route; suspend/remove only through a documented lifecycle transition. Removing a bus must notify schedule and passengers and release resources exactly once.
- Maintain no-visible-pop behavior and all relevant viewers, including mirrors/free camera/LAN players. Offscreen despawning is a population policy, not the acceptance criterion for fixing stuck traffic.
- Keep host authority in LAN. Clients display replicated committed state and run required presentation scripts without independently making traffic decisions. Rejoining, authority changes, and time resets must not duplicate duty ownership.

**Deliverables:** bounded demand/admission queues, coherent streamed/dormant lifecycle, explicit recovery events, LAN integration.

**Exit gate:** overload creates diagnosed capacity-limited demand; queues are not fed by overlapping spawns. Repeated tile load/unload and camera-range changes preserve identities and duty counts. No scheduled service quietly disappears to conceal an unresolved blockage.

### Stage 9 — Calibration, performance, cutover, and deletion

**Purpose:** finish the replacement instead of maintaining two traffic systems indefinitely.

- Finish calibrating bounded driver diversity, launch waves, braking/jerk, turning speed, docking, merge behavior, and optional passing against controlled observations and the mandatory improvement contract. Natural movement is already covered in earlier stages; this pass evaluates the combined system. Centralize parameters with units, rationale, and provenance.
- Run dense, prolonged mixed-traffic scenarios with buses, pedestrians, parking, signals, streaming, and LAN integration. Sweep density, seed, vehicle mix, and frame stalls to discover rare failures automatically. Measure safety, progress, service reliability, CPU, allocations, and memory separately from rendering and asset loading.
- Optimize only measured hotspots: local spatial queries, sorted lane occupancy, cached conflict geometry, incremental graph updates, bounded route work, and reused buffers. Avoid all-vehicle scans inside each vehicle's junction decision. Parallelize pure planning/body work after deterministic arbitration is correct.
- Compare the old and new runtimes using identical recorded inputs. Shadow decisions may run read-only for a common replay; they must not emit scripts/events/spawns or participate in reservations. Once trajectories diverge, evaluate each engine's invariants and outcomes instead of treating exact trace equality as the desired result.
- Enable the replacement at new-session initialization after gates pass, with a short-lived development rollback switch. Do not promise live hot switching. Migrate or explicitly version any persisted traffic state and network payload changes.
- Remove the old road planner, obsolete exceptions/constants, temporary re-exports, direct mutable access, and rollback flag after the agreed stabilization window. Update developer documentation, diagnostics, and behavior notes.

**Deliverables:** default replacement, measured benchmark report, concise maintainer guide, removed legacy road path.

**Exit gate:** acceptance suite, workspace checks, and representative real-map stress/soak sessions pass; recorded representative sessions demonstrate the Section 1 natural-behavior criteria and concrete improvements over observed OMSI shortcomings; budgets are met on the named hardware; and only one production road-AI implementation remains. The unavailable screenshot reproduction does not block cutover. If a capture of that failure becomes available, include it in regression coverage; do not claim that exact failure is proven fixed without evidence. Remaining content limitations are documented with reasons and available reproductions.

### Stage 10 — Physical safety and post-cutover validation

Implementation and evidence: [STAGE10_REPORT.md](STAGE10_REPORT.md).

1. **10A — Ground and actual motion:** stable body/planner feedback, signed road-height
   correction, bridge-level selection, missing-contact handling and actual odometers.
2. **10B — Passing safety:** scenery/pedestrian clearance along outward and return paths,
   divided-road protection for issue #126, and no discretionary bypass of one's own berth.
3. **10C — Emergency traffic:** route-directed cooperative yielding, validated passing,
   reserved low-speed red entry, occupant drainage and complete-tail release.
4. **10D — Roundabout priority and verification:** preserve give-way at object-path merges,
   cover both traffic sides, run the workspace regressions, accelerated soak and installed
   map sessions, then record remaining content/engine discrepancies for player acceptance.

Automated checks pass. Follow-up ground verification covers driving over continuous grades,
query seams and isolated wrong contacts, as well as normal removal at an unlinked road end.
The exact screenshot-roundabout reproduction remains unverified; see the report.

## 5. Dependencies and practical PR order

```text
0 Lean preparation -> 1 Boundaries/clock -> 2 Network -> 3 Snapshot/occupancy
                                              -> 4 Following/motion
                                                 -> 5 Junctions -> 6 Bus service
                                                               -> 7 Maneuvers
                                           3 + 5 + 6 + 7 -> 8 Population/streaming
                                           all stages   -> 9 Cutover/deletion
Continuous: trial runs, automatic captures, focused OMSI comparison, calibration
```

Infrastructure for population and bus service can be extracted earlier, but behavior rollout follows these gates. Avoid assigning independent rewrites of junctions and bus stops before their shared occupancy and command contracts exist.

Recommended PR-sized sequence:

| Batch | Focus | Required result |
| --- | --- | --- |
| A | Lean ownership/content audit and three scenario specifications | Extraction can start without reproducing the screenshot. |
| B | Network extraction with re-exports | Existing geometry/rule tests survive unchanged. |
| C | IDs, private state, capabilities, integration seam | Adapters replace direct cross-system writes. |
| D | Fixed clock, headless runner, first scenarios, automatic capture | Frame partitions do not change decision ticks; discovered failures leave a trace. |
| E | Network/route/stop validation | Bad content differs from pending content. |
| F | Occupancy snapshot and arbitration | All planners see consistent bodies and capacity. |
| G | Following/stopping and feedback reconciliation | Basic traffic is physically stable. |
| H | Junction coordinator | No full-exit timer override or stale claims. |
| I | Berth/docking service | Queued buses cannot falsely arrive. |
| J | Door/passenger handshake and duty handover | Safe departure and exactly-once service ownership. |
| K | Lane-change/passing/parking controller | Lateral intent has one owner. |
| L | Demand, streaming, dormant state, LAN | Lifecycle does not create or hide traffic failures. |
| M | Calibration, parameter sweeps, benchmarks, default cutover | Representative maps and stress cases pass; any available reported-failure capture is covered. |
| N | Legacy deletion and maintainer documentation | Replacement is complete. |

These are dependency boundaries, not a promise of one PR per row. Split further when a reviewer cannot explain the state transitions and tests in one sitting. If an urgent fix is needed before its stage, reproduce it first and land a narrow fix plus regression coverage; do not grow another speculative recovery branch.

## 6. Acceptance scenarios and measurements

Synthetic fixtures should be redistributable and headless. Optional real-map runs can load content from `OMSI_ROOT`; do not commit proprietary assets. Keep existing network, light, following, stop-side, vehicle-class, bridge, queue-arrival prediction, and motion regression coverage. Characterization of known bad behavior records the baseline but must not become the permanent success criterion.

| Scenario | Required behavior | Principal stages |
| --- | --- | --- |
| Red signal with 20 queued cars, then green | No line crossing during red; safe spacing; queue discharges with bounded reaction waves after green. | 3–5 |
| Leader brakes, cut-in, temporary player blockage | Safe response for feasible starting conditions; smooth ordinary braking; motion resumes after clearance. | 3–4 |
| Four-way/priority junction, turning conflict, roundabout | Legal priority, no conflicting admission, no alternating stop/go claims, measurable progress when legal gaps exist. | 2–5 |
| Explicit `[blockpath]` modes and `[crossingproblem]` flags | Import preserves both block values and the per-path flag; decisions follow the established semantics, including reservation versus entry distinctions. Unresolved interpretations are identified rather than silently flattened. | 2–5 |
| Full downstream exit for more than 45 seconds | Upstream remains outside the conflict; releasing the blocker restores flow; waiting duration cannot bypass capacity. | 3–5 |
| Stale reservation and a truly full closed road loop | Stale claim clears safely; physically full loop is diagnosed without forced collisions or a false promise of automatic progress. | 5, 8 |
| Three buses sharing one curb stop | One berth occupant; other buses wait/advance; each arrival/departure occurs once; no doors opening far up the queue. | 3, 6 |
| Stop beside opposite-direction lane; stop on a route loop | Correct direction, route occurrence, door side, and boarding position. | 2, 6 |
| Bay near a junction; articulated bus; blocked bay | Feasible approach and full-body clearance; queued bus does not count as docked. | 3, 6–7 |
| Busy/empty/request/timing-point stops, early/late bus | Dwell and skip policy match explicit service semantics; delay does not duplicate a duty. | 6 |
| Door acknowledgement absent or stuck; passenger in doorway | Unsupported feedback has a tested adapter policy; known open/unsafe doors or doorway holds prevent departure; faults are observable. | 6 |
| Different vehicle braking configurations and stop corrections | Verified braking capabilities affect anticipation/control; stop correction remains separate from braking strength and boarding geometry. | 2, 4, 6 |
| Terminus layover/next trip, midnight, clock jump | Exactly one vehicle owns the duty; stop/IBIS progression and lifecycle remain consistent. | 1, 6, 8 |
| Bus pull-out with following car; simultaneous lane changes | Compatible trajectories only; safe gaps; no repeated maneuver oscillation. | 3, 7 |
| Parked obstruction and oncoming traffic | Pass only when legal and feasible including return; otherwise wait safely. | 7 |
| Pedestrian/rail crossing and road under bridge | Correct crossing protection; no false elevated-road blockage. | 2–5 |
| Missing route, streaming extension, dormant reactivation | Pending versus invalid route is explicit; no teleport, overlapping re-entry, lost passengers, or duplicate duty. | 2, 8 |
| Excess spawn demand and multiple viewers/LAN players | Bounded admission/retry; no overlapping spawns or visibility-dependent rule changes; host authority maintained. | 8 |
| 15/30/60/144 FPS, frame stalls, pause, varied worker count | Same committed decisions/events for equivalent timestamped inputs on the same platform; presentation may interpolate differently. | 1–9 |

Proposed initial measurement gates below are calibrated as their stages become runnable and frozen before each behavior's rollout; final budgets are fixed before Stage 9 cutover. They are neoOMSI targets, not constants established by the reference report, and completing all measurements is not a prerequisite for Stage 1:

- **Safety:** zero AI-created body overlaps and conflicting grants in feasible synthetic scenarios; zero red-entry, prohibited-direction, and prohibited-movement violations. Use swept geometry and include trailers, not screenshots alone. Infeasible sudden player intrusions are classified separately and require an emergency response.
- **Docking:** initial target is longitudinal error within 0.5 m, lateral error within 0.25 m, and speed below 0.1 m/s before boarding permission. Measure against the vehicle/platform boarding geometry, not simply the stop object's origin. Replace these numbers if verified content geometry requires a different documented envelope.
- **Progress:** each designed clearance scenario specifies a deadline from blocker removal to first movement and full discharge, derived from its route length, acceleration, and reaction limits. Do not use one arbitrary timeout for every junction, boarding event, or over-capacity road.
- **Service:** exactly one active physical/logical owner per duty instance, once-only served/skipped stop events, zero false arrivals and duplicate trip spawns, and zero departures with known unsafe door/doorway state. Legitimate dwell and service delay are reported separately from unexplained hold time.
- **Natural motion and interaction:** record acceleration, jerk, lateral acceleration, steering rate, headway, emergency-brake frequency, unnecessary stop/restart cycles, maneuver reversals, and delay after a usable gap appears. Normal driving remains within the capability/comfort envelope; emergency exceptions carry a reason. Review representative recordings for anticipation, convincing variation, signaling, docking, and cooperative merging. Record the observed OMSI shortcoming and the measured/visible improvement for comparison scenes; identical trajectories to OMSI are not the target.
- **Replay:** seeded repeat runs preserve decision/event hashes on the same platform; reordering storage or changing worker count has no semantic effect. Cross-platform comparisons use documented floating-point tolerances rather than promising bit-for-bit physics.
- **Performance:** benchmark at 100, 500, and 1,000 active road vehicles with both ordinary and congested junction loads. Record p50/p95/p99 tick cost, allocation rate, memory, and streaming update cost on named hardware. Set an explicit budget from the frame/tick target and baseline; domain tick cost excludes rendering/asset upload, and integration cost is measured separately.
- **Soak:** initial release gate is a 60-minute seeded dense mixed-traffic run plus repeated streaming/time-reset cycles. No unresolved claim/berth/duty ownership leak, monotonic pending-queue growth under bounded admitted demand, or unexplained persistent stall. Preserve failure traces for replay.

Future implementation checks include focused crate/scenario tests, formatting/lint checks used by the repository, `cargo nextest run --workspace`, and `cargo build --release` before release/cutover. Run costly broad checks at integration gates, not after every mechanical file move. This planning change itself needs document/link review rather than runtime tests.

## 7. Keeping the replacement maintainable

- Each module documents what it owns, what it reads, its invariants, its state transitions, and one representative scenario. Keep a short maintainer walkthrough from input to committed pose/event.
- A developer must be able to select a vehicle and see its route, actual footprints, intended maneuver, applicable rules, all blockers, binding constraint, berth/claim owner, and next transition guard.
- Parameters have units and provenance: content-defined, observed compatibility behavior, deliberate improvement, or provisional tuning. Put shared physical bounds and tuning in named configuration types; do not scatter unexplained numbers or map-name exceptions.
- New behavior needs a reproducible discrepancy or a declared design requirement, a clear owner, and a scenario asserting externally meaningful outcomes. Avoid tests that merely repeat the implementation formula.
- Recovery never changes legality, erases physical occupancy, or silently abandons a scheduled service. It publishes its cause and lifecycle effects.
- Keep module APIs concrete. Add abstractions when a second real use case needs them; remove superseded helpers and temporary migration code.
- Review generated code with the same ownership standard as handwritten code. A PR should explain why a vehicle stops, why it may move next, and how the change preserves surrounding integrations.

## 8. Risks and decisions to resolve during implementation

| Risk or unknown | Resolution / stage |
| --- | --- |
| Screenshot context and first blocked vehicle are unknown; the user has never encountered it | Proceed from source/reference gaps and deliberately congested scenarios. Automatic capture runs during development; investigate that exact queue only if it becomes available. |
| Report findings and existing address comments are incomplete/inferred | Use the targeted audit's confidence labels and progressively compare relevant behavior during its owning stage; no copied legacy algorithm or mandatory initial reference session. |
| Some maps encode rules/berths incompletely or place stops poorly | Provenance-aware validation and a narrow documented fallback in Stage 2; diagnose impossible service geometry. |
| Legacy content fields are parsed but discarded or unused | Track `[crossingproblem]`, `[blockpath]` modes, braking data, and event dispatch in the audit matrix; preserve input information and close the owning stage's gaps. |
| Two independent route/body positions drift | Single physical-pose owner, local route feedback, swept checks, and commit validation in Stages 3–4. |
| Fixed ticks expose script/passenger update-order assumptions | Timestamped handshake tests and integration clocks in Stages 1 and 6; do not silently double-step scripts. |
| Correct downstream-capacity checks reveal real congestion | Demand backpressure in Stage 8; diagnose capacity limits instead of bypassing rules. |
| Fairness accidentally removes lawful priority | Rule-first arbitration and explicit legal-gap scenarios in Stage 5; no universal forced-turn timeout. |
| Unsupported door scripts need compatibility fallback | Capability-aware adapter contract in Stage 6; test absent feedback separately from known unsafe doors. |
| Network/route extraction affects passengers, trains, aircraft, and scenery | Temporary re-exports plus integration coverage; keep specialized behavior boundaries. |
| Asset loading/visibility makes spawning nondeterministic | Separate logical duty/admission from resource readiness; replay ordered readiness inputs; validate physical placement at commit. |
| Scope grows into a new whole-engine architecture | Keep the traffic crate and adapters small; defer unrelated physics, passenger, and rendering rewrites. |

The first implementation milestone is **a headless seam, private ownership of traffic state, executable synthetic queue scenarios, and automatic failure capture**, reached without reproducing the screenshot. The final milestone is **one maintainable default traffic runtime with demonstrably more natural and correct behavior than OMSI 2 in representative comparison scenes, reliable bus service, bounded population behavior, and the old road controller removed**.
