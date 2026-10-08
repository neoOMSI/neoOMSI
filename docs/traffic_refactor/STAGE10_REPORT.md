# Stage 10 — physical safety, emergency traffic and regression verification

Implemented on `refactor/traffic_ai`, starting from `f128e51`. Automated verification and
two installed-content sessions completed on 2026-10-08. Manual player acceptance remains
with the user; these results do not establish bug-free behavior on every addon map.

## Changes by layer

### 10A — Actual motion and ground contact (`simulation`, engine ground adapter)

- Removed an unstable longitudinal catch-up servo from `AiBody`. The planner already
  advances its command before realization; adding catch-up speed and then feeding it back
  amplified speed every tick. A real body/planner regression failed before the fix at tick
  104 and now stays within its 50 km/h command for 5,000 ticks.
- Wheel contact selects the nearest road surface on the current level, including signed
  corrections for imperfect path heights. Drive faces take precedence over buried terrain.
  Previous contact and local grade carry the reference forward. Bridge levels remain
  separate. Routine queries use a provisional 1.5 m bound; whole-body loss of nearby
  contact can reacquire an available surface within 3 m. Neither value is OMSI-derived.
- One missing wheel uses the other wheel's axle support. A missing axle extends the plane
  confirmed by remaining contacts, including its grade/crossfall. Partial support does
  **not** stop the vehicle before a road/tile end. Normal path-end removal remains intact.
- During motion, isolated contacts more than 0.35 m off the previous plane are discarded
  if other tyres still confirm it. This prevents a bad query onto buried terrain from
  tipping or sinking the body; an actual continuous uphill/downhill grade keeps advancing.
- When exact tyre probes all miss, real nearby faces within 2 m can confirm the same road
  plane before a wider vertical reacquisition. A short query seam no longer drops the body
  onto the lower terrain. Complete absence of usable support remains diagnosable as
  `ground_unavailable`; known road support is not silently replaced by path height.
- Road script/planner odometers now accumulate realized distance, including zero for a
  rejected step. Player rigid-body probing is unchanged.
- Installed VW Golf 2 and MAN SD80 model-offset tests verify that the static offset is
  applied exactly once while driving 25 m with path offsets of -2.7, +0.9 and +2.7 m and
  an isolated bad wheel query. This is asset-backed coverage, not a check of every bus.

### 10B — Passing and scenery (`traffic::maneuvers`, `core::traffic::safety`)

- Full vehicle width, height and overhangs are checked against streamed scenery collision
  geometry through an engine callback. The domain keeps its renderer/asset independence.
- Outward and return paths, and lane-change transitions, are sampled with trajectory
  headings. Pedestrians are supplied to production maneuver planning. A blocked transition
  cannot become permitted just because a vehicle has waited a long time.
