# Passengers

Passenger simulation lives in `crates/omsi-app/src/humans/`.

## State and movement

A local passenger's `Pax.pos` and `Pax.yaw` own movement. `Pax.inside` selects world
or cabin coordinates. Reserving a seat does not board the passenger. `Person.position`,
heading and place are projections for drawing and shared crowd queries; pedestrians and
remote mirrors own their `Person` pose.

Seat and waiting-place reservations belong to the passenger lifecycle; cancellation releases
held places. Cabin walking follows its directed path graph. Ground correction updates the
passenger root and its projection together. Seated waiting uses the map seat marker for the
hips and the walkable floor for the feet. Exit handoff changes coordinate frames at one
world position.

## Transactions and presentation

Fare ownership follows stable person IDs. The driver's `GivenTicket` is captured as a
frame input. Fare UI state and physical tray money have separate lifetimes.

LAN transfers use protocol 7 and stable identities. The host retains a passenger until
acceptance; rejection restores its waiting task. A transfer completes only on its session
receipt.

Rendering and procedural poses do not own passenger simulation state. `pax_motion` selects
natural or OMSI-style movement; `pax_ik` independently selects procedural or OMSI animation
poses. RealisticPax models are optional and load only when installed and selected.

Run focused and workspace tests with:

```powershell
cargo nextest run -p omsi-app --lib humans:: --locked
cargo nextest run --workspace --locked
```

Passenger compatibility tests use synthetic assets. Ignored `OMSI_ROOT` audits are developer diagnostics; tests do not establish OMSI reference parity, real-map visuals or separate-process LAN behavior.
