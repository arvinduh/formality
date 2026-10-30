#!/usr/bin/env bash
# Installs the pinned tools fml wraps, for CI. Bump a pin here to upgrade.
set -euo pipefail

npm install --global \
  prettier@3.9.10 \
  @biomejs/biome@2.6.4 \
  @taplo/cli@0.7.0 \
  markdownlint-cli2@0.24.0

for package in ruff==0.16.4 yamllint==1.38.0 shellcheck-py==0.11.0.1 \
  clang-format==23.1.3 clang-tidy==23.1.4; do
  pipx install "$package"
done

go install mvdan.cc/sh/v3/cmd/shfmt@v3.14.1
if [[ -n "${GITHUB_PATH:-}" ]]; then
  go env GOPATH | sed 's|$|/bin|' >>"$GITHUB_PATH"
fi
