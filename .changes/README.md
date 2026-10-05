# Changelog fragments

Each notable PR adds one short release-note fragment:

```text
.changes/<pr-number>.<category>.md
```

Categories: `parity`, `fix`, `feature`, `performance`, `breaking`, `docs`, `internal`.

The number must match the PR. Release tooling groups fragments by category and adds a
linked `#<pr>` reference automatically.

Write one concise paragraph describing the result, not the implementation process. No
headings, lists, jokes or PR-template prose. Use multiple category files for one PR when
needed.

Example:

```text
.changes/412.parity.md
```

```markdown
Fixed keyboard steering return behaviour to match OMSI 2.
```

Use the reviewer-approved `skip-changelog` label for trivial changes that should not appear
in release notes.

Nightlies show fragments changed since the previous nightly. Stable release
preparation compiles all pending fragments into `CHANGELOG.md` and removes them.
