# Traffic AI refactor — integration and test inventory

Recorded during Stage 0 at revision `35a460a54e51ed28f9fb081b4adf16c8c73993a8`. Line
numbers are from that revision and must be re-checked if code moves before it is used.

## Source map

| File | Lines | Role |
| --- | ---: | --- |
| [`crates/core/src/traffic.rs`](../../crates/core/src/traffic.rs) | 8417 | `Traffic`: network, vehicles, population, junctions, passing, parking, lights, scripts, presentation, LAN mirroring |
| [`crates/simulation/src/traffic.rs`](../../crates/simulation/src/traffic.rs) | 3898 | Geometry, topology, rules, signals, routing, per-car `AiState::drive` and `TrafficLightController` |
| [`crates/core/src/schedule.rs`](../../crates/core/src/schedule.rs) | 5865 | Duty allocation, spawning, route resolution, stop projection, connectors, streaming extension |
| [`crates/core/src/bus_service.rs`](../../crates/core/src/bus_service.rs) | 589 | `BusService`: approach/arrive/closing/depart stop state machine |
| [`crates/simulation/src/ai_motion.rs`](../../crates/simulation/src/ai_motion.rs) | 1119 | Bicycle steering, rate limits, ground contact, swept clearance, rail/air motion (`AiBody::step`) |
| [`crates/core/src/app_events/redraw/ai_traffic.rs`](../../crates/core/src/app_events/redraw/ai_traffic.rs) | 162 | Redraw-driven traffic/schedule/passenger stepping |
| [`crates/core/src/app_events/redraw/setup.rs`](../../crates/core/src/app_events/redraw/setup.rs) | 251 | Frame timing; caps `dt` at 0.1 s |
| [`crates/core/src/traffic_link.rs`](../../crates/core/src/traffic_link.rs) | 115 | Player/LAN outline and `traffic_inputs` glue |
| [`crates/core/src/lan_world.rs`](../../crates/core/src/lan_world.rs) | 1712 | LAN mirror tick and replication |
| [`crates/core/src/offscreen.rs`](../../crates/core/src/offscreen.rs) | — | Existing headless loop that builds and steps `Traffic` |

Module wiring: `crates/simulation/src/lib.rs` (`pub mod traffic`, `pub mod ai_motion`),
`crates/core/src/lib.rs` (`mod traffic`, `mod schedule`, `mod bus_service`,
`mod traffic_link`, `mod lan_world`, `mod offscreen`).

## Existing tests

There are no `tests/` integration directories in `core` or `simulation`; all existing
traffic tests are inline `#[cfg(test)]` modules.

### `crates/simulation/src/traffic.rs`

| Module | Lines | Tests |
| --- | --- | --- |
| `extend_tests` | 1671–1776 | `extend_links_like_link`, `start_grid_tracks_added_lanes_in_map_order`, `added_lanes_only_change_reach_of_their_predecessors` |
| `tests` | 3117–3845 | `a_stop_is_matched_to_the_lane_it_stands_beside`, `path_rules_open_lanes_by_vehicle_type`, `slows_down_for_a_bend`, `a_turning_lane_is_signalled_in_advance`, `a_light_shows_every_aspect_for_its_time_at_any_frame_rate`, `stock_state_codes_mean_what_the_lamp_scripts_show`, `a_level_crossing_waits_for_its_train`, `a_bus_phase_is_skipped_when_no_bus_comes`, `a_backwards_jump_extends_green_once_per_cycle`, `a_car_stops_at_the_line_without_braking_hard`, `a_follower_keeps_its_distance_when_the_leader_brakes`, `right_of_way_between_paths`, `right_of_way_on_the_left`, `a_shallow_crossing_is_a_long_meeting_place`, `a_driver_with_a_choice_keeps_out_of_a_dead_end`, `the_way_has_no_steps_at_a_lane_joint`, `upstream_walks_back_through_the_junction`, `an_acceleration_cap_holds_a_car_back`, `arrival_times`, `moving_back_in_clears_the_oncoming_lane_part_way` |
| `light_tests` | 3847–3880 | `remaining_green_of_a_pedestrian_light` |
| `aurora_pool_tests` | 3882–3898 | `independent_rules_and_default_references` |

### `crates/core/src/traffic.rs`

