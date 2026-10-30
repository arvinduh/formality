# Releasing

Releases are built and published by
[cargo-dist](https://opensource.axo.dev/cargo-dist/) from a bare `x.y.z` tag.
GitHub Releases are the only channel: no crates.io publish and no changelog
file; GitHub's `--generate-notes` writes the notes from the merged PRs.

1. Bump `version` in the root `Cargo.toml` in its own `chore(release)` PR,
   reading the bump off the Conventional Commits since the last tag.
2. Once it merges and CI on `main` is green, tag that commit and push the tag:

   ```sh
   git tag 0.5.0 && git push origin 0.5.0
   ```

3. `.github/workflows/release.yml` builds every target in
   `[workspace.metadata.dist]`, uploads the archives and installers, and
   creates the release. Check that `fml update` on the previous release finds
   it.

## Regenerating the release workflow

`allow-dirty = ["ci"]` keeps `dist generate` from overwriting
`release.yml`. After a dist bump, regenerate it and re-apply the three
`LOCAL EDIT` changes: the bare tag glob, `fetch-depth: 0` on the `host`
checkout, and the `gh release create --generate-notes` step.
