<!--
Before requesting review:

- Keep this PR focused on one coherent change.
- User-visible changes need a `.changes/<pr>.<category>.md` fragment. Use
  `skip-changelog` only when reviewers agree that no release note is appropriate.
- If this PR changes behavior that is meant to match OMSI 2.2.032, add a
  `## Reference behavior` section describing the exact OMSI result and how it was verified.
- Run the relevant checks and document what you actually tested below.
- Review and understand every submitted change, including AI-assisted code.
- Never include proprietary OMSI code or assets.

The rendered PR description should be useful to a reviewer, not a completed form.
Do not add checklist items merely to restate repository policy.
-->

## Summary

<!--
What changed, and why is this the right change?

Prefer a few concrete sentences or bullets. Describe the result rather than walking through
individual commits or files. Link related issues with `Fixes #123` when appropriate.
-->

## Impact

<!--
What changes for players, content authors, developers, or maintainers?

Call out observable behavior, compatibility implications, or workflow/API changes.
For a purely internal change, say `None (internal)` and briefly name the affected subsystem.
-->

<!--
For parity or compatibility work, insert a section here:

## Reference behavior

Describe what OMSI 2.2.032 does under the same inputs and conditions, how you verified it,
and the map/vehicle/content/settings used where relevant. Prefer reproducible evidence over
assumptions or recollection.
-->

## Validation

<!--
What did you actually verify?

Include exact automated checks and meaningful manual testing. For gameplay or rendering
changes, name the platform, map, vehicle, content, and scenario when that context matters.
Do not claim checks you did not run. If something important was not tested, state that here.
-->

<!--
Optional: add `## Reviewer notes` only when it adds review value.

Use it for non-obvious design decisions, intentional limitations, migration concerns, risky
areas, or specific parts of the diff that deserve extra scrutiny. Do not repeat the Summary.
-->