| Module | Lines | Tests |
| --- | --- | --- |
| `road_scale_tests` | 8244–8255 | `path_density_scales_the_street_target` |
| `junction_arrival_tests` | 8257–8312 | `stopped_queue_does_not_predict_a_restart`, `crawling_queue_is_measured_at_its_actual_speed`, `crawling_queue_allows_a_gap_but_nearby_traffic_still_counts`, `queue_already_at_the_conflict_still_blocks`, `freely_starting_car_keeps_its_accelerating_prediction`, `car_waiting_before_the_conflict_is_not_approaching` |
| `group_density_tests` | 8314–8417 | `a_bus_under_a_bridge_is_not_in_the_way_on_it`, `a_following_bus_is_no_bus_in_the_way`, `a_group_off_by_default_drives_where_a_path_asks_for_it`, `a_default_follows_the_first_group_on_the_path`, `defaults_naming_each_other_end` |

### Adjacent tests touching the same boundaries

- `crates/core/src/bus_service.rs` 528–589: `standing_time`,
  `only_the_ends_of_the_trip_and_an_early_bus_stop_for_nobody`,
  `station_side_comes_from_the_stop_it_boards_at`.
- `crates/simulation/src/ai_motion.rs` 823–1119: bend following, nose-dive/roll-out,
  no-steer-round-bus, pull-out room, pull-out ramps, aircraft height.
- `crates/core/src/schedule.rs` 5186–5865: 19 tests covering route/IBIS/duty logic.
- `crates/core/src/humans/tests.rs` and `crates/core/src/passenger_compat_tests.rs`
  construct `simulation::traffic::{LaneBuilder, Network}` directly.

## Public entry points and callers

### `Traffic` (`core::traffic`)

- Simulation: `Traffic::tick(&mut self, dt: f32, player: Option<PlayerBox>)`
  ([`crates/core/src/traffic.rs:5504`](../../crates/core/src/traffic.rs)); `sync`
  (`:7587`); `update_audio` (`:7027`); private `junction_stop` (`:4696`, called only from
  `tick`); private `mirror_tick` (`:8066`).
- Population/spawn: `populate`/`populate_seen`, `reset_population`, `add_tiles`,
  `attach_cars`, `spawn_bus`, `spawn_clear`, `park_in`, `pull_out_parked`, `reroute`,
  `release`, `remove_car`.
- Read/hooks: `cars` (public field), `boxes`, `blocked`, `car_pose`, `stuck_report`,
  `signal_aspects`, `switch_requests`, `light_*`, `set_pax_requests`, `set_stop_wishes`,
  `hold_boarding`.
- Constructed in `crates/core/src/app.rs` and `crates/core/src/offscreen.rs`.

Callers: `app_events/redraw/ai_traffic.rs` (window path), `offscreen.rs` (headless),
`schedule.rs` (`attach_cars`, `spawn_bus`, `blocked`, `spawn_clear`, `reroute`,
`remove_car`), `lan_world.rs` (mirror), and many read-only consumers of `Traffic::cars`
(camera, environment, world, humans, dev-tools, navigator, placing, admin, applog).

### `simulation::traffic`

- `AiState::drive(&mut self, net, dt, lead, stop) -> bool`
  ([`crates/simulation/src/traffic.rs:2977`](../../crates/simulation/src/traffic.rs));
  `AiState::advance` (`:2867`).
- `TrafficLightController::advance(dt)` (`:1909`), `state`, `remaining`, `aspect`,
  `allows_go`, `lamps`.
- `Network` (`:429`) with `link`, `compute_conflicts`, `compute_reach`, `build_grid`,
  `extend`, `shortest_path`, `nearest_lane*`, `project_stop_on_route`, `must_yield`,
  `upstream`, `lane_along`.
- `Lane`, `LaneBuilder`, `LaneKind`, `LaneKey` are built directly by tests and by
  passenger compatibility tests.

### `BusService` and `Schedule`

- `BusService::step(&mut self, st, vehicle, ctx) -> Option<f32>`
  ([`crates/core/src/bus_service.rs:333`](../../crates/core/src/bus_service.rs)); `Ctx`
  carries `dt` (`:138`). Built/stepped from `core/src/traffic.rs`.
