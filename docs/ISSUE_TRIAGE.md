# Issue triage

The neoOMSI issue tracker serves as an **active engineering backlog**, not an unmanaged archive of ideas or unresolved reports.

An open issue represents an actionable defect, verified bug, or planned task actively being addressed or scheduled by maintainers.

## Triage philosophy

High-volume repositories quickly become overwhelmed by duplicate, incomplete, or untriaged reports. Maintainers prioritize engineering time on actionable, reproducible issues.

Closing an issue does not mean the report was invalid; it simply means there is currently insufficient reproducible information or priority to warrant active tracking. Issues can be reopened or resubmitted once reproduction steps or logs become available.

## Classification conventions

### Native issue types

GitHub native issue types establish the primary classification:

- **Bug** – Unexpected defect, crash, visual glitch, regression, or discrepancy with OMSI 2.2.032.
- **Feature** – Intentional enhancement, user request, or capability going beyond OMSI 2.
- **Task** – Specific engineering task, refactoring, CI/infrastructure, or documentation work.

### Status labels

- `status: untriaged` – New issue, awaiting review.
- `status: needs-info` – Plausible report, but missing steps, logs, or reproduction details.
- `status: verified` – Successfully reproduced or substantiated by clear diagnostic evidence.
- `status: needs-investigation` – Valid problem, but the root cause or correct subsystem fix is unclear.
- `status: accepted` – Proposal or task reviewed by maintainers and accepted as in scope for neoOMSI. Acceptance does not imply a specific release or implementation date.

### Scope labels

- `scope: parity` – Discrepancy with OMSI 2.2.032 behavior.
- `scope: extension` – Feature or enhancement intentionally going beyond OMSI 2.
- `scope: internal` – Build infrastructure, CI, tooling, or refactoring.
- `scope: mod-specific` – Behavior isolated to a third-party add-on.

### Protection label

- `triage: protected` – Explicitly retained through automated maintenance sweeps during ongoing research or design.

### Planning fields (optional)

Maintainers may optionally assign planning fields to triaged issues:

- **Priority** (`Urgent`, `High`, `Medium`, `Low`) – Urgency relative to engine stability and milestone goals.
- **Effort** (`High`, `Medium`, `Low`) – Rough estimate of implementation complexity.
- **Dates** – Generally left blank unless an issue is tied to a fixed release timeline.

## Triage workflow

Maintainers review incoming issues regularly:

1. **Check for completeness:** Does the report specify the neoOMSI build, OMSI 2 path configuration, reproduction steps, and system details?
2. **OMSI 2 baseline comparison:** What does OMSI 2.2.032 do under the exact same inputs and conditions?
3. **Classify:**
   - Set the native issue type (**Bug**, **Feature**, or **Task**).
   - **Actionable & verified:** Assign `status: verified` and appropriate scope.
   - **Accepted feature / extension:** Assign `status: accepted` and the appropriate scope.
   - Optionally set **Priority** and **Effort** to assist in backlog scheduling.
   - **Missing information:** Request specifics and tag `status: needs-info`.
   - **Duplicate:** Reference the canonical issue and close.
   - **Out of scope / pure extension:** Label `scope: extension` and defer or close if not aligned with current phase goals.
   - **Incomplete / non-reproducible:** Close with an explanation of what details are required to re-evaluate.

## Automated triage

New or reopened issues without a status label receive `status: untriaged` automatically.

Issues labeled `status: needs-info` are closed after 14 days without activity unless they also carry `triage: protected`. Any new activity resets the timer. If the requested information becomes available later, the issue may be reopened or resubmitted with the missing details.

## Mod-specific reports

When an issue occurs only with a specific add-on map or vehicle, evaluate whether the content exposes a general OMSI rule that neoOMSI implements incorrectly (see [Compatibility policy](COMPATIBILITY.md#mod-specific-issues)):

- If the issue is due to broken scripts or syntax errors that also fail in OMSI 2, it is outside project scope.
- If OMSI 2 accommodates the mod via a discoverable fallback, document the general engine rule and label `scope: parity`.


## Feature requests

During the parity-focused phase, features that deliberately diverge from or extend OMSI 2 are secondary to core engine parity. Feature requests may be tagged `scope: extension` and closed or held as discussions to keep the issue tracker focused on engine stabilization.
