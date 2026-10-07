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

## Current state after Stage 1 (batches B, C1–C5a, D1, D2)

- `crates/traffic/src/lib.rs` holds the code formerly in `crates/simulation/src/traffic.rs`,
  unchanged except for severing a debug-only `legacy_config::env` call (behaviour-equivalent
  `std::env::var_os`) so the crate stays minimal.
- `crates/simulation/src/traffic.rs` is a re-export shim.
- New domain modules: `ids.rs` (stable `VehicleId`/`LaneId`/`StopId`/`TripId`/`DutyId`/
  `NetworkVersion`), `capabilities.rs` (`VehicleCapabilities`, `VehicleClass`,
  `CapabilitySource`), `routing.rs` (`RouteProgress`), `service.rs` (`StopTarget`,
  `PlatformSide`), `diagnostics.rs` (`Reason`, `Constraint`, `JunctionState`, `ServicePhase`,
  `TraceEvent`, `TRACE_VERSION`, `Capture`).
- `crates/core/src/traffic_runtime/` holds `content.rs` (capability adapter) and the adapter
  modules `vehicles.rs`, `passengers.rs`, `presentation.rs`, `replication.rs`. Query methods
  on `Traffic` (`car_count`, `dormant_count`, `target`, `set_target`) are the first migrated
  caller group; the rest migrate caller-group by caller-group.
- Headless scenarios: `crates/traffic/tests/s1_red_queue.rs` (red queue discharge) and
  `s2_blocked_exit.rs` (blocked downstream storage) run with no renderer or OMSI assets.
  S3 (shared stop) needs the service adapter and is not yet executable headlessly.
- Fixed ticking: the frame-driven window path steps AI decisions on a fixed 20 ms tick with
  bounded catch-up (`core/src/app_events/redraw/ai_traffic.rs`); the offscreen path already
  used a fixed 1/30 s step.
- The module split named in the plan beyond the above is done incrementally as each
  boundary is needed.
- `simulation` keeps `LaneKind` (street/sidewalk/rail/air); rail/air motion stays in
  `simulation` and is adapted later rather than forced through car following.

## Deletion rule

Do not remove the shim, the `Traffic` public fields, or any compatibility re-export until
its callers have migrated (see `TEST_INVENTORY.md` for the caller groups). A temporary
re-export must have a named migration target in the Stage 1 backlog (`A1`–`A5`, `A9`).
