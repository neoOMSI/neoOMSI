# Traffic AI refactor — Stage 0 preparation

Stage 0 of the [Traffic AI refactor plan](../TRAFFIC_AI_REFACTOR_PLAN.md) is lightweight
preparation. It produces the starting backlog, an integration/test inventory, the first
synthetic scenario specifications, and the decision/blocked-reason capture specification
required at the Stage 1 seam. It changes no runtime behaviour.

- Source revision: `35a460a54e51ed28f9fb081b4adf16c8c73993a8` (the plan's `35a460a`).
- Reference baseline: OMSI 2.2.032 (see [Compatibility](../COMPATIBILITY.md)).
- Reproduction of the original queue screenshot is **not** required for Stage 0. Missing
  reproduction data is recorded as unknown and is never promoted into a claimed diagnosis.

## Documents

| Document | Purpose | Plan deliverable |
| --- | --- | --- |
| [BACKLOG.md](BACKLOG.md) | Behaviour goals and source/reference findings adopted as the initial backlog | "adopt the behavior goals, current source findings, and OMSI coverage matrix as the initial backlog" |
| [TEST_INVENTORY.md](TEST_INVENTORY.md) | Source revision, existing tests and callers, the `dt` seam, existing tooling | "record the source revision and inventory existing tests/callers" |
| [SCENARIOS.md](SCENARIOS.md) | First three synthetic scenario specifications and provisional measurement targets | "define the first synthetic scenario specifications ... set initial behavior targets as provisional" |
| [TRACE_SCHEMA.md](TRACE_SCHEMA.md) | Field-level decision/blocked-reason capture and the first extraction boundary | "define the minimum decision/blocked-reason capture to add at the Stage 1 seam" |

## Exit gate

Stage 1 may start once the first extraction boundary and the Stage 0 scenarios are
specified. Stage 1 does not require a live queue reproduction, an OMSI recording session,
completed replay tooling, or a benchmark suite. Those become part of the continuous
validation track that runs alongside Stages 1–9.

## Scope boundaries

Stage 0 is documentation only. Creating `crates/traffic`, the scenario runner, fixed
ticking, and any change to `Traffic`, `BusService`, or `schedule` belong to Stage 1 and
later stages. Where this folder states a fact that is not proven by source or reference
material, it is labelled `unknown` or `uncertain` rather than asserted.
