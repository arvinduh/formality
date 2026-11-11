# Releasing

Releases are built and published by
[cargo-dist](https://opensource.axo.dev/cargo-dist/) from a bare `x.y.z` tag.
GitHub Releases are the only channel: no crates.io publish and no changelog
file; GitHub's `--generate-notes` writes the notes from the merged PRs.

1. Bump `version` in the root `Cargo.toml` in its own `chore(release)` PR,
   reading the bump off the Conventional Commits since the last tag.
2. Once it merges, tag that commit and push the tag:

   ```sh
   git tag 0.1.0 && git push origin 0.1.0
   ```

3. `.github/workflows/release.yml` runs the tests on every target in
   `[workspace.metadata.dist]`, then builds them, uploads the archives and
   installers, and creates the release. A failing test stops it before anything
   is published; fix it on `main` and tag the fix. Check that `fml update` on
   the previous release finds it.

## Regenerating the release workflow

`allow-dirty = ["ci"]` keeps `dist generate` from overwriting `release.yml`.
After a dist bump, regenerate it and re-apply the four `LOCAL EDIT` changes: the
two installer checksum steps, `fetch-depth: 0` on the `host` checkout, and the
`gh release create --generate-notes` step.
