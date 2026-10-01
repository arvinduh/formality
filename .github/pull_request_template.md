<!-- Title: `<type>(<scope>): <summary>`, the eventual squash commit subject.
Branch: `<type>/<slug>`, e.g. `fix/spawn`, `feat/diagnostics`. -->

Fixes #

## Summary

<!-- What changed and why. -->

## Gate

- [ ] `cargo test` (full suite; `--lib` alone skips `tests/`)
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo run -q -- fmt`

## Notes for reviewers

<!-- The one claim a reviewer should verify independently, if any. -->
