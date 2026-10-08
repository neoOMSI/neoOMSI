# Docking, neighbouring lanes and emergency response

These are requested neoOMSI traffic improvements, not claims of verified OMSI parity.

- Road stop placement uses the leading body's actual front extent, including bounding-box
  origin offset, minus the existing content holding correction. Rail alignment retains
  its half-car convention. The cyan debug marker now shows the front holding target.
- Lane-change occupancy records the body's realized lateral offset on each claimed lane.
  A target reservation alone does not become a physical leader on the adjacent lane.
  Shared-exit following starts within the stopping approach to the actual merge.
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

Regression coverage lives in `crates/traffic/tests/reported_blockers.rs` and
`crates/simulation/src/ai_motion/traffic_regression_tests.rs`. The latter uses the actual
bicycle-model body and realization feedback, not the synthetic maneuver fixture alone.
The original map scenes still require an in-game replay to verify asset-specific geometry.
