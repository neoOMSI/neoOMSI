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
stops can create waiting passengers after the scheduled call only while an actual bus
still has that call ahead and its existing service deadline is valid. AI progress comes
from the bus's remaining stop queue plus calls waiting for streamed lanes; player progress
comes from the active duty. Served or skipped calls do not create replacement passengers,
including when the bus leaves early. Without an active bus, overdue timetable entries
do not create new demand. Upcoming departures retain the normal five-minute window.
The stop cube's entering counts, hourly passenger density and
available waiting places continue to limit the crowd. The window and headless server both
refresh targets as simulation time advances.

Regressions for #420 and #422 cover a delayed trip entering new stop regions, a 23:55
departure calling after midnight, previous-day 24:xx departures, invalid service days,
non-stopping calls, crowd limits and expired passengers. Additional regressions cover
served stops, early departures, completed trips, absent buses, equal-time calls, stops
whose lanes are still unloaded, later departures and progress across service days. Run them with:

```powershell
cargo test --locked -p core --lib passenger_targets
cargo test --locked -p core --lib a_newly_loaded_stop
```

The optional `installed_tettau_keeps_passenger_targets` test reads Tettau's installed
timetable via `OMSI_ROOT` and checks all intermediate stops of its Kronach trip five
minutes late; invoke it with `-- --ignored`.

The supplied local OMSI 2.2.032 static reference (executable SHA-256
`7dab063d1f62e73b3a2c7a6ac1921d7edf5e5db0fbc731481d117eec8de7d759`) was inspected:
the station update at `0061c044` combines stop entering counts, a station factor, demand
and global density (`0061c257`–`0061c27c`), then limits occupancy by waiting places.
The target refresh at `0061a118` adjusts previous-day times by 86400 seconds
(`0061a2e6`–`0061a2f8`). These are static navigation/behavior findings, not a runtime
comparison or verification of neoOMSI's existing deadline formula. A controlled drive in
OMSI and neoOMSI is still needed to establish full parity.

## Transactions and presentation

Fare ownership follows stable person IDs. The driver's `GivenTicket` is captured as a
frame input. Fare UI state and physical tray money have separate lifetimes.

LAN transfers use protocol 7 and stable identities. The host retains a passenger until
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