- `Schedule::tick(world, traffic, renderer, scene, day_time, window)`
  ([`crates/core/src/schedule.rs:1990`](../../crates/core/src/schedule.rs)) takes **no
  `dt`** and advances on `day_time`; called from `ai_traffic.rs` and `offscreen.rs`.

## The `dt` seam

1. `Frame` carries `now`, `raw_dt`, `dt`
   (`crates/core/src/app_events/redraw/mod.rs`).
2. `raw_dt = (now - last).as_secs_f32()`, then `let dt = raw_dt.min(0.1);`
   ([`setup.rs:182`](../../crates/core/src/app_events/redraw/setup.rs)).
3. `redraw_traffic` passes the capped `dt` to `t.tick(dt, ...)`
   ([`ai_traffic.rs:116`](../../crates/core/src/app_events/redraw/ai_traffic.rs)).
4. `Traffic::tick` distributes `dt` to light programs (`c.advance(dt)`), `BusService::Ctx`,
   `AiState::drive`, and `AiBody::step`.
5. The offscreen loop mirrors this contract with its own loop `dt`
   ([`offscreen.rs:712`](../../crates/core/src/offscreen.rs)); `Schedule` uses `day_time`
   instead.

Consequence for Stage 1: any fixed-clock change must keep the existing `advance`/`drive`/
`step` entry points usable with synthetic fixed `dt`, since unit tests call them directly.

## Existing headless / diagnostic tooling

- `run_offscreen(...)` ([`crates/core/src/offscreen.rs`](../../crates/core/src/offscreen.rs))
  is the de-facto headless harness: builds `Traffic` and `Schedule` and steps traffic per
  frame. Driven by CLI flags (`--offscreen`, `--drive`, `--traffic`, `--schedule`,
  `--exit-after`, `--snapshots`, `--follow`, `--situation`, ...) and env profiles
  (`OMSI_DRIVE_PROFILE`, `OMSI_DRIVE_V0`, `OMSI_DEBUG_PHYSICS`).
- `OMSI_TRACE_AI=<file.csv>` writes one row per car per frame with header
  `t,id,type,x,y,z,heading,pitch,bank,steer,speed,lane,s,blinker,turn,lane_heading,lateral,at_station,acc,yielding,light_hold,passing,front,rear,half_width,scheduled,why,why_gap,phase,lane_z`
  ([`crates/core/src/traffic.rs:861`](../../crates/core/src/traffic.rs)).
  `OMSI_TRACE_AI_BUSES=1` restricts it to timetable buses.
- `OMSI_TIMETABLE_TRACE`, `OMSI_LAN_TRACE`, `OMSI_TRACE_PAX`, and `OMSI_PROFILE` add
  narrower CSV/stage timing dumps.
- `AiCar::why`, `holding`, `geo_block`, `junction_why` already carry ad-hoc blocker hints
  for these env diagnostics ([`crates/core/src/traffic.rs:181`](../../crates/core/src/traffic.rs)).

Gap: these are frame dumps, not replayable inputs and not assertion-capable. There is no
deterministic scenario runner, no trace schema/version, and no automatic failure capture.
Stage 1 (`A9`) provides the first of these.

## Build, test, and lint tooling

- Test runner: **cargo-nextest**. No `.config/nextest.toml` and no extra profiles exist.
- CI: `cargo nextest run --workspace --locked` on `windows-latest`
  (`.github/workflows/test.yml`); shader test `cargo nextest run --locked --release -p render`.
- Contributor command: `cargo nextest run --workspace`; release build `cargo build --release`
  ([DEVELOPMENT.md](../DEVELOPMENT.md), [CONTRIBUTING.md](../../CONTRIBUTING.md)).
- **No** `cargo fmt` or `cargo clippy` anywhere in CI or `scripts/`. No root
  `rustfmt.toml`, `.rustfmt.toml`, `clippy.toml`, or `[workspace.lints]`.
- **No** `criterion`, `benches/`, or `[[bench]]` targets. Performance gates in
  [SCENARIOS.md](SCENARIOS.md) therefore have no harness yet and stay provisional.
- Changelog: `.changes/<pr>.<category>.md`, categories `parity`, `fix`, `feature`,
  `performance`, `breaking`, `docs`, `internal` ([.changes/README.md](../../.changes/README.md)).