- Separate opposing carriageways are rejected as passing alternatives when their centers
  imply an intervening strip beyond the lane widths. This addresses the grass-median
  example at Ballenbergstraße in [issue #126](https://github.com/neoOMSI/neoOMSI/issues/126).
  The 0.6 m tolerance is provisional and conservative; it is not a full road-marking model.
- Safe passing of a stopped bus remains enabled on an ordinary clear two-way road.
  A scheduled bus queuing for its own berth does not start a discretionary pass.
- A newly obstructed realized road pose is rejected when the prior pose was clear, with
  `scenery_blocked` diagnostics. Collision-free spawn poses still depend on content/admission;
  this guard is not a universal continuous collision solver or trailer physics replacement.
- Overtaking checks use scenery's authored collision geometry. Render-only objects lacking
  collision data cannot automatically acquire correct physical behavior through this change.

### 10C — Cooperative emergency behavior (`traffic::emergency`, junction owner)

- Active emergency requests follow directed route occurrences. Traffic ahead slows and
  shifts toward an available lane edge, mirrored for left-hand traffic, after checking
  scenery, bodies and pedestrians. Narrow lanes do not acquire imaginary passing space.
- Emergency passing uses the same validated maneuver system. Ordinary vehicles retain
  their signals and priority constraints; they are not pushed through a red light.
- Short exclusive junction reservations let current occupants/committed traffic clear,
  then hold new conflicting entries. An emergency may cross red only near its own reserved
  junction at at most 5 m/s. Occupied exits, pedestrians and physical conflicts still veto
  entry. Reservations release on tail clearance, removal or network invalidation; realized
  rear sections retain protection even after the warning state is switched off.
- Existing unscheduled emergency scripts use `TrafficPriority`; explicit `AI_Emergency`
  overrides it. Scheduled buses are not classified as emergencies solely for ordinary
  script priority. The local player is integrated too. Remote LAN emergency-state
  replication is not added by this stage.

### 10D — Roundabout/merge priority (`traffic::junctions`)

- An entry lane may belong to the same scenery object as the circulating path. Previously
  the `inside`/committed shortcut could skip give-way behavior before the actual merge.
- Directed merge priority now continues to apply there. A stoppable entrant or its
  speculative claim does not take priority from circulation. Actual bodies already in
  the conflict, explicit blockpath rules, and entrants unable to stop stay protected.
- Priority follows imported path rules, not a guessed circular shape. Missing/wrong
  authored rules can still require a content correction. Tests cover right/left traffic,
  reversed planning order, a stale speculative entry claim, occupied merge and a clear gap.
- The exact roundabout in the user's screenshots has not been reproduced locally; its
  map/location was requested. The synthetic failure was demonstrated before the fix.

## Verification

| Check | Result |
| --- | --- |
| `cargo test --workspace --quiet` | **1,269 passed, 0 failed, 23 ignored**; ignored asset/long-running tests are not counted as ordinary passes |
| `cargo check --workspace --all-targets` | Passed; existing hard-link cache warnings and unused launcher helper warning remain |
| `cargo test -p traffic --test s9_soak -- --ignored` | 60 simulated minutes / 180,000 ticks; passed in 47.43 s; domain harness, not a 60-minute graphical map session |
| `cargo test -p traffic --test s10_emergency` | 10 passed, including 5,000 warning/route/removal/network lifecycle cycles |
| Installed-content model-offset/motion test with `OMSI_ROOT` | Passed for VW Golf 2 and MAN SD80, including displaced path heights and a bad wheel query |
| `cargo tree -p traffic --edges normal --depth 1` | Only `glam`, `hashbrown`, `log` |
| `git diff --check` | Passed |

New regression sources: `simulation/src/ai_motion/ground_tests.rs`,
`core/src/traffic/safety.rs`, and `traffic/tests/s10_{passing,courtesy,emergency,roundabout}.rs`.
The full suite also retains the prior bus berth/door/duty, following, junction, streaming,
articulated-vehicle and determinism scenarios.

Follow-up ground regressions cover normal dead-end removal without stopping first, driving
on a surface displaced from path Z in either direction, continuous up/downhill travel
against an incorrect flat path, a short whole-body query gap, and an isolated wheel query
onto lower terrain. The last two failed before their fixes (a 2.7 m drop and an incorrect
body rotation/height change) and pass afterward.

### Installed-map sessions

Content root: `G:\SteamLibrary\steamapps\common\OMSI 2`. No installed assets were copied
into the repository or modified. Runs used the offscreen engine with stationary player
brake profile, 40 requested traffic vehicles, schedules and passengers enabled.

| Session | Evidence |
| --- | --- |
| Städtedreieck V3, Ballenbergstraße, 06:33–06:36, after ground follow-up | 180 simulated seconds, 354,071 CSV records including player; 129 distinct AI IDs over churn, peak AI speed 14.984 m/s. No `ground_unavailable` or `scenery_blocked` samples. Maximum absolute sampled AI pitch 3.06°, bank 2.158°. Cars wait behind the player bus instead of using the grass median. `target/stage10-stadtdreieck-ground-fixed.{png,log}`, `target/stage10-stadtdreieck-ground-fixed-ai.csv` |
| Grundorf, 06:33–06:38 | 300 simulated seconds, 341,100 CSV records including player; 138 distinct AI IDs over churn; peak 22.805 m/s. Ordinary traffic, trucks and the RTW exercised emergency yielding (4,159 sampled holds); no `ground_unavailable` or `scenery_blocked` records. No scheduled AI bus appeared in this session. `target/stage10-grundorf.{png,log}`, `target/stage10-grundorf-ai.csv` |

Distinct IDs are cumulative, not concurrent fleet size. Frame samples are not incident
counts. The first exploratory Städtedreieck run exposed speeds up to 405.302 m/s in the
unstable feedback loop; the isolated regression established the cause before removal.
Neither scene capture alone proves the complete absence of clipping or queue bugs.

### Corrected diagnosis and verified road-end behavior

The initial report incorrectly classified `(6984, -2214)` as an unresolved path/ground
height discrepancy. Inspection of the imported network established that this is the
**unlinked end** of spline `5600473`, path 3, tile `(23, -8)`, lane 679 in these runs:
length 83.4 m, ending at `(6984.4, -2214.5)`. The road is at 13.300 m; front tyres beyond
its end queried the lower terrain at 10.590 m while the rear axle remained on the asphalt.
Requiring every axle to have nearby contact caused the premature hold.

That condition is corrected by partial support and normal path-end removal, not by moving
the car down onto terrain beyond the street. In the final 180-second trace, **27 distinct
AI vehicles** have samples within 3 m of this end: **356 samples, zero stopped samples**.
Their last samples reach s≈83.3–83.4 at normal speed before normal removal. The prior
2,113 sampled ground holds disappear entirely in the new run.

Several missing addon vehicle, texture/font and material warnings still limit exact
reproduction of the supplied setup. Continuous-road ground/rotation cases have additional
reproducible body regressions; the exact original intermittent addon-map failure was not
supplied as a replay and is not claimed to be universally eliminated.

`OMSI_DEBUG_AI_GROUND=1` records coordinates and probes when axle support is lost.
`OMSI_TRACE_AI=<absolute CSV path>` records the corresponding vehicle decisions.
Trace schema version is now **8**.

## Manual follow-up

For player acceptance, reproduce Ballenbergstraße with the original bus and check the
reported roundabout with the same map/settings as the screenshots. Check stopped-bus
passing with both a clear oncoming gap and an occupied return path, bus queues at a shared
berth, emergency release after the complete vehicle clears, and raised/sloped roads.
Record location, vehicle and trace reason for remaining failures. These are validation
targets; no forced despawn, timer-based priority override or teleport is introduced as a
substitute for a safe maneuver.
