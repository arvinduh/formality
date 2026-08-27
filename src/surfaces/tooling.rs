//! Tool-binary discovery and installation: the `InstallMethod` preference
//! chains for each supported CLI tool, binary-on-PATH detection, and
//! Windows-aware `Command` construction.

use super::{SurfaceResult, SurfaceStatus};
use crate::engine::version::Version;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// A package-manager-level way to install a CLI tool: knows how to detect
/// its own availability and how to build the concrete installer command.
/// Each tool below declares an ordered slice of these (prebuilt binary
/// managers first, `cargo install --locked` source compilation as the
/// fallback) instead of duplicating the "is X available?" cascade per tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
  /// `cargo binstall <package>`. Requires `cargo-binstall` on PATH. `package`
  /// may carry a pinned version via cargo's `name@version` syntax (e.g.
  /// `"taplo-cli@0.10.0"`) — see the pinned-versions note above the chain
  /// constants below.
  CargoBinstall(&'static str),
  /// `npm install -g <package>`. Requires `npm` on PATH. `package` may carry
  /// a pinned version via npm's `name@version` syntax (e.g.
  /// `"prettier@3.9.6"`, or `"@scope/name@version"` for scoped packages) —
  /// see the pinned-versions note above the chain constants below.
  Npm(&'static str),
  /// `pnpm add -g <package>`. Requires `pnpm` on PATH. Same `name@version`
  /// pinning convention as [`InstallMethod::Npm`].
  Pnpm(&'static str),
  /// `yarn global add <package>`. Requires `yarn` on PATH. Same
  /// `name@version` pinning convention as [`InstallMethod::Npm`].
  Yarn(&'static str),
  /// `bun add -g <package>`. Requires `bun` on PATH. Same `name@version`
  /// pinning convention as [`InstallMethod::Npm`].
  Bun(&'static str),
  /// `uv tool install <package>`. Requires `uv` on PATH. `package` may carry
  /// a pinned version via PEP 440's `name==version` syntax (e.g.
  /// `"ruff==0.16.4"`).
  Uv(&'static str),
  /// `pipx install <package>`. Requires `pipx` on PATH. Same `name==version`
  /// pinning convention as [`InstallMethod::Uv`].
  Pipx(&'static str),
  /// `pip install --user <package>`. Requires `pip` on PATH. Same
  /// `name==version` pinning convention as [`InstallMethod::Uv`].
  Pip(&'static str),
  /// `pip3 install --user <package>`. Requires `pip3` on PATH. Same
  /// `name==version` pinning convention as [`InstallMethod::Uv`].
  Pip3(&'static str),
  /// `apt-get install -y <package>`. Requires `apt-get` on PATH.
  Apt(&'static str),
  /// `brew install <package>`. Requires `brew` on PATH.
  Brew(&'static str),
  /// `scoop install <package>`. Requires `scoop` on PATH.
  Scoop(&'static str),
  /// winget resolves the package by fuzzy name/id match.
  WingetName(&'static str),
  /// winget resolves the package via `--id=<id> -e`, an exact,
  /// unambiguous match.
  WingetId(&'static str),
  /// `cargo install <package>`, optionally with `--locked`. Requires `cargo`
  /// on PATH. `package` may carry a pinned version via cargo's
  /// `name@version` syntax (e.g. `"taplo-cli@0.10.0"`); `locked` only pins
  /// *that release's* dependency graph, not the release itself.
  Cargo {
    /// The crate name to install, optionally as `name@version`.
    package: &'static str,
    /// Whether to pass `--locked` to pin dependency versions.
    locked: bool,
  },
  /// `rustup component add <component>`. Requires `rustup` on PATH.
  Rustup(&'static str),
  /// `go install <package>`. Requires the Go toolchain (`go`) on PATH.
  /// `package` must include an explicit `@version` (or `@latest`) suffix —
  /// unlike the other variants this one never appends one implicitly, so a
  /// pinned version (e.g. `"...@v0.49.0"`) is exactly what gets requested.
  GoInstall(&'static str),
}

impl InstallMethod {
  /// Returns whether this install method's underlying package manager is
  /// currently available on the system `PATH`.
  #[must_use]
  pub fn is_available(&self) -> bool {
    match self {
      InstallMethod::CargoBinstall(_) => has_cargo_binstall(),
      InstallMethod::Npm(_) => check_binary_exists("npm"),
      InstallMethod::Pnpm(_) => check_binary_exists("pnpm"),
      InstallMethod::Yarn(_) => check_binary_exists("yarn"),
      InstallMethod::Bun(_) => check_binary_exists("bun"),
      InstallMethod::Uv(_) => check_binary_exists("uv"),
      InstallMethod::Pipx(_) => check_binary_exists("pipx"),
      InstallMethod::Pip(_) => check_binary_exists("pip"),
      InstallMethod::Pip3(_) => check_binary_exists("pip3"),
      InstallMethod::Apt(_) => check_binary_exists("apt-get"),
      InstallMethod::Brew(_) => check_binary_exists("brew"),
      InstallMethod::Scoop(_) => check_binary_exists("scoop"),
      InstallMethod::WingetName(_) | InstallMethod::WingetId(_) => {
        check_binary_exists("winget")
      }
      InstallMethod::Cargo { .. } => check_binary_exists("cargo"),
      InstallMethod::Rustup(_) => check_binary_exists("rustup"),
      InstallMethod::GoInstall(_) => check_binary_exists("go"),
    }
  }

  /// Builds the executable command tuple `(program, args)` to execute this
  /// installation method.
  #[must_use]
  pub fn command(&self) -> (String, Vec<String>) {
    fn strs(v: &[&str]) -> Vec<String> {
      v.iter().map(std::string::ToString::to_string).collect()
    }
    match self {
      InstallMethod::CargoBinstall(pkg) => {
        ("cargo".to_string(), strs(&["binstall", "-y", pkg]))
      }
      InstallMethod::Npm(pkg) => {
        ("npm".to_string(), strs(&["install", "-g", pkg]))
      }
      InstallMethod::Pnpm(pkg) => {
        ("pnpm".to_string(), strs(&["add", "-g", pkg]))
      }
      InstallMethod::Yarn(pkg) => {
        ("yarn".to_string(), strs(&["global", "add", pkg]))
      }
      InstallMethod::Bun(pkg) => ("bun".to_string(), strs(&["add", "-g", pkg])),
      InstallMethod::Uv(pkg) => {
        ("uv".to_string(), strs(&["tool", "install", pkg]))
      }
      InstallMethod::Pipx(pkg) => ("pipx".to_string(), strs(&["install", pkg])),
      InstallMethod::Pip(pkg) => ("pip".to_string(), strs(&["install", pkg])),
      InstallMethod::Pip3(pkg) => ("pip3".to_string(), strs(&["install", pkg])),
      InstallMethod::Apt(pkg) => {
        if check_binary_exists("sudo") {
          ("sudo".to_string(), strs(&["apt-get", "install", "-y", pkg]))
        } else {
          ("apt-get".to_string(), strs(&["install", "-y", pkg]))
        }
      }
      InstallMethod::Brew(pkg) => ("brew".to_string(), strs(&["install", pkg])),
      InstallMethod::Scoop(pkg) => {
        ("scoop".to_string(), strs(&["install", pkg]))
      }
      InstallMethod::WingetName(pkg) => (
        "winget".to_string(),
        strs(&[
          "install",
          pkg,
          "--accept-source-agreements",
          "--accept-package-agreements",
        ]),
      ),
      InstallMethod::WingetId(id) => (
        "winget".to_string(),
        vec![
          "install".to_string(),
          format!("--id={id}"),
          "-e".to_string(),
          "--accept-source-agreements".to_string(),
          "--accept-package-agreements".to_string(),
        ],
      ),
      InstallMethod::Cargo { package, locked } => {
        let mut args = vec!["install".to_string(), package.to_string()];
        if *locked {
          args.push("--locked".to_string());
        }
        ("cargo".to_string(), args)
      }
      InstallMethod::Rustup(component) => {
        ("rustup".to_string(), strs(&["component", "add", component]))
      }
      InstallMethod::GoInstall(pkg) => (
        "go".to_string(),
        vec!["install".to_string(), pkg.to_string()],
      ),
    }
  }

  /// The exact version this specific install method pins to, parsed from
  /// its package spec, when that spec embeds one. Covers every
  /// pinning-syntax family declared on the variants above: npm-family
  /// `name@version` (scoped `@scope/name@version` too — the split is on the
  /// *last* `@`, so the scope's leading `@` is untouched), pip-family
  /// `name==version`, and cargo/cargo-binstall/`go install`'s `name@version`.
  /// System package managers (apt/brew/scoop/winget) and
  /// `rustup component add` never carry an inline version — see the
  /// "Pinned tool versions" note below — so those return `None`, same as a
  /// spec whose trailing segment doesn't parse as a version at all.
  #[must_use]
  pub fn pinned_version(&self) -> Option<Version> {
    match self {
      InstallMethod::CargoBinstall(pkg)
      | InstallMethod::Npm(pkg)
      | InstallMethod::Pnpm(pkg)
      | InstallMethod::Yarn(pkg)
      | InstallMethod::Bun(pkg)
      | InstallMethod::GoInstall(pkg) => {
        pkg.rsplit_once('@').and_then(|(_, v)| Version::parse(v))
      }
      InstallMethod::Uv(pkg)
      | InstallMethod::Pipx(pkg)
      | InstallMethod::Pip(pkg)
      | InstallMethod::Pip3(pkg) => {
        pkg.rsplit_once("==").and_then(|(_, v)| Version::parse(v))
      }
      InstallMethod::Cargo { package, .. } => package
        .rsplit_once('@')
        .and_then(|(_, v)| Version::parse(v)),
      InstallMethod::Apt(_)
      | InstallMethod::Brew(_)
      | InstallMethod::Scoop(_)
      | InstallMethod::WingetName(_)
      | InstallMethod::WingetId(_)
      | InstallMethod::Rustup(_) => None,
    }
  }

  /// Returns the user-facing name of this installer (e.g. `"cargo-binstall"`,
  /// `"npm"`, `"brew"`).
  #[must_use]
  pub fn installer_name(&self) -> &'static str {
    match self {
      InstallMethod::CargoBinstall(_) => "cargo-binstall",
      InstallMethod::Npm(_) => "npm",
      InstallMethod::Pnpm(_) => "pnpm",
      InstallMethod::Yarn(_) => "yarn",
      InstallMethod::Bun(_) => "bun",
      InstallMethod::Uv(_) => "uv",
      InstallMethod::Pipx(_) => "pipx",
      InstallMethod::Pip(_) => "pip",
      InstallMethod::Pip3(_) => "pip3",
      InstallMethod::Apt(_) => "apt",
      InstallMethod::Brew(_) => "brew",
      InstallMethod::Scoop(_) => "scoop",
      InstallMethod::WingetName(_) | InstallMethod::WingetId(_) => "winget",
      InstallMethod::Cargo { .. } => "cargo",
      InstallMethod::Rustup(_) => "rustup",
      InstallMethod::GoInstall(_) => "go",
    }
  }
}

// --- Pinned tool versions -------------------------------------------------
//
// `fml install` used to ask package managers for these tools with no
// version at all (`npm install -g prettier`), so the exact bits it pulled
// down were whatever that registry happened to resolve as "latest" *at
// install time* — for npm/PyPI/crates.io that can, and does, change between
// two otherwise-identical CI runs. That made CI nondeterministic: a commit
// that was green today could turn red tomorrow with zero code changes,
// purely because an upstream tool shipped a release that formats or lints
// something differently (concretely reproduced in #191: `docs/table-spec.md`
// passed locally against prettier 3.8.1 but failed on a CI runner that
// resolved 3.9.6).
//
// The fix is the same idea `rust-toolchain.toml` already applies to the Rust
// toolchain and `dtolnay/rust-toolchain@1.97.1` applies to the GitHub Action
// itself: pin to an exact version instead of floating on "latest". Below,
// every package-manager-resolved entry (npm/pnpm/yarn/bun, uv/pipx/pip,
// cargo/cargo-binstall, `go install`) embeds an explicit version directly in
// the package specifier passed to that manager — `"prettier@3.9.6"` for
// npm-family managers, `"ruff==0.16.4"` for pip-family ones, `"pkg@version"`
// for cargo/cargo-binstall/go install. This file is the single place that
// version lives: CI workflows never re-declare it in YAML, they just run
// `fml install` and get whatever's pinned here.
//
// System package managers (apt/brew/scoop/winget) are deliberately left
// unpinned: their inline version syntax isn't uniform across managers, and
// they resolve against a distro/tap snapshot rather than a single global
// "latest" tag, so they drift far less than npm/PyPI/crates.io do. `fml
// doctor`'s MSTV floor (`engine::version::mstv`) still catches a
// system-package install that's too old to work at all.
//
// Bumping a pin is a deliberate action, same as bumping
// `rust-toolchain.toml`: update the literal(s) here, re-run the presubmit
// dogfooding (`fmt`/`lint`/`doctor --all`/`install`) to confirm the new
// version doesn't reformat/re-lint this repo's own tree differently, then
// commit.

const TAPLO_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("taplo-cli@0.10.0"),
  InstallMethod::Npm("@taplo/cli@0.7.0"),
  InstallMethod::Pnpm("@taplo/cli@0.7.0"),
  InstallMethod::Yarn("@taplo/cli@0.7.0"),
  InstallMethod::Bun("@taplo/cli@0.7.0"),
  InstallMethod::Brew("taplo"),
  InstallMethod::Scoop("taplo"),
  InstallMethod::WingetId("tamasfe.taplo"),
  InstallMethod::Cargo {
    package: "taplo-cli@0.10.0",
    locked: true,
  },
];

const TYPSTYLE_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("typstyle@0.15.1"),
  InstallMethod::Brew("typstyle"),
  InstallMethod::Scoop("typstyle"),
  InstallMethod::WingetName("typstyle"),
  InstallMethod::Cargo {
    package: "typstyle@0.15.1",
    locked: true,
  },
];

// `Npm("@myriaddreamin/tinymist")` used to live here but never corresponded
// to a real published package (404 on npm; the whole `@myriaddreamin` scope
// only publishes Typst.ts WASM bindings, not this CLI, and the unscoped
// `tinymist` package is likewise a WASM analyzer module) -- confirmed by
// direct registry lookup while fixing #195. Dropped rather than pinned: no
// npm distribution of this CLI exists to pin a version of. cargo-binstall
// (first below) and the plain `cargo install` fallback already cover it.
const TINYMIST_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("tinymist@0.15.2"),
  InstallMethod::Brew("tinymist"),
  InstallMethod::Scoop("tinymist"),
  InstallMethod::WingetName("Myriad-Dreamin.tinymist"),
  InstallMethod::Cargo {
    package: "tinymist@0.15.2",
    locked: true,
  },
];

const RUFF_CHAIN: &[InstallMethod] = &[
  InstallMethod::Uv("ruff==0.16.4"),
  InstallMethod::Pipx("ruff==0.16.4"),
  InstallMethod::Pip("ruff==0.16.4"),
  InstallMethod::Pip3("ruff==0.16.4"),
  InstallMethod::Brew("ruff"),
  InstallMethod::CargoBinstall("ruff@0.16.4"),
  InstallMethod::Scoop("ruff"),
  InstallMethod::WingetName("Astral-sh.ruff"),
  InstallMethod::Cargo {
    package: "ruff@0.16.4",
    locked: true,
  },
];

const PRETTIER_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("prettier@3.9.6"),
  InstallMethod::Pnpm("prettier@3.9.6"),
  InstallMethod::Yarn("prettier@3.9.6"),
  InstallMethod::Bun("prettier@3.9.6"),
  InstallMethod::Brew("prettier"),
  InstallMethod::Scoop("prettier"),
  InstallMethod::WingetName("Prettier.Prettier"),
];

const BIOME_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("@biomejs/biome@2.5.10"),
  InstallMethod::Pnpm("@biomejs/biome@2.5.10"),
  InstallMethod::Yarn("@biomejs/biome@2.5.10"),
  InstallMethod::Bun("@biomejs/biome@2.5.10"),
  InstallMethod::Brew("biome"),
  InstallMethod::Scoop("biome"),
];

const MARKDOWNLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("markdownlint-cli2@0.23.2"),
  InstallMethod::Pnpm("markdownlint-cli2@0.23.2"),
  InstallMethod::Yarn("markdownlint-cli2@0.23.2"),
  InstallMethod::Bun("markdownlint-cli2@0.23.2"),
  InstallMethod::Brew("markdownlint-cli2"),
  InstallMethod::Scoop("markdownlint-cli2"),
];

const YAMLLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Uv("yamllint==1.38.0"),
  InstallMethod::Pipx("yamllint==1.38.0"),
  InstallMethod::Pip("yamllint==1.38.0"),
  InstallMethod::Pip3("yamllint==1.38.0"),
  InstallMethod::Apt("yamllint"),
  InstallMethod::Brew("yamllint"),
  InstallMethod::Scoop("yamllint"),
  InstallMethod::WingetName("yamllint"),
];

const CLANG_FORMAT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Apt("clang-format"),
  InstallMethod::Brew("clang-format"),
  InstallMethod::Pipx("clang-format==22.1.8"),
  InstallMethod::Pip("clang-format==22.1.8"),
  InstallMethod::Pip3("clang-format==22.1.8"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

const CLANG_TIDY_CHAIN: &[InstallMethod] = &[
  InstallMethod::Apt("clang-tidy"),
  InstallMethod::Brew("llvm"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

// Note: `Npm("google-java-format")` below IS real (the
// `invertase/nodejs-google-java-format` npm wrapper, pinned) and is pinned
// like any other npm entry. A `Pipx("google-java-format")` entry used to sit
// below it but never corresponded to a real PyPI distribution -- confirmed
// by direct registry lookup while fixing #195 (no `google-java-format` on
// PyPI; the closest name match, `gjf`, is an unrelated GeoJSON-fixing tool).
// Dropped rather than pinned: the working `Npm` entry above already covers
// this tool, and no PyPI wrapper exists to pin a version of.
const GOOGLE_JAVA_FORMAT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("google-java-format"),
  InstallMethod::Npm("google-java-format@2.3.0"),
];

// An `Npm("checkstyle")` entry used to sit here but never corresponded to a
// real published package (404 on npm, confirmed by direct registry lookup
// while fixing #195; searching npm for "checkstyle" turns up only adapters
// that consume some *other* tool's output and reformat it as Checkstyle XML,
// not a wrapper that installs the actual `checkstyle` Java tool). Dropped
// rather than pinned: no npm distribution of this tool exists, and the
// Brew/Apt entries above already cover the platforms that have a real
// install path.
const CHECKSTYLE_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("checkstyle"),
  InstallMethod::Apt("checkstyle"),
];

const RUSTFMT_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("rustfmt")];
const CLIPPY_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("clippy")];

const GOIMPORTS_CHAIN: &[InstallMethod] = &[InstallMethod::GoInstall(
  "golang.org/x/tools/cmd/goimports@v0.49.0",
)];

const GOLANGCI_LINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("golangci-lint"),
  InstallMethod::Scoop("golangci-lint"),
  InstallMethod::GoInstall(
    "github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.13.1",
  ),
];

// ktlint ships as a prebuilt executable jar; there is no cargo fallback, so
// the chain is otherwise limited to system package managers (mirrors the
// CLANG_FORMAT_CHAIN / CLANG_TIDY_CHAIN pattern above). The unscoped
// `Npm("ktlint@0.0.5")` this used to point at is an abandoned, single-version
// 2018 package (its npm "version" 0.0.5 has never tracked the real tool's
// version at all) whose preinstall script shells out to `curl` a *hardcoded*
// `shyiko/ktlint` 0.29.0 binary straight from GitHub, bypassing npm's
// registry entirely -- so pinning its npm arg wouldn't have pinned its
// actual behavior, the whole point of the #191/#194 pinning convention.
// Confirmed by extracting the published tarball and reading its
// preinstall.js while fixing #195. Replaced with `@naturalcycles/ktlint`, a
// maintained npm wrapper (23 published versions, tracking upstream) that
// republishes the actual `com.pinterest.ktlint` self-executable jar under
// `resources/ktlint` (verified by extracting the published tarball and
// confirming the `com/pinterest/ktlint/Main.class` entry, and by actually
// installing and running it end to end: `ktlint version` reports the real
// tool's `1.8.0`). Requires a JVM on PATH at run time, same as the
// Brew/Scoop/Apt entries below.
const KTLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("ktlint"),
  InstallMethod::Scoop("ktlint"),
  InstallMethod::Npm("@naturalcycles/ktlint@1.16.1"),
  InstallMethod::Apt("ktlint"),
];

/// One row of the tool-chain registry: the canonical binary name, its
/// ordered installer preference chain, and (if known) the exact version
/// `<binary> --version` is expected to report once installed via that
/// chain's pin.
///
/// `expected_binary_version` is intentionally *not* derived automatically
/// from the chain's package-spec pins — a package-manager's own version
/// number and the underlying binary's self-reported version are two
/// different things that happen to agree for most tools but are **not
/// guaranteed to**, and conflating them was a real, empirically-confirmed
/// bug: `@taplo/cli@0.7.0` is exactly what gets installed, but the `taplo`
/// binary it produces reports `0.9.0`, and cargo-binstall's own
/// `taplo-cli@0.10.0` pin disagrees with the npm pin too — three different
/// numbers for "one" tool. Set this field only when independently confirmed
/// that the binary's `--version` output tracks the pin 1:1; leave it `None`
/// for anything not confirmed, which exactly preserves this file's
/// pre-`[STALE]` behavior (presence/executability + the MSTV floor, no
/// pin-mismatch comparison) rather than risking a false `[STALE]` verdict
/// that would make `fml install` reinstall an already-correct tool forever.
struct ToolChain {
  /// Canonical binary name (see [`install_chain_for`]'s alias resolution).
  binary: &'static str,
  /// Ordered installer preference chain.
  chain: &'static [InstallMethod],
  /// The version `<binary> --version` should report once installed via
  /// this chain's pin, when confirmed to track it 1:1. See the struct doc
  /// above for why this is a hand-confirmed fact, not a derived value.
  expected_binary_version: Option<Version>,
}

/// The tool-chain side-table every tool in the fleet is registered in
/// exactly once. This is what [`install_chain_for`] and
/// [`pinned_version_for`] below look up, and what both
/// `test_tool_info_auto_install_cmd_coverage` and
/// `test_registry_resolved_install_methods_are_version_pinned` iterate —
/// per `docs/style-guide.md`'s tier-2 convention ("walk ... an in-crate
/// side-table", not a hand-copied literal array), a new chain constant only
/// needs adding here to automatically get install-time lookup and test
/// coverage; forgetting a row here is what let the `clang-format` pinning
/// gap through unnoticed in an earlier pass of this same table (previously a
/// hand-maintained `match` plus a hand-copied test array that had already
/// drifted apart from each other).
///
/// `expected_binary_version` status per row, and why:
/// - `Some(...)`: `typstyle`, `tinymist`, `ruff`, `prettier`, `biome`,
///   `markdownlint-cli2`, `yamllint`, `golangci-lint` — each ships its own CLI directly
///   (not a repackaging of some other project's binary) and every
///   registry-resolved pin in its chain agrees on the same version, so the
///   package-manager pin and the binary's self-reported version are the
///   same fact stated twice. `test_expected_binary_version_agrees_with_chain_pins`
///   below is a standing regression guard on that agreement.
/// - `None`, confirmed mismatched: `taplo` (see the struct doc above —
///   directly tested: pinned npm spec `0.7.0`, installed binary reports
///   `0.9.0`, neither matches the cargo-binstall chain's own `0.10.0` pin
///   either), `ktlint` (this file's own long-standing comment above
///   `KTLINT_CHAIN` already documents its `@naturalcycles/ktlint@1.16.1`
///   npm wrapper reporting the real jar's `1.8.0`, an entirely different
///   numbering track).
/// - `None`, suspected mismatched but not independently confirmed:
///   `google-java-format` (npm wrapper around a separately-versioned Java
///   tool, same shape as ktlint's wrapper) and `goimports` (its pin is a Go
///   *module* version tag, not a tool release version — `goimports` has no
///   meaningful `--version` output to compare against at all). Treat these
///   the same as a confirmed mismatch until someone verifies otherwise.
/// - `None`, no registry-resolved pin to compare at all: `clang-tidy`,
///   `checkstyle`, `rustfmt`, `clippy-driver` — every entry in these chains
///   is an unpinned system-package-manager/rustup install.
/// - `None`, unverified even though internally consistent: `clang-format`
///   — its `Pipx`/`Pip`/`Pip3` entries agree on `22.1.8`, and the PyPI
///   `clang-format` wheel plausibly bundles a matching prebuilt binary, but
///   that hasn't been independently confirmed the way taplo/ktlint's
///   mismatches were, and the apt/brew/winget/scoop fallbacks in the same
///   chain resolve against uncontrolled system versions regardless. Flip to
///   `Some(Version::new(22, 1, 8))` once confirmed against a real pip
///   install.
const ALL_CHAINS: &[ToolChain] = &[
  ToolChain {
    binary: "taplo",
    chain: TAPLO_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "typstyle",
    chain: TYPSTYLE_CHAIN,
    expected_binary_version: Some(Version::new(0, 15, 1)),
  },
  ToolChain {
    binary: "tinymist",
    chain: TINYMIST_CHAIN,
    expected_binary_version: Some(Version::new(0, 15, 2)),
  },
  ToolChain {
    binary: "ruff",
    chain: RUFF_CHAIN,
    expected_binary_version: Some(Version::new(0, 16, 4)),
  },
  ToolChain {
    binary: "prettier",
    chain: PRETTIER_CHAIN,
    expected_binary_version: Some(Version::new(3, 9, 6)),
  },
  ToolChain {
    binary: "biome",
    chain: BIOME_CHAIN,
    expected_binary_version: Some(Version::new(2, 5, 10)),
  },
  ToolChain {
    binary: "markdownlint-cli2",
    chain: MARKDOWNLINT_CHAIN,
    expected_binary_version: Some(Version::new(0, 23, 2)),
  },
  ToolChain {
    binary: "yamllint",
    chain: YAMLLINT_CHAIN,
    expected_binary_version: Some(Version::new(1, 38, 0)),
  },
  ToolChain {
    binary: "clang-format",
    chain: CLANG_FORMAT_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "clang-tidy",
    chain: CLANG_TIDY_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "google-java-format",
    chain: GOOGLE_JAVA_FORMAT_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "checkstyle",
    chain: CHECKSTYLE_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "rustfmt",
    chain: RUSTFMT_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "clippy-driver",
    chain: CLIPPY_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "goimports",
    chain: GOIMPORTS_CHAIN,
    expected_binary_version: None,
  },
  ToolChain {
    binary: "golangci-lint",
    chain: GOLANGCI_LINT_CHAIN,
    expected_binary_version: Some(Version::new(2, 13, 1)),
  },
  ToolChain {
    binary: "ktlint",
    chain: KTLINT_CHAIN,
    expected_binary_version: None,
  },
];

/// Resolves `markdownlint`/`clippy` legacy binary-name aliases to their
/// canonical [`ALL_CHAINS`] row name (`markdownlint-cli2`/`clippy-driver`).
/// Shared by [`install_chain_for`] and [`pinned_version_for`] so alias
/// resolution lives in exactly one place.
fn canonical_chain_binary(binary: &str) -> &str {
  match binary {
    "markdownlint" => "markdownlint-cli2",
    "clippy" => "clippy-driver",
    other => other,
  }
}

/// Looks up the ordered installer preference chain for a tool binary name,
/// via [`ALL_CHAINS`] above.
#[must_use]
pub fn install_chain_for(binary: &str) -> Option<&'static [InstallMethod]> {
  let canonical = canonical_chain_binary(binary);
  ALL_CHAINS
    .iter()
    .find(|entry| entry.binary == canonical)
    .map(|entry| entry.chain)
}

/// The version `<binary> --version` is expected to report when it's
/// installed to the pin `fml install` currently uses, per [`ALL_CHAINS`]'s
/// `expected_binary_version` field. Returns `None` — a "no known pin to
/// compare against" result, not an error — when the tool has no registered
/// chain row, or (deliberately, for most rows — see the doc comment above
/// [`ALL_CHAINS`]) when the binary's own version output isn't confirmed to
/// track the package-manager pin 1:1. Callers (`fml doctor`'s `[STALE]`
/// check) must treat `None` as "skip the pin comparison", never crash on
/// it, and never treat it as "definitely up to date" either — it means
/// "unknown", not "yes".
#[must_use]
pub fn pinned_version_for(binary: &str) -> Option<Version> {
  let canonical = canonical_chain_binary(binary);
  ALL_CHAINS
    .iter()
    .find(|entry| entry.binary == canonical)?
    .expected_binary_version
    .clone()
}

/// Returns the first available installer in `binary`'s preference chain,
/// or `None` if no installer in the chain is currently available on PATH.
#[must_use]
pub fn selected_install_method_for(binary: &str) -> Option<InstallMethod> {
  install_chain_for(binary)?
    .iter()
    .copied()
    .find(InstallMethod::is_available)
}

/// Returns the pinned version of the first available installer in `binary`'s
/// preference chain, or `None` if no installer is available or the available
/// installer has no inline pin.
#[must_use]
pub fn selected_pinned_version_for(binary: &str) -> Option<Version> {
  selected_install_method_for(binary).and_then(|m| m.pinned_version())
}

/// Returns the name of the first installer in `binary`'s preference chain that
/// carries an inline version pin matching `expected_binary_version`, if any.
#[must_use]
pub fn pinned_installer_for(binary: &str) -> Option<&'static str> {
  let expected = pinned_version_for(binary)?;
  install_chain_for(binary)?
    .iter()
    .find(|m| m.pinned_version().as_ref() == Some(&expected))
    .map(InstallMethod::installer_name)
}

static BINARY_CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

/// Returns whether `binary` is resolvable on `PATH`, memoized per-process so
/// repeated checks for the same binary don't re-hit the filesystem.
#[must_use]
pub fn check_binary_exists(binary: &str) -> bool {
  let cache = BINARY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
  if let Some(&exists) = guard.get(binary) {
    return exists;
  }
  let exists = which::which(binary).is_ok();
  guard.insert(binary.to_string(), exists);
  exists
}

/// Builds the `SurfaceResult` every surface returns from `format`/`lint` when
/// a required tool binary is not on `PATH`. Every call site previously
/// repeated this same `SurfaceResult { .. status: SurfaceStatus::ToolMissing
/// { .. } .. }` struct literal by hand (~23 instances across the 12 language
/// surfaces) — this is the single place that shape lives now.
#[must_use]
pub fn tool_missing_result(
  surface_name: &'static str,
  start: Instant,
  binary: &str,
  install_hint: &str,
) -> SurfaceResult {
  SurfaceResult {
    surface_name,
    status: SurfaceStatus::ToolMissing {
      binary: binary.to_string(),
      install_hint: install_hint.to_string(),
    },
    duration: start.elapsed(),
  }
}

/// Returns whether `cargo binstall` is usable: both `cargo` and
/// `cargo-binstall` must be on `PATH`. This is a pure `PATH` lookup (via
/// [`check_binary_exists`]/`which`) for both binaries -- it never spawns a
/// child process (e.g. `cargo binstall --version`) to probe availability.
#[must_use]
pub fn has_cargo_binstall() -> bool {
  check_binary_exists("cargo") && check_binary_exists("cargo-binstall")
}

/// Creates a `Command` with proper handling for Windows batch files (.cmd/.bat)
/// such as `npm`, `pnpm`, `yarn`, `npx`, and globally installed node CLIs.
#[must_use]
pub fn create_tool_command(binary: &str) -> std::process::Command {
  #[cfg(windows)]
  {
    if binary == "npm"
      || binary == "pnpm"
      || binary == "yarn"
      || binary == "npx"
    {
      let mut cmd = std::process::Command::new("cmd");
      cmd.arg("/C").arg(binary);
      return cmd;
    }
    if let Ok(path) = which::which(binary) {
      if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && (ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
      {
        let mut cmd = std::process::Command::new("cmd");
        cmd.arg("/C").arg(path);
        return cmd;
      }
      return std::process::Command::new(path);
    }
  }
  std::process::Command::new(binary)
}

/// Runs a tool command, measures execution duration, and translates exit status to a `SurfaceResult`.
pub fn run_tool_command(
  surface_name: &'static str,
  cmd: &mut std::process::Command,
) -> SurfaceResult {
  let start = Instant::now();
  match cmd.output() {
    Ok(output) => {
      let duration = start.elapsed();
      if output.status.success() {
        SurfaceResult {
          surface_name,
          status: SurfaceStatus::Passed,
          duration,
        }
      } else {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if !stdout.is_empty() {
          stdout
        } else if !stderr.is_empty() {
          stderr
        } else {
          format!("Command failed with exit code {}", output.status)
        };
        SurfaceResult {
          surface_name,
          status: SurfaceStatus::ViolationsFound {
            message,
            diff: None,
          },
          duration,
        }
      }
    }
    Err(err) => {
      let duration = start.elapsed();
      SurfaceResult {
        surface_name,
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to execute command: {err}"),
        },
        duration,
      }
    }
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::surfaces::ToolInfo;

  /// Returns whether `pkg` (the final argument passed to a package-manager
  /// install command) carries an explicit version pin, recognizing both the
  /// npm/cargo/go `name@version` convention and the pip-family
  /// `name==version` convention. A single leading `@` (npm scope, e.g.
  /// `@taplo/cli@0.7.0`) is stripped first so it isn't mistaken for the
  /// version separator.
  fn has_version_pin(pkg: &str) -> bool {
    if pkg.contains("==") {
      return true;
    }
    pkg.strip_prefix('@').unwrap_or(pkg).contains('@')
  }

  #[test]
  fn test_has_version_pin_helper() {
    assert!(has_version_pin("prettier@3.9.6"));
    assert!(has_version_pin("@taplo/cli@0.7.0"));
    assert!(has_version_pin("ruff==0.16.4"));
    assert!(has_version_pin("golang.org/x/tools/cmd/goimports@v0.49.0"));
    assert!(!has_version_pin("prettier"));
    assert!(!has_version_pin("@myriaddreamin/tinymist"));
  }

  #[test]
  fn test_pinned_chain_command_shapes() {
    // A handful of concrete, exact assertions (not just "is it pinned") for
    // the tools #191 called out by name, so a future accidental revert back
    // to an unversioned package string fails loudly and specifically.
    let prettier = install_chain_for("prettier").unwrap();
    assert_eq!(
      prettier[0].command(),
      (
        "npm".to_string(),
        vec![
          "install".to_string(),
          "-g".to_string(),
          "prettier@3.9.6".to_string()
        ]
      )
    );

    let taplo = install_chain_for("taplo").unwrap();
    assert_eq!(
      taplo[1].command(),
      (
        "npm".to_string(),
        vec![
          "install".to_string(),
          "-g".to_string(),
          "@taplo/cli@0.7.0".to_string()
        ]
      )
    );

    let ruff = install_chain_for("ruff").unwrap();
    assert_eq!(
      ruff[0].command(),
      (
        "uv".to_string(),
        vec![
          "tool".to_string(),
          "install".to_string(),
          "ruff==0.16.4".to_string()
        ]
      )
    );

    let goimports = install_chain_for("goimports").unwrap();
    assert_eq!(
      goimports[0].command(),
      (
        "go".to_string(),
        vec![
          "install".to_string(),
          "golang.org/x/tools/cmd/goimports@v0.49.0".to_string()
        ]
      )
    );

    let golangci = install_chain_for("golangci-lint").unwrap();
    assert_eq!(golangci.len(), 3);
    assert_eq!(
      golangci[2].command(),
      (
        "go".to_string(),
        vec![
          "install".to_string(),
          "github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.13.1"
            .to_string(),
        ]
      )
    );
  }

  #[test]
  fn test_pinned_version_parses_registry_pin_syntaxes() {
    // npm-family `name@version`.
    assert_eq!(
      InstallMethod::Npm("prettier@3.9.6").pinned_version(),
      Some(Version::new(3, 9, 6))
    );
    // Scoped npm `@scope/name@version` -- must split on the *last* `@`, not
    // the scope's leading one.
    assert_eq!(
      InstallMethod::Npm("@taplo/cli@0.7.0").pinned_version(),
      Some(Version::new(0, 7, 0))
    );
    // pip-family `name==version`.
    assert_eq!(
      InstallMethod::Uv("ruff==0.16.4").pinned_version(),
      Some(Version::new(0, 16, 4))
    );
    // cargo/cargo-binstall `name@version`.
    assert_eq!(
      InstallMethod::CargoBinstall("taplo-cli@0.10.0").pinned_version(),
      Some(Version::new(0, 10, 0))
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "taplo-cli@0.10.0",
        locked: true,
      }
      .pinned_version(),
      Some(Version::new(0, 10, 0))
    );
    // `go install` `name@vVERSION` -- leading `v` must be stripped.
    assert_eq!(
      InstallMethod::GoInstall("golang.org/x/tools/cmd/goimports@v0.49.0")
        .pinned_version(),
      Some(Version::new(0, 49, 0))
    );
    assert_eq!(
      InstallMethod::GoInstall(
        "github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.13.1"
      )
      .pinned_version(),
      Some(Version::new(2, 13, 1))
    );
  }

  #[test]
  fn test_pinned_version_for_golangci_lint() {
    assert_eq!(
      pinned_version_for("golangci-lint"),
      Some(Version::new(2, 13, 1))
    );
  }

  #[test]
  fn test_installer_names() {
    assert_eq!(
      InstallMethod::CargoBinstall("ruff@0.16.4").installer_name(),
      "cargo-binstall"
    );
    assert_eq!(InstallMethod::Npm("prettier@3.9.6").installer_name(), "npm");
    assert_eq!(
      InstallMethod::Pnpm("prettier@3.9.6").installer_name(),
      "pnpm"
    );
    assert_eq!(
      InstallMethod::Yarn("prettier@3.9.6").installer_name(),
      "yarn"
    );
    assert_eq!(InstallMethod::Bun("prettier@3.9.6").installer_name(), "bun");
    assert_eq!(InstallMethod::Uv("ruff==0.16.4").installer_name(), "uv");
    assert_eq!(InstallMethod::Pipx("ruff==0.16.4").installer_name(), "pipx");
    assert_eq!(InstallMethod::Pip("ruff==0.16.4").installer_name(), "pip");
    assert_eq!(InstallMethod::Pip3("ruff==0.16.4").installer_name(), "pip3");
    assert_eq!(InstallMethod::Apt("yamllint").installer_name(), "apt");
    assert_eq!(InstallMethod::Brew("prettier").installer_name(), "brew");
    assert_eq!(InstallMethod::Scoop("prettier").installer_name(), "scoop");
    assert_eq!(
      InstallMethod::WingetName("LLVM.LLVM").installer_name(),
      "winget"
    );
    assert_eq!(
      InstallMethod::WingetId("tamasfe.taplo").installer_name(),
      "winget"
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "ruff@0.16.4",
        locked: true
      }
      .installer_name(),
      "cargo"
    );
    assert_eq!(InstallMethod::Rustup("clippy").installer_name(), "rustup");
    assert_eq!(
      InstallMethod::GoInstall("golang.org/x/tools/cmd/goimports@v0.49.0")
        .installer_name(),
      "go"
    );
  }

  #[test]
  fn test_pinned_installer_for() {
    assert_eq!(pinned_installer_for("prettier"), Some("npm"));
    assert_eq!(pinned_installer_for("ruff"), Some("uv"));
    assert_eq!(pinned_installer_for("typstyle"), Some("cargo-binstall"));
    assert_eq!(pinned_installer_for("tinymist"), Some("cargo-binstall"));
    assert_eq!(pinned_installer_for("biome"), Some("npm"));
    assert_eq!(pinned_installer_for("markdownlint-cli2"), Some("npm"));
    assert_eq!(pinned_installer_for("yamllint"), Some("uv"));
    assert_eq!(pinned_installer_for("golangci-lint"), Some("go"));

    // Tools without expected_binary_version return None
    assert_eq!(pinned_installer_for("taplo"), None);
    assert_eq!(pinned_installer_for("clang-format"), None);
    assert_eq!(pinned_installer_for("clang-tidy"), None);
    assert_eq!(pinned_installer_for("rustfmt"), None);
    assert_eq!(pinned_installer_for("not-a-real-tool"), None);
  }

  #[test]
  fn test_pinned_version_none_for_unpinned_system_managers() {
    // apt/brew/scoop/winget/rustup never carry an inline version -- see the
    // "Pinned tool versions" note above ALL_CHAINS.
    assert_eq!(InstallMethod::Apt("prettier").pinned_version(), None);
    assert_eq!(InstallMethod::Brew("prettier").pinned_version(), None);
    assert_eq!(InstallMethod::Scoop("prettier").pinned_version(), None);
    assert_eq!(
      InstallMethod::WingetName("Prettier.Prettier").pinned_version(),
      None
    );
    assert_eq!(
      InstallMethod::WingetId("tamasfe.taplo").pinned_version(),
      None
    );
    assert_eq!(InstallMethod::Rustup("rustfmt").pinned_version(), None);
  }

  #[test]
  fn test_pinned_version_none_for_unversioned_package_spec() {
    // A package spec with no `@`/`==` at all (e.g. the unpinned npm entries
    // #195 documents as deliberately left bare) must not be misparsed --
    // None, not a crash or a bogus version.
    assert_eq!(
      InstallMethod::Npm("@myriaddreamin/tinymist").pinned_version(),
      None
    );
  }

  #[test]
  fn test_pinned_version_for_unregistered_tool_is_none() {
    // No install chain at all for this binary: fail soft to None, never
    // panic -- this is the "no pinned version configured" edge case #5
    // calls out explicitly.
    assert_eq!(pinned_version_for("totally-unregistered-tool-xyz"), None);
  }

  #[test]
  fn test_pinned_version_for_registered_chain_never_panics() {
    // Smoke test across the whole registry: whether or not any installer in
    // a chain is actually available on this test machine, resolving the pin
    // must never panic -- it's allowed to return None (no installer
    // available / available one is unpinned), just not crash `fml doctor`.
    for entry in ALL_CHAINS {
      let _ = pinned_version_for(entry.binary);
    }
  }

  #[test]
  fn test_go_install_never_appends_implicit_latest() {
    // GoInstall used to always append "@latest" itself, which is exactly
    // the floating-version behavior #191 is about; command() must now pass
    // the package spec through unchanged so the chain constants are the
    // only place a version (pinned or "@latest") gets decided.
    assert_eq!(
      InstallMethod::GoInstall("example.com/tool@v1.2.3").command(),
      (
        "go".to_string(),
        vec!["install".to_string(), "example.com/tool@v1.2.3".to_string()]
      )
    );
  }

  #[test]
  fn test_registry_resolved_install_methods_are_version_pinned() {
    // Every chain entry that resolves against a floating package registry
    // (npm-family, pip-family, cargo/cargo-binstall, `go install`) must
    // request an explicit version -- see the "Pinned tool versions" note
    // above the chain constants. System package managers (apt/brew/scoop/
    // winget) are exempt: they resolve against a distro/tap snapshot rather
    // than a single global "latest" tag, so they drift far less, and their
    // inline version syntax isn't uniform.
    //
    // This test used to also carry an `is_known_dead_package` escape hatch
    // for chain entries that didn't correspond to a real package at all
    // under a given install method (#195) -- pinning a version of a
    // nonexistent package wouldn't make it exist. #195 fixed or dropped
    // every entry that needed it, so the exemption list emptied out; it was
    // removed rather than left as permanently-unused dead code. Re-add the
    // same mechanism if a future audit finds another dead chain entry that
    // can't be fixed immediately.
    //
    // Iterates ALL_CHAINS itself (the same side-table `install_chain_for`
    // and the coverage test below use) rather than a separately maintained
    // binary list, so a new chain constant automatically gets checked here
    // too the moment it's added to ALL_CHAINS.
    for entry in ALL_CHAINS {
      let binary = entry.binary;
      for method in entry.chain {
        let is_registry_resolved = matches!(
          method,
          InstallMethod::Npm(_)
            | InstallMethod::Pnpm(_)
            | InstallMethod::Yarn(_)
            | InstallMethod::Bun(_)
            | InstallMethod::Uv(_)
            | InstallMethod::Pipx(_)
            | InstallMethod::Pip(_)
            | InstallMethod::Pip3(_)
            | InstallMethod::CargoBinstall(_)
            | InstallMethod::Cargo { .. }
            | InstallMethod::GoInstall(_)
        );
        if !is_registry_resolved {
          continue;
        }
        let (_, args) = method.command();
        // `Cargo` appends `--locked` after the package; every other
        // registry-resolved variant's command puts the package last.
        let pkg_arg = if matches!(method, InstallMethod::Cargo { .. }) {
          &args[1]
        } else {
          args
            .last()
            .expect("registry install command should have a package arg")
        };
        assert!(
          has_version_pin(pkg_arg),
          "{binary}: {method:?} resolves package {pkg_arg:?} with no \
           version pin -- fml install would float on \"latest\""
        );
      }
    }
  }

  #[test]
  fn test_expected_binary_version_agrees_with_chain_pins() {
    // Regression guard for the taplo bug this field exists to prevent: a
    // tool that declares `expected_binary_version: Some(v)` is claiming
    // "every registry-resolved pin in my chain agrees with v" -- if a
    // future edit adds a disagreeing pin to that chain (exactly what
    // TAPLO_CHAIN's npm-vs-cargo-binstall pins used to do, silently), this
    // must fail loudly instead of quietly producing false `[STALE]`
    // verdicts again. A row with `expected_binary_version: None` is making
    // no such claim.
    for entry in ALL_CHAINS {
      let Some(expected) = &entry.expected_binary_version else {
        continue;
      };
      for method in entry.chain {
        let Some(pin) = method.pinned_version() else {
          continue;
        };
        assert_eq!(
          &pin, expected,
          "{}: {method:?} pins {pin} but expected_binary_version is {expected} -- \
           either this chain entry drifted, or expected_binary_version needs \
           re-confirming against a real install",
          entry.binary
        );
      }
    }
  }

  #[test]
  fn test_tool_info_auto_install_cmd_coverage() {
    for entry in ALL_CHAINS {
      let info = ToolInfo {
        binary: entry.binary,
        description: "test tool",
        install_hint: "test hint",
        is_required_for_fmt: true,
        is_required_for_lint: true,
      };

      // Ensure get_auto_install_cmd executes without error
      let cmd = info.get_auto_install_cmd();
      if let Some((program, args)) = cmd {
        assert!(!program.is_empty());
        assert!(!args.is_empty());
      }
    }
  }

  #[test]
  fn test_unknown_tool_has_no_install_chain() {
    let info = ToolInfo {
      binary: "not-a-real-tool",
      description: "test tool",
      install_hint: "test hint",
      is_required_for_fmt: false,
      is_required_for_lint: false,
    };
    assert!(info.get_auto_install_cmd().is_none());
  }

  // Command-shape tests below are pure and environment-independent: they
  // exercise InstallMethod::command() directly rather than going through
  // is_available(), so they don't depend on what's actually installed on
  // the machine running the tests.

  #[test]
  fn test_install_method_command_shapes() {
    assert_eq!(
      InstallMethod::CargoBinstall("ruff").command(),
      (
        "cargo".to_string(),
        vec!["binstall".to_string(), "-y".to_string(), "ruff".to_string()]
      )
    );
    assert_eq!(
      InstallMethod::Npm("@taplo/cli").command(),
      (
        "npm".to_string(),
        vec![
          "install".to_string(),
          "-g".to_string(),
          "@taplo/cli".to_string()
        ]
      )
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "typstyle",
        locked: true
      }
      .command(),
      (
        "cargo".to_string(),
        vec![
          "install".to_string(),
          "typstyle".to_string(),
          "--locked".to_string()
        ]
      )
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "some-tool",
        locked: false
      }
      .command(),
      (
        "cargo".to_string(),
        vec!["install".to_string(), "some-tool".to_string()]
      )
    );
    assert_eq!(
      InstallMethod::WingetId("tamasfe.taplo").command(),
      (
        "winget".to_string(),
        vec![
          "install".to_string(),
          "--id=tamasfe.taplo".to_string(),
          "-e".to_string(),
          "--accept-source-agreements".to_string(),
          "--accept-package-agreements".to_string(),
        ]
      )
    );
    assert_eq!(
      InstallMethod::WingetName("LLVM.LLVM").command(),
      (
        "winget".to_string(),
        vec![
          "install".to_string(),
          "LLVM.LLVM".to_string(),
          "--accept-source-agreements".to_string(),
          "--accept-package-agreements".to_string(),
        ]
      )
    );
    assert_eq!(
      InstallMethod::Rustup("clippy").command(),
      (
        "rustup".to_string(),
        vec![
          "component".to_string(),
          "add".to_string(),
          "clippy".to_string()
        ]
      )
    );
    let (prog, args) = InstallMethod::Apt("clang-format").command();
    assert!(prog == "sudo" || prog == "apt-get");
    assert!(args.contains(&"clang-format".to_string()));
  }

  #[test]
  fn test_extra_args_wired_to_command() {
    let mut cmd = create_tool_command("cargo");
    let extra_args = vec!["--verbose".to_string(), "--locked".to_string()];
    cmd.args(&extra_args);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().to_string())
      .collect();
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args.contains(&"--locked".to_string()));
  }

  #[test]
  fn test_check_binary_exists_nonexistent_and_edge_case_inputs() {
    assert!(!check_binary_exists("__nonexistent_binary_xyz_987654321__"));
    assert!(!check_binary_exists(""));
    assert!(!check_binary_exists("   "));
    assert!(!check_binary_exists("\0invalid_null_byte"));
  }

  #[test]
  fn test_tool_missing_result_construction_for_all_surfaces() {
    let surfaces = crate::surfaces::all_surfaces();
    let start = Instant::now();

    for surface in surfaces {
      let missing_res = tool_missing_result(
        surface.name(),
        start,
        "dummy-tool",
        "install via package manager",
      );
      assert_eq!(missing_res.surface_name, surface.name());
      match missing_res.status {
        SurfaceStatus::ToolMissing {
          binary,
          install_hint,
        } => {
          assert_eq!(binary, "dummy-tool");
          assert_eq!(install_hint, "install via package manager");
        }
        other => panic!("Expected ToolMissing status, got {other:?}"),
      }
    }
  }

  #[test]
  fn test_doctor_tool_missing_table_generation() {
    use crate::commands::doctor::install_missing_tools;
    use crate::surfaces::ToolInfo;

    let missing_tool = ToolInfo {
      binary: "__missing_dummy_binary_test__",
      description: "Dummy Missing Tool Test",
      install_hint: "Run npm install -g dummy",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    };

    let ok = install_missing_tools(&[missing_tool]);
    assert!(
      !ok,
      "Should return false when tool cannot be auto-installed"
    );
  }

  #[test]
  fn test_has_cargo_binstall_is_pure_path_lookup() {
    // has_cargo_binstall must resolve purely via check_binary_exists
    // (which::which under the hood) for both "cargo" and "cargo-binstall" --
    // no subprocess (e.g. `cargo binstall --version`) is spawned to probe
    // availability.
    //
    // Comparing the return value against `check_binary_exists("cargo") &&
    // check_binary_exists("cargo-binstall")` would NOT lock that contract: a
    // subprocess-based probe agrees with the PATH lookup in both environments
    // that matter (binstall installed and working / not installed at all), so
    // such an assertion passes either way. What actually discriminates the two
    // designs is the side effect: only the PATH-lookup implementation leaves
    // entries in BINARY_CACHE. "cargo-binstall" is looked up nowhere else in
    // the crate, so its presence there is attributable to this call alone.
    let result = has_cargo_binstall();

    let cache = BINARY_CACHE.get().expect("cache should be initialized");
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());

    let cargo_on_path = guard.get("cargo").copied().expect(
      "has_cargo_binstall must resolve `cargo` through check_binary_exists",
    );

    if cargo_on_path {
      // Short-circuiting means the second lookup only happens when the first
      // succeeded; when it does happen it must go through the PATH cache too,
      // and it must be what the return value is derived from.
      let binstall_on_path = guard.get("cargo-binstall").copied().expect(
        "has_cargo_binstall must resolve `cargo-binstall` through check_binary_exists",
      );
      assert_eq!(
        result, binstall_on_path,
        "return value must be the `cargo-binstall` PATH lookup, not a subprocess probe"
      );
    } else {
      assert!(
        !result,
        "has_cargo_binstall must be false when `cargo` is not on PATH"
      );
    }
  }

  #[test]
  fn test_check_binary_exists_caching() {
    let non_existent = "non_existent_binary_xyz_12345";
    let non_existent_result = check_binary_exists(non_existent);
    assert!(!non_existent_result);

    let existing = "cargo";
    let existing_result = check_binary_exists(existing);

    // Inspect BINARY_CACHE directly to verify process-lifetime memoization
    let cache = BINARY_CACHE.get().expect("cache should be initialized");
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(guard.get(non_existent), Some(&false));
    assert_eq!(guard.get(existing), Some(&existing_result));
  }

  #[test]
  fn test_check_binary_exists_thread_safety() {
    let handles: Vec<_> = (0..10)
      .map(|i| {
        std::thread::spawn(move || {
          let binary_name = format!("thread_test_binary_{i}");
          for _ in 0..50 {
            let _ = check_binary_exists("cargo");
            let _ = check_binary_exists(&binary_name);
          }
        })
      })
      .collect();

    for handle in handles {
      handle.join().unwrap();
    }

    let cache = BINARY_CACHE.get().expect("cache should be initialized");
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    assert!(guard.contains_key("cargo"));
    for i in 0..10 {
      let binary_name = format!("thread_test_binary_{i}");
      assert!(guard.contains_key(&binary_name));
    }
  }

  #[test]
  fn test_check_tool_compatibility_unprobeable_binary_returns_unknown_version()
  {
    use crate::engine::version::{
      ToolStatus, Version, check_tool_compatibility,
    };

    let bin = if cfg!(windows) { "where" } else { "false" };
    if which::which(bin).is_err() {
      return;
    }

    let status = check_tool_compatibility(bin, &Version::new(1, 0, 0));
    assert!(
      status.is_unknown_version(),
      "Unprobeable binary {bin} must return UnknownVersion status, got {status:?}"
    );
    assert!(!status.is_compatible());
    assert!(!status.is_not_found());
    assert!(!status.is_stale());
    assert!(!status.is_outdated());
    match status {
      ToolStatus::UnknownVersion(raw) => {
        // Output must be cleanly captured (or empty) and not panic
        let _ = raw;
      }
      other => panic!("Expected UnknownVersion, got {other:?}"),
    }
  }
}
