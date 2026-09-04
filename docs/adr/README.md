# Architecture Decision Records

This directory records non-obvious architectural and process decisions for
`fml`, backfilled where a real decision was made but never written down durably.
Convention: one file per decision, numbered sequentially —
`docs/adr/NNNN-short-title.md`, zero-padded to 4 digits, never reused or
renumbered even if a later decision supersedes an earlier one (mark the
superseded file instead of deleting it).

Each ADR states the decision, the real context/rationale that produced it
(citing the issue/PR where it was actually decided, not invented after the
fact), and cross-links rather than duplicates any doc that already explains the
mechanism in depth (e.g. [style-guide.md](../style-guide.md)).

## Index

| #                                                         | Title                                                                         | Status   |
| --------------------------------------------------------- | ----------------------------------------------------------------------------- | -------- |
| [0001](0001-arc-shared-execution-context.md)              | Arc-shared `ExecutionContext` fields                                          | Accepted |
| [0002](0002-tokei-as-library-not-subprocess.md)           | `tokei` as a library dependency, not a subprocess, for a future `fml stat`    | Proposed |
| [0003](0003-two-tag-release-versioning.md)                | Independent `v*`/`s*` release tags instead of a single unified tag            | Accepted |
| [0004](0004-status-label-tracking-not-shared-document.md) | Per-issue `status:*` labels instead of a shared hand-edited tracking document | Accepted |
| [0005](0005-extra-args-exit-code-contracts.md)            | Document `extra_args` exit-code hazards; guard only flags `fml` passes itself | Accepted |
