# Berlin 156 follow-up and Traffic AI debug tools

## Using the overlay

In a developer build, open the existing developer UI and choose **Game → Traffic AI**.
The overlay uses the existing ImGui GPU debug renderer and projects world-space points,
just like the light and walk overlays. It is drawn over scenery, without depth occlusion.

- Green: the vehicle's planned path, sampled every 3 m for the next 60 m.
- Yellow: an active lane change or passing maneuver.
- Blue: physical body boxes, including articulated trailer sections.
- Red: a blocking vehicle connection or the current stopping constraint.
- Cyan: the next scheduled bus stop's vehicle-origin target. This is **not** the pole
  location or the front bumper. Vehicle brake-performance corrections and bay offsets
  belong to stop setup; comparing the pole to a bumper alone cannot diagnose docking.

Labels show vehicle ID, speed, lane, binding reason and bus-service phase. The window
also shows lane distance, clearance, maneuver phase, ground support and motion faults.
Use the radius slider to reduce clutter, or click a vehicle / enter its ID. ID 0 restores
the nearby fleet. At most 64 nearest vehicles are included; selecting an ID overrides
the radius. The snapshot and route samples are collected only while this window and
the developer UI are open.

**Copy diagnostic report** copies the currently displayed fleet and timing information
to the clipboard and writes it to the application log. Send that text with a screenshot
showing the problematic vehicles. It includes map, camera, simulation time, fleet size,
adapter, viewport and graphics settings as well as vehicle reasons and stop targets.
It does not capture a history of individual decisions or a full deterministic replay.

FPS / frame time include rendering. AI timings cover presence, planning and body/script
phases of fixed ticks, excluding GPU and overlay drawing. The history samples the most
recent tick at UI collection time; its p95 is a sampled phase sum, not the full-frame
p95 or a measurement of every tick. Reset timing history after changing settings.
The existing **Graphics → Performance** window provides broader frame diagnostics.
Disable Traffic AI drawing when comparing normal play FPS, since the overlay has a cost.

## Corrections

The report concerned Berlin 156, entry Prenzlauer Allee/Ostseestraße (158), Wednesday
09:00, with a stopped articulated bus and traffic waiting on both same-direction lanes.

- Discretionary lane changes now participate in target-lane arbitration. Previously only
  existing or route-required changes submitted requests, so a safe discretionary pass
  never obtained permission. Stopped cars can bypass a serving bus on a free adjacent
  lane, with gap and full trajectory checks. Red-light queues retain their protections.
- The nearest-leader calculation no longer subtracts the current lane position twice;
  it uses the leader's rear and follower's front. Changes use projected adjacent-lane
  coordinates and required changes win over optional changes deterministically.
- Off-lane physical perception excludes the querying vehicle's own body sections.
  A stationary serving bus is not predicted to accelerate into neighboring traffic.
  Boarding buses do not reserve a future route-required lane change.
- The normal-road scenery guard uses the actual authored physical body box and rest
  height. Passing-specific padding remains in maneuver validation, where clearance is
  needed. The old normal-road guard could falsely block the bus before docking.
  Unchanged poses no longer repeat the physical overlap query.
- Scenery collision stops lazily at the first genuine clipped mesh hit and rejects
  irrelevant height layers early. Ground queries stop after passing the reference height.
- Emergency approach forecasts are built only for an active emergency or a reservation
  whose tail still needs clearing. Existing reservations retain their lifetime checks.
- Repeated blocked passing sweeps have a 0.5 s retry interval. Occupancy queries visit
  only intersecting grid cells, expanded by the fleet's body extents, rather than a fixed
  25-cell neighborhood per path sample. This also handles long bodies more correctly.

No arbitrary offset was added to move every bus relative to every visible stop pole.
The Berlin trace now reaches docking, boarding, closing and departure; the debug marker
makes any remaining authored target or model-origin issue measurable in the user's scene.

## Validation

- `cargo test --workspace`: **1,278 passed, 0 failed, 23 ignored**.
- Eight multilane regression tests cover moving and stopped bypasses, occupied-lane and
  scenery vetoes, red queues, a free neighbor corridor, arbitration order, boarding-bus
  reservations and spatial-query boundaries / long bodies against a linear reference.
- A collision regression compares lazy and full eager mesh queries over 783 probes,
  including hollow rotated meshes, different heights and headings.
- The debug executable builds. Native interactive inspection of the new window and its
  clipboard button remains for the user's next run; compilation is not visual QA.
- Follow-up: opening the overlay with physical boxes originally panicked because its
  box renderer acquired an already-live background draw list. Boxes now use the caller's
  draw list. An actual ImGui frame regression reproduced that panic before the correction
  and exercises routes, boxes, labels and neighboring overlays across repeated frames.
  The corrected `cargo test -p core --lib` run passes 422 tests (7 ignored).
- `cargo check --workspace --all-targets` and `cargo check -p core --no-default-features`
  both pass, including the configuration without developer tools.
- The separate ignored 60-minute dense mixed soak passes (180,000 fixed ticks per
  simulated owner, no leaked coordinator claims / queues).

## Real-map measurement

The baseline is `e89c826` with the same offscreen profiling instrumentation. Both runs
use entry 11, nine loaded tiles (`--radius 1`), 100 requested traffic vehicles, scheduled
buses/passengers, the GN92 player held stationary, 120 simulated seconds at 50 Hz,
1024×576, Wednesday 2026-10-07 09:00. CSV tracing is enabled in both. This is a debug
build on RTX 5070/DX12; the timing is CPU AI work, not a live-window FPS benchmark.
Behavior fixes change how vehicles move and which remain active, so workload is not
identical despite matching settings. Buses may reach an unloaded route edge afterwards.

The baseline GN92 remains in Approach before docking and repeatedly hits the old scenery
guard. With the corrections the scheduled GN92 and EN92 both serve stops and depart.
The isolated run records 5,998 ticks:

| CPU time per fixed tick | Baseline mean | Follow-up mean | Baseline p95 | Follow-up p95 |
| --- | ---: | ---: | ---: | ---: |
| Presence | 0.033 ms | 0.036 ms | — | 0.053 ms |
| Planning | 0.817 ms | 0.659 ms | 1.041 ms | 0.905 ms |
| Bodies / scripts | 0.226 ms | 0.227 ms | 0.347 ms | 0.353 ms |
| Full traffic tick, including tracing | 1.203 ms | 1.052 ms | 1.520 ms | 1.381 ms |

Planning mean is about 19% lower and full tick mean about 13% lower. Average active
fleet is 49.73 before and 47.05 afterwards; final active counts are 65 and 57. The changed
behavior means these are scenario measurements, not an equal-workload speedup guarantee.
Compared with the same follow-up behavior before tightening the occupancy grid query,
planning mean falls from 0.899 to 0.659 ms and total from 1.368 to 1.052 ms, with identical
sampled bus phases. Actual gameplay FPS still needs measurement on the user's normal
view, density and graphics settings; the new diagnostic report provides that context.

Both scheduled buses record Boarding, ClosingDoors and Departing. There are no new
normal-road scenery-block warnings. The GN92 subsequently enters RoutePending at a
loaded route edge; 33 sampled ground-unavailable holds are also present elsewhere in
its trace. These remain visible diagnostics, not evidence that every map surface or
streaming boundary is now perfect. The final PNG is the stationary player bus, not a
visual record of the earlier scheduled-bus docking sequence.

Artifacts are under `target/`: `berlin-stop-before.{log,csv,png}` and
`berlin-traffic-optimized.{log,csv,png}`. The offscreen render includes only a final frame;
its cold rendering/readback time cannot establish normal gameplay FPS.
