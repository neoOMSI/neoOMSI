# Traffic AI refactor — measured performance and soak report (Stage 9)

Measured for the Stage 9 exit gate. All numbers are from the headless domain benchmark and the
accelerated soak, on the named hardware below. They are **domain** costs: they exclude
rendering, asset upload, vehicle scripts and body realization, which the engine measures
separately (see "Integration cost"). The benchmark is deliberately asset-free and
redistributable.

## Hardware and toolchain

| | |
| --- | --- |
| CPU | AMD Ryzen 7 5800X (8 cores / 16 threads) |
| RAM | 32 GB |
| OS | Windows 11 Pro 10.0.26200 |
| Toolchain | rustc 1.98.1, cargo 1.98.1 |
| Profile | bench (release + debuginfo), `cargo bench -p traffic --bench domain` |
| Revision | `refactor/traffic_ai` Stage 9 |

## Method

`crates/traffic/benches/domain.rs` is a `harness = false` benchmark (std only; **no**
dependency is added — `cargo tree -p traffic` stays `glam`, `hashbrown`, `log`). Each measured
tick rebuilds the frozen `Occupancy` from the realized bodies and plans every vehicle's
junction admission against that one snapshot: the same "snapshot then decide" flow
`core::Traffic::tick` uses. A counting global allocator records allocations and bytes per
tick, and peak live heap. `BENCH_TICKS` (default 60) sets the measured tick count; the numbers
below use `BENCH_TICKS=200` (maneuver: 30, because its per-tick scene is heavier per vehicle).

The soak (`crates/traffic/tests/s9_soak.rs`) runs the full 60-minute simulation (180,000 ticks
at the 50 Hz fixed clock) across every owner in turn — junction claims, population admission,
service berths and lateral maneuvers — while churning vehicles in and out and periodically
invalidating the network, then asserts that every commitment/berth/queue is released and the
bounded queues stayed bounded. It is `#[ignore]`d; run it with
`cargo test -p traffic --test s9_soak -- --ignored`.

## Domain tick cost

100/500/1000 active vehicles, ordinary (spread along the approaches) and congested (packed on
the stop lines) junction load. `allocs/tick` and `bytes/tick` are per fixed step; `peak` is the
peak live heap during the case.

| Case | p50 (ms) | p95 (ms) | p99 (ms) | allocs/tick | bytes/tick | peak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| junction 100 ordinary | 0.017 | 0.018 | 0.031 | 423 | 39,580 | 60 KiB |
| junction 100 congested | 0.020 | 0.026 | 0.045 | 453 | 40,552 | 63 KiB |
| junction 500 ordinary | 0.138 | 0.213 | 0.274 | 2,041 | 198,632 | 276 KiB |
| junction 500 congested | 0.135 | 0.215 | 0.238 | 2,061 | 199,016 | 278 KiB |
| junction 1000 ordinary | 0.304 | 0.514 | 0.665 | 4,053 | 396,680 | 544 KiB |
| junction 1000 congested | 0.388 | 0.718 | 1.140 | 4,065 | 397,000 | 545 KiB |
| maneuver 100 congested | 0.007 | 0.016 | 0.035 | 49 | 37,336 | — |
| maneuver 500 congested | 0.031 | 0.051 | 0.065 | 176 | 205,640 | — |
| maneuver 1000 congested | 0.064 | 0.125 | 0.125 | 321 | 411,768 | 766 KiB |
| streaming: extend 50 lanes + relink | 0.080 | 0.184 | 0.228 | — | — | — |

Per-tick allocation is linear in the vehicle count (~4 allocations/vehicle/tick for the
junction path, dominated by the per-tick scene and occupancy rebuild), and the whole-rebuild
scene model is what keeps decisions independent of container order (Stages 3 and 8).

## Budget

The fixed simulation tick is 20 ms (`traffic::scenario::SIM_DT`). The worst measured domain
tick — 1000 active vehicles, congested junction — is **1.14 ms at p99, 5.7 % of the tick
budget**. The lateral owner is an order of magnitude cheaper again. The named budget is met
with large headroom for motion realization, scripts and rendering, which run outside this
measurement.

## Soak

The 60-minute mixed churn soak (180,000 ticks; junction, population, service and maneuver
owners) passes in ~40 s wall, with no unresolved claim, berth, duty or population entry, no
monotonic pending-queue growth beyond the bounded `QUEUE_MAX`, and no unexplained persistent
stall. The short version runs in the normal test suite.

## Integration cost (engine, separate from rendering)

The engine already separates the domain tick from rendering in `core::Traffic::tick` via
`tick_split` (`[lanes, plan, ai]`), reported by `OMSI_PROFILE` as `traffic.tick`,
`traffic.tick.lanes`, `traffic.tick.plan`, `traffic.tick.ai`, separate from the `render`/`gpu`
stages. Use it for a representative real-map run. A full 60-minute real-map run is
wall-clock-locked (~1× real time) and is left to Stage 10's session testing; this report's
release evidence is the accelerated synthetic soak above.

## Reproduce

```text
cargo bench -p traffic --bench domain
BENCH_TICKS=200 cargo bench -p traffic --bench domain
cargo test -p traffic --test s9_soak
cargo test -p traffic --test s9_soak -- --ignored
```
