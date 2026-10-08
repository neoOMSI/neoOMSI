# Docking, neighbouring lanes and emergency response

These are requested neoOMSI traffic improvements, not claims of verified OMSI parity.

- Road stop placement uses the leading body's actual front extent, including bounding-box
  origin offset, minus the existing content holding correction. Rail alignment retains
  its half-car convention. The cyan debug marker now shows the front holding target.
- Lane-change occupancy records the body's realized lateral offset on each claimed lane.
  A target reservation alone does not become a physical leader on the adjacent lane.
  Shared-exit following starts within the stopping approach to the actual merge.
- Swept body clearance uses the normal of each cross-section as a separating axis in
  addition to the body's axes. Independently growing an angled bus's box on its two
  local axes alone creates false contacts beyond its corners.
- Articulated rear sections retain the heading of the realized collision box, which
  is already in radians. Converting it a second time rotated a 149.6-degree bus rear
  to about 2.6 degrees in perception and falsely blocked its neighbouring lane.
- Scheduled lane changes keep their source lane through motion feedback until the change
  finishes. Both longitudinal coordinates follow the realized source projection.
- An overshot docking target, or a stopped bus at the target that cannot finish its lateral
  alignment, records `MissedStop` after eight continuous seconds. The berth is released and
  the vehicle continues without boarding. Vehicles waiting upstream keep their stop.
  Buses already centred on their lane can depart without a follower granting a merge.
- Active emergency response targets 130% of the posted speed, capped at 20 km/h above it
  and at the vehicle's own maximum speed. Curve limits and physical following still apply.
  Normal scheduled priority alone does not activate emergency response.
- Courtesy affects both sides of an authored multilane carriageway, not unrelated parallel
  roads. Vehicles roll at up to walking speed to move aside when space permits, then wait.
  The response vehicle uses the corridor between the innermost lane and its neighbour
  once the physical sweep is clear. On wider carriageways it first needs a safe lane
  change into that pair. Full queues, red signals for yielding cars, pedestrians and
  scenery remain constraints; response does not guarantee passage where no room exists.
- Ordinary traffic retains its driving-side preference and may overtake a slow leader on
  a free passing lane. This is intentional; both driving-side conventions have coverage.
- A bus bypass uses the continuous parallel carriageway rather than requiring 40 metres
  on one spline. Berlin's 30-metre pieces previously disabled it completely. Random
  traffic retains its lane-change blend across an unbranched, aligned pair of spline
  joints. Destination gaps include preceding and following pieces; forks and signals
  do not extend the corridor. The short six-metre bypass limits pull-out acceleration
  and checks the whole vehicle, including its front corner, against realized bodies.
- Cars following a docking bus reserve their vehicle-specific steering room in the
  following model as well as the holding target. Waiting until the bus was fully stopped
  let cars creep to their ordinary minimum gap (2.45 metres in the report), where a safe
  pull-out was no longer possible. Timetable buses queueing for their own stop retain
  their existing queue behaviour.

Regression coverage lives in `crates/traffic/tests/reported_blockers.rs` and
`crates/simulation/src/ai_motion/traffic_regression_tests.rs`. The latter uses the actual
bicycle-model body and realization feedback, not the synthetic maneuver fixture alone.
The Prenzlauer Allee/Ostseestraße case was also reproduced with the installed Berlin 156
content in the offscreen game pipeline (1989-07-12, 09:00, 55 traffic vehicles, timetable
and passengers enabled). At simulation time 28 s the same bus remains boarding in the
same position: the two left-lane cars that previously stood still now pass at approximately
51 and 47 km/h. Other asset-specific layouts still need their own replay.
