# Passengers

Passenger simulation lives in `../crates/core`.

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

## Waiting demand and timetable time

Timetable targets use the traffic clock, which continues past midnight. Calls belonging
to a trip that started before midnight keep the previous day's validity mask. Newly loaded
stops can create waiting passengers after the scheduled call while its existing service
deadline is still valid. The stop cube's entering counts, hourly passenger density and
available waiting places continue to limit the crowd. The window and headless server both
refresh targets as simulation time advances.

Regressions for #420 and #422 cover a delayed trip entering new stop regions, a 23:55
departure calling after midnight, previous-day 24:xx departures, invalid service days,
non-stopping calls, crowd limits and expired passengers. Run them with:

```powershell
cargo test --locked -p core --lib passenger_targets
cargo test --locked -p core --lib a_newly_loaded_stop
```

The optional `installed_tettau_keeps_passenger_targets` test reads Tettau's installed
timetable via `OMSI_ROOT` and checks all intermediate stops of its Kronach trip five
minutes late; invoke it with `-- --ignored`.


## Transactions and presentation

Fare ownership follows stable person IDs. The driver's `GivenTicket` is captured as a
frame input. Fare UI state and physical tray money have separate lifetimes.

LAN transfers use protocol 8 and stable identities. The host retains a passenger until
acceptance; rejection restores its waiting task. A transfer completes only on its session
receipt.

Rendering and procedural poses do not own passenger simulation state. `pax_motion` selects
natural or OMSI-style movement; `pax_ik` independently selects procedural or OMSI animation
poses. RealisticPax models are optional and load only when installed and selected.

Natural movement lets passengers who buy nothing from the driver board at `[exit]` doors
too: open ones, or shut ones whose outside button the script reads as the next
`PAX_Entry<n>_Req` after the cabin's entries. They let people off first and give up a
shut door after 5 s for an open one. Ticket buyers keep to the selling entries; OMSI-style
movement keeps OMSI's entry-only choice.

Run focused and workspace tests with:

```powershell
cargo nextest run -p core --lib humans:: --locked
cargo nextest run --workspace --locked
```

Passenger compatibility tests use synthetic assets. Ignored `OMSI_ROOT` audits are developer diagnostics; tests do not establish OMSI reference parity, real-map visuals or separate-process LAN behavior.
