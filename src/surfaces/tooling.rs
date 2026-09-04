//! Tool-binary discovery and installation: the `InstallMethod` preference
//! chains for each supported CLI tool, binary-on-PATH detection, and
//! Windows-aware `Command` construction.

use super::{SurfaceResult, SurfaceStatus};
use crate::engine::version::Version;
use std::collections::HashMap;
use std::path::PathBuf;
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
// toolchain and `dtolnay/rust-toolchain@1.98.1` applies to the GitHub Action
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

// `CargoBinstall` sits *below* the npm family here, unlike every other
// chain that lists it, and deliberately so: cargo-binstall only installs a
// prebuilt binary when one is published for the target, and for
// `taplo-cli@0.10.0` none is -- the Fresh-Install Regression job caught it
// 404ing on both cargo-quickinstall targets and then falling through to
// "will be installed from source (with cargo)", a 1m54s release build of
// 279 crates on an ubuntu runner that already had npm sitting right there.
// A binstall entry ahead of a real prebuilt package is only a win when the
// prebuilt actually exists; here it inverted the whole point of the chain
// ordering. The npm-family entries resolve a genuinely prebuilt binary
// (reporting 0.9.0 -- see ToolChain's doc on why this row's
// `expected_binary_version` is None -- comfortably above MSTV_TAPLO), and
// binstall/cargo remain as the fallback for a machine with no Node
// toolchain at all.
const TAPLO_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("@taplo/cli@0.7.0"),
  InstallMethod::Pnpm("@taplo/cli@0.7.0"),
  InstallMethod::Yarn("@taplo/cli@0.7.0"),
  InstallMethod::Bun("@taplo/cli@0.7.0"),
  InstallMethod::Brew("taplo"),
  InstallMethod::Scoop("taplo"),
  InstallMethod::WingetId("tamasfe.taplo"),
  InstallMethod::CargoBinstall("taplo-cli@0.10.0"),
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

static BINARY_CACHE: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> =
  OnceLock::new();

/// Resolves `binary` to its concrete path on `PATH`, memoized per-process so
/// repeated lookups for the same binary don't re-hit the filesystem.
#[must_use]
pub fn resolve_binary_path(binary: &str) -> Option<PathBuf> {
  let cache = BINARY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
  if let Some(resolved) = guard.get(binary) {
    return resolved.clone();
  }
  let resolved = which::which(binary).ok();
  guard.insert(binary.to_string(), resolved.clone());
  resolved
}

/// Evicts `binary`'s entry (if any) from [`BINARY_CACHE`], forcing the next
/// [`resolve_binary_path`]/[`check_binary_exists`] call for it to re-hit the
/// filesystem instead of returning a stale memoized result.
///
/// Required after a successful install performed *within the same process*
/// (`fml fmt --install`, `fml lint --install`, `fml doctor --install`): the
/// preflight scan that decided a tool needed installing already called
/// [`check_binary_exists`] on it and memoized the miss. Without evicting that
/// entry here, every lookup for the rest of this invocation -- including the
/// one the just-installed tool's own surface makes before actually running
/// it -- would keep reading the pre-install "not found" result and report
/// the tool as still missing, even though the installer just placed it on
/// `PATH` and a fresh lookup would find it immediately.
pub fn forget_binary(binary: &str) {
  let cache = BINARY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
  guard.remove(binary);
}

/// Returns whether `binary` is resolvable on `PATH`, memoized per-process so
/// repeated checks for the same binary don't re-hit the filesystem.
#[must_use]
pub fn check_binary_exists(binary: &str) -> bool {
  resolve_binary_path(binary).is_some()
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

/// Returns `Some(SurfaceResult)` with `SurfaceStatus::ToolMissing` if `binary`
/// is not found on `PATH`, or `None` if it is available.
#[must_use]
pub fn tool_missing_guard(
  name: &'static str,
  binary: &str,
  start: Instant,
  hint: Option<&'static str>,
) -> Option<SurfaceResult> {
  if !check_binary_exists(binary) {
    Some(tool_missing_result(name, start, binary, hint.unwrap_or("")))
  } else {
    None
  }
}

/// Builds the `SurfaceResult` returned when autofix is requested on a surface
/// whose underlying tool does not support automatic lint fixing.
#[must_use]
pub fn lint_fix_unsupported(
  name: &'static str,
  start: Instant,
) -> SurfaceResult {
  SurfaceResult {
    surface_name: name,
    status: SurfaceStatus::Skipped {
      reason: "Tool does not support autofix; run fml fmt instead".to_string(),
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

/// Memoizes the outcome of [`ensure_cargo_binstall`]'s bootstrap attempt for
/// the lifetime of this process: `None` means it hasn't been tried yet,
/// `Some(bool)` records whether `cargo-binstall` was available afterward.
/// Shared across every tool in a single `fml ... --install` invocation so a
/// run that needs `cargo-binstall` for several tools (e.g. `typstyle` and a
/// `taplo`/`ruff` fallback) only pays the network round-trip once, and a
/// failed/offline attempt doesn't get retried per tool.
static BINSTALL_BOOTSTRAP: OnceLock<Mutex<Option<bool>>> = OnceLock::new();

/// Returns whether any step in `chain` would use `cargo-binstall`.
#[must_use]
pub fn chain_wants_cargo_binstall(chain: &[InstallMethod]) -> bool {
  chain
    .iter()
    .any(|m| matches!(m, InstallMethod::CargoBinstall(_)))
}

/// Runs `cargo-binstall`'s official prebuilt-binary install script, so
/// bootstrapping it never itself falls back to compiling `cargo-binstall`
/// from source. Uses the same `curl ... | sh` pattern rustup's own installer
/// documents (`--proto '=https' --tlsv1.2 -sSf`) on Linux/macOS, and the
/// equivalent PowerShell script on Windows. Returns `false` (rather than
/// propagating an error) on any failure to run the script -- a missing
/// `curl`/`powershell`, no network, or a non-zero exit -- so the caller can
/// fall through to the next installer in the chain instead of aborting.
fn run_cargo_binstall_bootstrap() -> bool {
  #[cfg(windows)]
  {
    std::process::Command::new("powershell")
      .args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "Set-ExecutionPolicy Unrestricted -Scope Process -Force; \
         iex (iwr 'https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.ps1' -UseBasicParsing).Content",
      ])
      .status()
      .is_ok_and(|status| status.success())
  }
  #[cfg(not(windows))]
  {
    if !check_binary_exists("sh") || !check_binary_exists("curl") {
      return false;
    }
    std::process::Command::new("sh")
      .arg("-c")
      .arg(
        "curl -L --proto '=https' --tlsv1.2 -sSf \
         https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
         | sh",
      )
      .status()
      .is_ok_and(|status| status.success())
  }
}

/// Ensures `cargo-binstall` is available, bootstrapping it via its official
/// install script (a real prebuilt binary for Linux/macOS/Windows, not a
/// source compile) if `cargo` is present but `cargo-binstall` itself isn't
/// yet on `PATH`.
///
/// This exists so tools with no genuine native package anywhere (`typstyle`,
/// `tinymist`; `taplo`/`ruff` as a fallback) get a real prebuilt-binary
/// install path on every OS instead of silently dropping straight to
/// `cargo install --locked` source compilation just because `cargo-binstall`
/// itself hadn't been bootstrapped yet. Side-effecting (spawns a network
/// request) and memoized at most once per process via [`BINSTALL_BOOTSTRAP`]
/// -- callers must only invoke this from an actual `--install` code path,
/// never from a read-only status scan (`fml doctor` without `--install`,
/// `fml fmt`/`lint` preflight without `--install`), which must stay free of
/// side effects.
#[must_use]
pub fn ensure_cargo_binstall() -> bool {
  if has_cargo_binstall() {
    return true;
  }
  if !check_binary_exists("cargo") {
    return false;
  }

  let cell = BINSTALL_BOOTSTRAP.get_or_init(|| Mutex::new(None));
  let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
  if let Some(available) = *guard {
    return available;
  }

  let ran_ok = run_cargo_binstall_bootstrap();
  if ran_ok {
    // The install script may have placed the binary in a PATH directory
    // whose "missing" result is already memoized from an earlier lookup
    // this process -- evict it so the recheck below hits the filesystem.
    forget_binary("cargo-binstall");
  }
  let available = has_cargo_binstall();
  *guard = Some(available);
  available
}

/// Returns whether bootstrapping `cargo-binstall` would let a *pinned*
/// prebuilt `CargoBinstall` entry take over from the installer the chain
/// currently resolves to, when that installer can't itself pin to the tool's
/// confirmed `expected_binary_version`.
///
/// This is the `typstyle`/`tinymist` case: their chains list
/// `CargoBinstall("<tool>@<pin>")` first, but on a machine where
/// `cargo-binstall` isn't on `PATH` yet the first *available* method is
/// `Brew`, whose core-tap bottle routinely trails the crates.io pin
/// (`typstyle` 0.15.0 vs. the pinned 0.15.1). Installing via that lagging
/// Homebrew formula then trips `install_missing_tools`' post-install
/// convergence guard -- a spurious `[WARN]` + non-clean exit on every macOS
/// `fml install` until the bottle catches up. Bootstrapping `cargo-binstall`
/// up front lets the already-first, pin-carrying prebuilt win instead;
/// Homebrew stays in the chain as the fallback if the bootstrap fails.
///
/// Kept pure (chain + pin + currently-selected method in, `bool` out) so it's
/// testable without touching `PATH`. `expected` is only ever `Some` for a row
/// whose `expected_binary_version` is a hand-confirmed 1:1 match with its
/// chain pins (see [`ToolChain`]), so a `Brew`-style unpinned entry losing to
/// `CargoBinstall` here can't regress a tool whose binary version legitimately
/// differs from its package-manager pin.
fn binstall_bootstrap_would_fix_pin_lag(
  chain: &[InstallMethod],
  expected: Option<&Version>,
  selected: Option<&InstallMethod>,
) -> bool {
  let Some(expected) = expected else {
    return false;
  };
  let Some(selected) = selected else {
    return false;
  };
  // The currently-selected installer already pins to the exact version --
  // nothing for a bootstrap to improve.
  if selected.pinned_version().as_ref() == Some(expected) {
    return false;
  }
  let binstall_idx = chain.iter().position(|m| {
    matches!(m, InstallMethod::CargoBinstall(_))
      && m.pinned_version().as_ref() == Some(expected)
  });
  let selected_idx = chain.iter().position(|m| m == selected);
  match (binstall_idx, selected_idx) {
    // The pin-carrying `CargoBinstall` entry sits ahead of whatever's
    // currently winning, so making it available flips the selection.
    (Some(b), Some(s)) => b < s,
    _ => false,
  }
}

/// Returns whether `binary`'s install chain would *actually* benefit from
/// bootstrapping `cargo-binstall` right now. Two cases:
///
/// 1. With `cargo-binstall` unavailable the chain's current first-available
///    method is the `cargo install --locked` source-compile fallback (or
///    nothing at all) -- bootstrapping the prebuilt-binary installer avoids a
///    multi-minute source build.
/// 2. A pin-carrying `CargoBinstall` entry sits ahead of the currently-winning
///    installer, which can't pin to the tool's confirmed version -- see
///    [`binstall_bootstrap_would_fix_pin_lag`].
///
/// Deliberately narrower than "the chain merely contains a `CargoBinstall`
/// step": `taplo`'s chain reaches `Npm` well before its `CargoBinstall`
/// entry, and on most runners `npm` is already on `PATH` -- bootstrapping
/// `cargo-binstall` there would spend a network round-trip changing
/// nothing about which installer actually runs.
#[must_use]
pub fn tool_would_benefit_from_cargo_binstall_bootstrap(binary: &str) -> bool {
  let Some(chain) = install_chain_for(binary) else {
    return false;
  };
  if !chain_wants_cargo_binstall(chain) {
    return false;
  }
  let selected = selected_install_method_for(binary);
  matches!(selected, None | Some(InstallMethod::Cargo { .. }))
    || binstall_bootstrap_would_fix_pin_lag(
      chain,
      pinned_version_for(binary).as_ref(),
      selected.as_ref(),
    )
}

/// Merges `additional`'s `PATH`-style entries onto the end of `current`'s,
/// case-insensitively deduplicated (Windows paths are case-insensitive) and
/// preserving `current`'s entries and their relative order first. Pure and
/// platform-independent so it can be unit-tested directly; the only
/// Windows-specific, side-effecting part of the refresh this supports is
/// [`refresh_windows_path_from_registry`], which sources `additional` from
/// the registry and applies the result via `std::env::set_var`.
#[must_use]
fn merge_path_entries(current: &str, additional: &str) -> String {
  let separator = if cfg!(windows) { ';' } else { ':' };
  let mut seen: std::collections::HashSet<String> =
    std::collections::HashSet::new();
  let mut entries: Vec<&str> = Vec::new();

  for entry in current
    .split(separator)
    .chain(additional.split(separator))
    .filter(|s| !s.is_empty())
  {
    // Windows paths are case-insensitive, so `C:\Go\bin` and `C:\go\bin`
    // are one entry there and folding the key is what stops a second copy
    // being appended. Unix paths are not: `/b` and `/B` are two different
    // directories, and folding them would silently drop a real entry the
    // caller asked to add.
    let key = if cfg!(windows) {
      entry.to_lowercase()
    } else {
      entry.to_string()
    };
    if seen.insert(key) {
      entries.push(entry);
    }
  }

  entries.join(&separator.to_string())
}

/// Re-reads `Path` from the Windows registry (`HKEY_LOCAL_MACHINE`'s System
/// Environment, then `HKEY_CURRENT_USER`'s -- the same precedence a freshly
/// started process would inherit) and merges any new entries into this
/// process's own `PATH`.
///
/// Needed because on Windows, an installer that registers a new directory
/// via the registry (Scoop, `winget`) does not update an *already-running*
/// process's inherited environment block -- only a process started after
/// the change picks it up. Without this, a tool Scoop/`winget` just
/// installed mid-run can be completely unresolvable for the rest of this
/// invocation: not a [`BINARY_CACHE`] staleness problem (already fixed by
/// [`forget_binary`]) but a genuine "this process's `PATH` string does not
/// contain that directory at all" problem underneath it, which shows up as
/// the tool's post-install version probe reporting "an unparseable
/// version" -- `which`/`Command::new(bare-name)` both fail to resolve the
/// binary at all, indistinguishable in that message from a real parse
/// failure of `--version` output that *did* run.
///
/// A no-op (and cheap: no process spawned) on non-Windows, since Scoop and
/// `winget` don't exist there and every other installer this crate uses on
/// Unix (`npm`, `cargo`, `pip`, `brew`, `apt`, ...) installs into a
/// directory already on `PATH` at process start.
pub fn refresh_windows_path_from_registry() {
  #[cfg(windows)]
  {
    let Ok(output) = std::process::Command::new("powershell")
      .args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User')",
      ])
      .output()
    else {
      return;
    };
    if !output.status.success() {
      return;
    }

    let registry_path =
      String::from_utf8_lossy(&output.stdout).trim().to_string();
    if registry_path.is_empty() {
      return;
    }

    merge_into_process_path(&registry_path);
  }
}

/// Merges `additional`'s entries into this process's own `PATH`, appended
/// after the entries already there (so nothing already resolvable changes
/// which binary it resolves to) and de-duplicated by
/// [`merge_path_entries`]. A no-op when `additional` contributes nothing
/// new, so the `set_var` below only ever runs when the `PATH` string
/// actually changes.
///
/// Only this process (and anything it spawns from here on) sees the change;
/// nothing is written to a shell profile or the registry. That is
/// deliberate -- making a `PATH` addition durable means editing files this
/// tool doesn't own, and the per-tool `install_hint` already tells the user
/// what to do about their own shell.
fn merge_into_process_path(additional: &str) {
  let current = std::env::var("PATH").unwrap_or_default();
  let merged = merge_path_entries(&current, additional);
  if merged != current {
    // SAFETY: single-threaded call site (`install_missing_tools` runs its
    // per-tool loop sequentially, not from a `rayon` fan-out like
    // `Runner::run`'s per-surface dispatch), and no other code in this
    // crate reads `PATH` concurrently with a call to this function.
    unsafe {
      std::env::set_var("PATH", merged);
    }
  }
}

/// Resolves the directory `go install` writes binaries into, from the raw
/// values of `GOBIN` and `GOPATH`: `GOBIN` when it is set, otherwise the
/// first `GOPATH` entry plus `bin` (Go's own documented default).
///
/// Split out from [`refresh_go_install_path`] and kept pure so the
/// precedence is unit-testable on a machine with no Go toolchain at all;
/// the `go env` invocation that sources these two values is the only part
/// left in the caller.
#[must_use]
fn go_bin_dir_from_env(gobin: &str, gopath: &str) -> Option<PathBuf> {
  let gobin = gobin.trim();
  if !gobin.is_empty() {
    return Some(PathBuf::from(gobin));
  }
  let separator = if cfg!(windows) { ';' } else { ':' };
  let first = gopath
    .split(separator)
    .map(str::trim)
    .find(|entry| !entry.is_empty())?;
  Some(PathBuf::from(first).join("bin"))
}

/// Adds `go install`'s output directory (`GOBIN`, else `$GOPATH/bin`) to
/// this process's `PATH` if it isn't already on it.
///
/// [`InstallMethod::GoInstall`] is the one installer in this module that
/// routinely writes into a directory that is *not* already on `PATH`.
/// Every other installer used here puts binaries next to (or under the same
/// prefix as) a package manager the user must already be able to invoke:
/// `npm -g`, `pipx`, `uv`, `brew`, `cargo install`, `rustup`. `$GOPATH/bin`
/// has no such guarantee -- Go creates it on demand, and it is on `PATH`
/// only if the user put it there. On a stock GitHub Actions Linux runner it
/// is not, so `go install golang.org/x/tools/cmd/goimports@v0.49.0`
/// succeeds and the very next lookup for `goimports` in the same invocation
/// still finds nothing.
///
/// That is the same user-visible symptom as the [`BINARY_CACHE`] staleness
/// [`forget_binary`] fixes, but a different cause -- here the binary
/// genuinely is not reachable from this process's `PATH` -- and the same
/// shape as the Scoop/`winget` case
/// [`refresh_windows_path_from_registry`] handles on Windows. Both are
/// dispatched from [`refresh_path_after_install`].
pub fn refresh_go_install_path() {
  let mut cmd = create_tool_command("go");
  cmd.args(["env", "GOBIN", "GOPATH"]);
  let Ok(output) = cmd.output() else {
    return;
  };
  if !output.status.success() {
    return;
  }

  // `go env NAME...` prints one value per line, in the order requested,
  // emitting an empty line for a variable that is unset -- so GOBIN being
  // empty (the common case) still leaves GOPATH on line 2.
  let stdout = String::from_utf8_lossy(&output.stdout);
  let mut lines = stdout.lines();
  let gobin = lines.next().unwrap_or_default();
  let gopath = lines.next().unwrap_or_default();

  if let Some(bin_dir) = go_bin_dir_from_env(gobin, gopath) {
    merge_into_process_path(&bin_dir.to_string_lossy());
  }
}

/// Applies whatever `PATH` fix-up the installer `program` needs for a
/// binary it just installed to be resolvable for the rest of this process,
/// and does nothing for the installers that need none.
///
/// Called right after a successful install (alongside [`forget_binary`],
/// which handles the separate in-process caching half of the same
/// symptom). Keeping the per-installer knowledge here rather than at the
/// call site means a new [`InstallMethod`] whose bin directory isn't on
/// `PATH` has exactly one place to be taught about.
pub fn refresh_path_after_install(program: &str) {
  match program {
    // Scoop and winget register their PATH changes in the Windows
    // registry, which an already-running process's inherited environment
    // block never picks up on its own.
    "scoop" | "winget" => refresh_windows_path_from_registry(),
    // `go install` writes into $GOBIN / $GOPATH/bin, which is frequently
    // not on PATH at all.
    "go" => refresh_go_install_path(),
    _ => {}
  }
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
    if let Some(path) = resolve_binary_path(binary) {
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

/// How a caller of [`run_tool_command_classified`] /
/// [`crate::surfaces::diff_check_via_tempcopy_classified`] wants a given
/// non-zero exit code from its tool interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
  /// The tool ran to completion and reported rule violations or formatting
  /// drift — translated to [`SurfaceStatus::ViolationsFound`].
  ViolationsFound,
  /// The tool could not do its job (bad config, typecheck failure, internal
  /// crash, unusable arguments) — translated to
  /// [`SurfaceStatus::ExecutionError`]. This is *not* a lint result.
  ExecutionError,
}

/// Classifier for a tool that signals "ran, found violations" with exit code
/// `1` and a genuine failure with any other non-zero exit. Fits
/// `golangci-lint` (`1` = issues found; `7` = typecheck/config error, `2`,
/// `3`, `5`, `6` = other internal failures). An exit with no numeric code
/// (killed by a signal) classifies as [`ExitClass::ExecutionError`].
#[must_use]
pub fn classify_exit_one_as_violation(code: Option<i32>) -> ExitClass {
  if code == Some(1) {
    ExitClass::ViolationsFound
  } else {
    ExitClass::ExecutionError
  }
}

/// Classifier for a tool that has **no** exit code meaning "found
/// violations", so every non-zero exit is a real failure. This is the case
/// for every write-mode formatter `fml` drives — `gofmt -w`, `goimports -w`,
/// `prettier --write`, `biome format --write` all exit `0` whether or not
/// they reformatted anything and only exit non-zero (`2`) on a syntax error,
/// an unreadable file, or a bad config. Formatting drift on the `--check`
/// path is detected by comparing file contents, never by the exit code.
#[must_use]
pub fn classify_all_nonzero_as_error(_code: Option<i32>) -> ExitClass {
  ExitClass::ExecutionError
}

/// Combines a failed tool's captured `stdout` and `stderr` into one
/// human-readable message. When both streams carry content, **neither is
/// dropped**: stdout is shown first, then a `stderr:`-labelled block — this is
/// what keeps findings visible for tools (markdownlint-cli2, golangci-lint)
/// that print a banner to stdout and their actual diagnostics to stderr. When
/// only one stream is non-empty it is returned trimmed; when neither is,
/// `fallback` is used verbatim.
pub(crate) fn merge_tool_streams(
  stdout: &str,
  stderr: &str,
  fallback: &str,
) -> String {
  let stdout = stdout.trim();
  let stderr = stderr.trim();
  match (stdout.is_empty(), stderr.is_empty()) {
    (false, false) => format!("{stdout}\n\nstderr:\n{stderr}"),
    (false, true) => stdout.to_string(),
    (true, false) => stderr.to_string(),
    (true, true) => fallback.to_string(),
  }
}

/// Plain-text description of a non-zero [`std::process::ExitStatus`] with no
/// `Display`-stutter (`ExitStatus`'s own `Display` is already `exit code: N`).
fn exit_status_summary(status: &std::process::ExitStatus) -> String {
  status.code().map_or_else(
    || "Command failed (terminated by signal)".to_string(),
    |code| format!("Command failed with exit code {code}"),
  )
}

/// Runs a tool command, measures execution duration, and translates exit
/// status to a `SurfaceResult`, treating **every** non-zero exit as
/// [`SurfaceStatus::ViolationsFound`].
///
/// Surfaces whose tool distinguishes "found violations" from "could not run"
/// through its exit code should call [`run_tool_command_classified`] instead,
/// so a tool *failure* is reported as [`SurfaceStatus::ExecutionError`] rather
/// than a spurious lint violation.
pub fn run_tool_command(
  surface_name: &'static str,
  cmd: &mut std::process::Command,
) -> SurfaceResult {
  run_tool_command_classified(surface_name, cmd, |_| ExitClass::ViolationsFound)
}

/// Like [`run_tool_command`], but lets the caller classify each non-zero exit
/// code as either a violation result or a tool failure via `classify`, which
/// receives [`std::process::ExitStatus::code`] (`None` when the process was
/// killed by a signal).
///
/// On any non-zero exit, both captured streams are surfaced when both are
/// non-empty (see [`merge_tool_streams`]); no non-empty stream is discarded.
pub fn run_tool_command_classified(
  surface_name: &'static str,
  cmd: &mut std::process::Command,
  classify: impl Fn(Option<i32>) -> ExitClass,
) -> SurfaceResult {
  let start = Instant::now();
  match cmd.output() {
    Ok(output) => {
      let duration = start.elapsed();
      if output.status.success() {
        return SurfaceResult {
          surface_name,
          status: SurfaceStatus::Passed,
          duration,
        };
      }

      let stdout = String::from_utf8_lossy(&output.stdout);
      let stderr = String::from_utf8_lossy(&output.stderr);
      let message = merge_tool_streams(
        &stdout,
        &stderr,
        &exit_status_summary(&output.status),
      );
      let status = match classify(output.status.code()) {
        ExitClass::ViolationsFound => SurfaceStatus::ViolationsFound {
          message,
          diff: None,
        },
        ExitClass::ExecutionError => SurfaceStatus::ExecutionError { message },
      };
      SurfaceResult {
        surface_name,
        status,
        duration,
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

    // Matched by content rather than by chain position: the order of
    // taplo's chain is a separate decision that has already changed once
    // (see TAPLO_CHAIN's comment on why binstall sits below the npm
    // family), and reordering it must not fail an assertion whose actual
    // subject is that the npm entry stays version-pinned.
    let taplo = install_chain_for("taplo").unwrap();
    let taplo_npm = (
      "npm".to_string(),
      vec![
        "install".to_string(),
        "-g".to_string(),
        "@taplo/cli@0.7.0".to_string(),
      ],
    );
    assert!(
      taplo.iter().any(|method| method.command() == taplo_npm),
      "taplo's npm entry must stay pinned to @taplo/cli@0.7.0"
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

    let cargo_on_path = guard.get("cargo").expect(
      "has_cargo_binstall must resolve `cargo` through check_binary_exists",
    );

    if cargo_on_path.is_some() {
      // Short-circuiting means the second lookup only happens when the first
      // succeeded; when it does happen it must go through the PATH cache too,
      // and it must be what the return value is derived from.
      let binstall_on_path = guard.get("cargo-binstall").expect(
        "has_cargo_binstall must resolve `cargo-binstall` through check_binary_exists",
      );
      assert_eq!(
        result,
        binstall_on_path.is_some(),
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
    assert_eq!(guard.get(non_existent), Some(&None));
    assert_eq!(
      guard.get(existing).map(Option::is_some),
      Some(existing_result)
    );
  }

  // Regression coverage for the bug this PR fixes: a tool installed within
  // an `fml ... --install` invocation was reported ToolMissing by the very
  // next step of the *same* invocation, because the preflight scan's
  // "not found" result stayed memoized in BINARY_CACHE for the rest of the
  // process. `forget_binary` is the fix -- these tests lock its contract
  // directly against the cache rather than against a real install (which
  // would need a real package manager and network access to exercise).
  #[test]
  fn test_forget_binary_evicts_a_stale_cached_miss() {
    let binary = "__forget_binary_test_stale_miss__";

    // Prime the cache exactly the way a preflight scan does when a tool is
    // still missing: a memoized `None`.
    {
      let cache = BINARY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
      let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
      guard.insert(binary.to_string(), None);
    }
    assert!(
      !check_binary_exists(binary),
      "check_binary_exists must read the primed cache entry, not re-probe PATH"
    );

    forget_binary(binary);

    // The entry must be gone entirely, not just still `None` -- leaving a
    // `None` behind would be exactly the bug this function exists to fix.
    {
      let cache = BINARY_CACHE.get().expect("cache should be initialized");
      let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
      assert!(
        !guard.contains_key(binary),
        "forget_binary must remove the cache entry entirely"
      );
    }

    // And the next lookup must actually re-probe (re-populating the entry),
    // not just leave it absent forever.
    let _ = check_binary_exists(binary);
    let cache = BINARY_CACHE.get().expect("cache should be initialized");
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
      guard.contains_key(binary),
      "the lookup right after forget_binary must repopulate the cache"
    );
  }

  #[test]
  fn test_forget_binary_is_a_noop_for_a_binary_never_looked_up() {
    // Must not panic when called for a binary `install_missing_tools` is
    // about to install but that was never actually looked up this process
    // (e.g. a tool added to `missing` via a path that skipped the usual
    // `lookup_tool_info` preflight probe).
    forget_binary("__forget_binary_test_never_looked_up__");
  }

  // Coverage for the "at least one real prebuilt-binary installer per OS"
  // gap this PR also fixes: typstyle/tinymist/taplo have no genuine native
  // package anywhere, so cargo-binstall (a real prebuilt binary, not a
  // source compile) is their only non-source-compile path on every OS,
  // Linux included.
  #[test]
  fn test_chain_wants_cargo_binstall() {
    let typstyle_chain =
      install_chain_for("typstyle").expect("typstyle must have a chain");
    assert!(chain_wants_cargo_binstall(typstyle_chain));

    // rustfmt's chain is rustup-only -- must not spuriously report wanting
    // cargo-binstall just because *some* chain does.
    let rustfmt_chain =
      install_chain_for("rustfmt").expect("rustfmt must have a chain");
    assert!(!chain_wants_cargo_binstall(rustfmt_chain));
  }

  #[test]
  fn test_every_source_compile_only_cargo_tool_offers_cargo_binstall_first() {
    // If any of these ever loses its CargoBinstall step, the *only*
    // remaining install path on an OS without a matching Brew/Scoop/Winget
    // entry (Linux, for all three) becomes compiling from source -- exactly
    // the multi-minute typstyle build the bug report was filed over.
    for binary in ["typstyle", "tinymist", "taplo"] {
      let chain = install_chain_for(binary)
        .unwrap_or_else(|| panic!("{binary} must have a registered chain"));
      assert!(
        chain_wants_cargo_binstall(chain),
        "{binary}'s chain must include CargoBinstall -- otherwise Linux has \
         no real prebuilt-binary install path for it at all"
      );
    }
  }

  #[test]
  fn test_tool_would_benefit_from_cargo_binstall_bootstrap_unknown_binary() {
    // A binary with no registered chain at all must never claim it would
    // benefit from bootstrapping cargo-binstall -- there's nothing to
    // select an installer from.
    assert!(!tool_would_benefit_from_cargo_binstall_bootstrap(
      "__no_such_registered_tool__"
    ));
  }

  #[test]
  fn test_tool_would_benefit_from_cargo_binstall_bootstrap_rustup_only_chain() {
    // rustfmt's chain never references cargo-binstall at all, so it must
    // never trigger a bootstrap attempt regardless of what's on PATH.
    assert!(!tool_would_benefit_from_cargo_binstall_bootstrap("rustfmt"));
  }

  #[test]
  fn test_typstyle_chain_prefers_pinned_cargo_binstall_over_brew() {
    // Chain-definition guard: `CargoBinstall("typstyle@<pin>")` must sit
    // ahead of `Brew("typstyle")`, and its inline pin must equal the
    // confirmed `expected_binary_version`. This is the ordering the
    // pin-lag bootstrap (below) relies on -- if the chain ever regresses so
    // Brew comes first, `selected_install_method_for` would hand back the
    // lagging Homebrew bottle even after cargo-binstall is bootstrapped.
    let chain =
      install_chain_for("typstyle").expect("typstyle must have a chain");
    let expected =
      pinned_version_for("typstyle").expect("typstyle has a confirmed pin");

    let binstall_idx = chain
      .iter()
      .position(|m| {
        matches!(m, InstallMethod::CargoBinstall(_))
          && m.pinned_version().as_ref() == Some(&expected)
      })
      .expect("typstyle chain must carry a pin-matching CargoBinstall entry");
    let brew_idx = chain
      .iter()
      .position(|m| matches!(m, InstallMethod::Brew(_)))
      .expect("typstyle chain keeps Brew as a fallback");

    assert!(
      binstall_idx < brew_idx,
      "pinned CargoBinstall must be preferred over Brew for typstyle"
    );
  }

  #[test]
  fn test_binstall_bootstrap_fixes_brew_pin_lag_for_typstyle() {
    // The macOS #102 case: cargo-binstall isn't on PATH, so the first
    // *available* installer is Brew, whose core-tap bottle trails the
    // crates.io pin. Bootstrapping cargo-binstall lets the already-first,
    // pin-carrying prebuilt win -- so this must report `true`.
    let chain =
      install_chain_for("typstyle").expect("typstyle must have a chain");
    let expected = pinned_version_for("typstyle");
    let brew = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::Brew(_)))
      .copied();

    assert!(binstall_bootstrap_would_fix_pin_lag(
      chain,
      expected.as_ref(),
      brew.as_ref(),
    ));
  }

  #[test]
  fn test_binstall_bootstrap_no_op_when_binstall_already_selected() {
    // If the currently-selected installer *is* the pin-carrying
    // CargoBinstall entry, there is nothing for a bootstrap to improve.
    let chain =
      install_chain_for("typstyle").expect("typstyle must have a chain");
    let expected = pinned_version_for("typstyle");
    let binstall = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::CargoBinstall(_)))
      .copied();

    assert!(!binstall_bootstrap_would_fix_pin_lag(
      chain,
      expected.as_ref(),
      binstall.as_ref(),
    ));
  }

  #[test]
  fn test_binstall_bootstrap_no_op_without_a_confirmed_pin() {
    // Isolates the `expected: None` guard specifically. taplo's chain has a
    // `CargoBinstall` entry *ahead of* its `cargo install` source-compile
    // fallback, so if `expected` were `Some(<that pin>)` the index check
    // (binstall before selected) would fire and return `true`. taplo
    // deliberately carries `expected_binary_version: None` (its installed
    // binary reports a different number than any chain pin), so the guard
    // must short-circuit to `false` before the ordering is ever considered.
    let chain = install_chain_for("taplo").expect("taplo must have a chain");
    let cargo_fallback = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::Cargo { .. }))
      .copied();
    let binstall_idx = chain
      .iter()
      .position(|m| matches!(m, InstallMethod::CargoBinstall(_)))
      .expect("taplo chain has a CargoBinstall entry");
    let cargo_idx = chain
      .iter()
      .position(|m| matches!(m, InstallMethod::Cargo { .. }))
      .expect("taplo chain has a cargo-install fallback");
    assert!(
      binstall_idx < cargo_idx,
      "precondition: taplo's CargoBinstall sits ahead of its cargo fallback, \
       so only the `expected: None` guard can make this return false"
    );
    assert_eq!(
      pinned_version_for("taplo"),
      None,
      "taplo must stay opted out of the pin comparison for this test to \
       isolate the guard it targets"
    );

    assert!(!binstall_bootstrap_would_fix_pin_lag(
      chain,
      None,
      cargo_fallback.as_ref(),
    ));
  }

  #[test]
  fn test_binstall_bootstrap_fixes_brew_pin_lag_for_tinymist() {
    // tinymist has the identical chain shape to typstyle (pin-carrying
    // `CargoBinstall` first, `Brew` as fallback) and a confirmed
    // `expected_binary_version`, so the same #102 mechanism must cover it.
    let chain =
      install_chain_for("tinymist").expect("tinymist must have a chain");
    let expected = pinned_version_for("tinymist");
    let brew = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::Brew(_)))
      .copied();

    assert!(binstall_bootstrap_would_fix_pin_lag(
      chain,
      expected.as_ref(),
      brew.as_ref(),
    ));
  }

  #[test]
  fn test_binstall_bootstrap_pin_lag_for_ruff_is_scoop_winget_only() {
    // ruff's chain is the asymmetric case: `Brew` sits *ahead* of the
    // pin-carrying `CargoBinstall` entry, but `Scoop`/`WingetName` sit
    // *after* it. So a Windows host with only scoop (no Python toolchain)
    // selects the unpinned scoop package and benefits from a bootstrap,
    // while a brew-selected ruff does not -- brew wins regardless of whether
    // cargo-binstall gets bootstrapped, an incidental consequence of chain
    // order, not a deliberate exemption.
    let chain = install_chain_for("ruff").expect("ruff must have a chain");
    let expected = pinned_version_for("ruff");

    let scoop = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::Scoop(_)))
      .copied();
    assert!(
      binstall_bootstrap_would_fix_pin_lag(
        chain,
        expected.as_ref(),
        scoop.as_ref(),
      ),
      "scoop-selected ruff (no pin) benefits from the cargo-binstall bootstrap"
    );

    let brew = chain
      .iter()
      .find(|m| matches!(m, InstallMethod::Brew(_)))
      .copied();
    assert!(
      !binstall_bootstrap_would_fix_pin_lag(
        chain,
        expected.as_ref(),
        brew.as_ref(),
      ),
      "brew-selected ruff is unaffected -- Brew sits ahead of CargoBinstall \
       in RUFF_CHAIN, so bootstrapping changes nothing about the selection"
    );
  }

  // merge_path_entries is the pure half of every post-install PATH refresh
  // (refresh_windows_path_from_registry, refresh_go_install_path) -- the
  // registry/`go env` read plus the std::env::set_var side effect isn't
  // something a unit test should perform for real (it would mutate the test
  // process's actual PATH for every other test running in the same binary),
  // but the merge logic itself (case-insensitive dedup, order preservation)
  // is exactly what determines whether a just-installed binary's new
  // directory actually gets picked up, so it's worth locking down directly.
  // Builds a fixture path that is well-formed for the platform under test.
  // merge_path_entries splits on the platform's own PATH separator, so a
  // hardcoded Windows-style `C:\a` fixture splits at its own colon when the
  // tests run on Unix -- which is exactly how these tests failed on Linux
  // once the lib started compiling there.
  fn fixture_path(name: &str) -> String {
    if cfg!(windows) {
      format!("C:\\{name}")
    } else {
      format!("/{name}")
    }
  }

  #[test]
  fn test_merge_path_entries_appends_new_dirs_only() {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let (a, b, c) = (fixture_path("a"), fixture_path("b"), fixture_path("c"));
    let current = format!("{a}{sep}{b}");
    let additional = format!("{b}{sep}{c}");

    let merged = merge_path_entries(&current, &additional);
    let parts: Vec<&str> = merged.split(sep).collect();

    assert_eq!(
      parts,
      vec![a.as_str(), b.as_str(), c.as_str()],
      "must keep `current`'s entries first, in order, and only append \
       genuinely new entries from `additional`"
    );
  }

  #[test]
  fn test_merge_path_entries_case_folds_only_where_paths_are() {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let (a, b, c) = (fixture_path("a"), fixture_path("b"), fixture_path("c"));
    let b_upper = b.to_uppercase();
    let current = format!("{a}{sep}{b}");
    let additional = format!("{b_upper}{sep}{c}");

    let merged = merge_path_entries(&current, &additional);
    let parts: Vec<&str> = merged.split(sep).collect();

    if cfg!(windows) {
      assert_eq!(
        parts,
        vec![a.as_str(), b.as_str(), c.as_str()],
        "Windows paths are case-insensitive, so a case-only variant of an \
         entry already present must not be appended a second time"
      );
    } else {
      assert_eq!(
        parts,
        vec![a.as_str(), b.as_str(), b_upper.as_str(), c.as_str()],
        "Unix paths are case-sensitive: /b and /B are different \
         directories, and folding them would drop a directory the caller \
         asked to add"
      );
    }
  }

  #[test]
  fn test_merge_path_entries_noop_when_nothing_new() {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let current = format!("{}{sep}{}", fixture_path("a"), fixture_path("b"));
    let merged = merge_path_entries(&current, &current);
    assert_eq!(
      merged, current,
      "merging PATH with itself must not change anything (and callers rely \
       on this to skip the std::env::set_var call entirely when nothing \
       actually changed)"
    );
  }

  #[test]
  fn test_merge_path_entries_ignores_empty_segments() {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let (a, b, c) = (fixture_path("a"), fixture_path("b"), fixture_path("c"));
    let current = format!("{a}{sep}{sep}{b}{sep}");
    let additional = format!("{sep}{c}{sep}");
    let merged = merge_path_entries(&current, &additional);
    let parts: Vec<&str> =
      merged.split(sep).filter(|s| !s.is_empty()).collect();
    assert_eq!(parts, vec![a.as_str(), b.as_str(), c.as_str()]);
    assert!(
      !merged.contains(&format!("{sep}{sep}")),
      "must not introduce empty PATH segments from empty input segments"
    );
  }

  // go_bin_dir_from_env decides where `go install` just put a binary, which
  // is what refresh_go_install_path adds to PATH. Getting the GOBIN/GOPATH
  // precedence wrong means adding a directory that holds nothing and
  // leaving `goimports` unresolvable right after installing it -- the exact
  // failure the Fresh-Install Regression CI job exists to catch.
  #[test]
  fn test_go_bin_dir_prefers_gobin_when_set() {
    let dir = go_bin_dir_from_env("/custom/gobin", "/home/u/go");
    assert_eq!(
      dir,
      Some(PathBuf::from("/custom/gobin")),
      "GOBIN, when set, is exactly where `go install` writes -- it must win \
       over $GOPATH/bin"
    );
  }

  #[test]
  fn test_go_bin_dir_falls_back_to_first_gopath_entry() {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let gopath = format!("/home/u/go{sep}/home/u/other");
    let dir = go_bin_dir_from_env("", &gopath);
    assert_eq!(
      dir,
      Some(PathBuf::from("/home/u/go").join("bin")),
      "with GOBIN unset, `go install` uses the FIRST GOPATH entry's bin \
       directory, not the last and not all of them"
    );
  }

  #[test]
  fn test_go_bin_dir_tolerates_whitespace_and_empty_values() {
    assert_eq!(
      go_bin_dir_from_env("  /custom/gobin  ", ""),
      Some(PathBuf::from("/custom/gobin")),
      "`go env` output arrives with a trailing newline per value; a padded \
       GOBIN must not produce a path with whitespace baked into it"
    );
    assert_eq!(
      go_bin_dir_from_env("", "   "),
      None,
      "no GOBIN and no usable GOPATH means there's no directory to add -- \
       must be None rather than a bare \"bin\" relative path"
    );
    assert_eq!(
      go_bin_dir_from_env("", ""),
      None,
      "a Go toolchain that reports neither value must leave PATH alone"
    );
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
  fn test_tool_missing_guard() {
    let start = Instant::now();
    let res = tool_missing_guard(
      "test",
      "non_existent_tool_xyz_123",
      start,
      Some("install it"),
    );
    assert!(res.is_some());
    let res = res.unwrap();
    assert_eq!(res.surface_name, "test");
    match res.status {
      SurfaceStatus::ToolMissing {
        binary,
        install_hint,
      } => {
        assert_eq!(binary, "non_existent_tool_xyz_123");
        assert_eq!(install_hint, "install it");
      }
      other => panic!("Expected ToolMissing, got {other:?}"),
    }

    let res_none =
      tool_missing_guard("test", "non_existent_tool_xyz_123", start, None);
    assert!(res_none.is_some());
    match res_none.unwrap().status {
      SurfaceStatus::ToolMissing {
        binary,
        install_hint,
      } => {
        assert_eq!(binary, "non_existent_tool_xyz_123");
        assert_eq!(install_hint, "");
      }
      other => panic!("Expected ToolMissing, got {other:?}"),
    }
  }

  #[test]
  fn test_lint_fix_unsupported() {
    let start = Instant::now();
    let res = lint_fix_unsupported("test", start);
    assert_eq!(res.surface_name, "test");
    match res.status {
      SurfaceStatus::Skipped { reason } => {
        assert_eq!(
          reason,
          "Tool does not support autofix; run fml fmt instead"
        );
      }
      other => panic!("Expected Skipped, got {other:?}"),
    }
  }

  /// Builds a `Command` that writes the given text to stdout and/or stderr
  /// (each skipped when empty) and then exits with `exit_code`, using the
  /// platform shell so the tests run on both Windows and Unix CI.
  fn scripted_command(
    stdout: &str,
    stderr: &str,
    exit_code: i32,
  ) -> std::process::Command {
    #[cfg(windows)]
    {
      let mut steps: Vec<String> = Vec::new();
      if !stdout.is_empty() {
        steps.push(format!("echo {stdout}"));
      }
      if !stderr.is_empty() {
        steps.push(format!("echo {stderr} 1>&2"));
      }
      steps.push(format!("exit /b {exit_code}"));
      let mut cmd = std::process::Command::new("cmd");
      cmd.arg("/C").arg(steps.join(" & "));
      cmd
    }
    #[cfg(not(windows))]
    {
      let mut script = String::new();
      if !stdout.is_empty() {
        script.push_str(&format!("printf '%s\\n' '{stdout}'; "));
      }
      if !stderr.is_empty() {
        script.push_str(&format!("printf '%s\\n' '{stderr}' 1>&2; "));
      }
      script.push_str(&format!("exit {exit_code}"));
      let mut cmd = std::process::Command::new("sh");
      cmd.arg("-c").arg(script);
      cmd
    }
  }

  #[test]
  fn test_run_tool_command_success_is_passed() {
    let mut cmd = scripted_command("", "", 0);
    let res = run_tool_command("t", &mut cmd);
    assert!(matches!(res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_run_tool_command_stdout_only() {
    let mut cmd = scripted_command("ONLYOUT", "", 1);
    let res = run_tool_command("t", &mut cmd);
    match res.status {
      SurfaceStatus::ViolationsFound { message, .. } => {
        assert_eq!(message, "ONLYOUT");
        assert!(!message.contains("stderr:"));
      }
      other => panic!("expected ViolationsFound, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_stderr_only() {
    let mut cmd = scripted_command("", "ONLYERR", 1);
    let res = run_tool_command("t", &mut cmd);
    match res.status {
      SurfaceStatus::ViolationsFound { message, .. } => {
        assert_eq!(message, "ONLYERR");
        assert!(!message.contains("stderr:"));
      }
      other => panic!("expected ViolationsFound, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_both_streams_surface_both() {
    let mut cmd = scripted_command("BANNER", "FINDINGS", 1);
    let res = run_tool_command("t", &mut cmd);
    match res.status {
      SurfaceStatus::ViolationsFound { message, .. } => {
        let out = message.find("BANNER").expect("stdout kept");
        let err = message.find("FINDINGS").expect("stderr kept");
        assert!(out < err, "stdout is shown before stderr");
        assert!(message.contains("stderr:"), "stderr block is labelled");
      }
      other => panic!("expected ViolationsFound, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_neither_stream() {
    let mut cmd = scripted_command("", "", 3);
    let res = run_tool_command("t", &mut cmd);
    match res.status {
      SurfaceStatus::ViolationsFound { message, .. } => {
        // Exact, so a reintroduced `Display` stutter ("exit code exit
        // code: 3") fails the assertion.
        assert_eq!(message, "Command failed with exit code 3");
      }
      other => panic!("expected ViolationsFound, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_classified_all_nonzero_as_error() {
    // The write-mode-formatter classifier: exit 2 (prettier/gofmt parse
    // failure) is a tool failure, not formatting drift, and both streams
    // are still surfaced.
    let mut cmd = scripted_command("BANNER", "PARSEFAIL", 2);
    let res =
      run_tool_command_classified("t", &mut cmd, classify_all_nonzero_as_error);
    match res.status {
      SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("BANNER"));
        assert!(message.contains("PARSEFAIL"));
      }
      other => panic!("expected ExecutionError, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_classified_error_exit() {
    // Classifier maps everything but exit 1 to a tool failure; exit 7 is
    // golangci-lint's typecheck/config error.
    let mut cmd = scripted_command("0 issues.", "TYPECHECKFAIL", 7);
    let res = run_tool_command_classified(
      "t",
      &mut cmd,
      classify_exit_one_as_violation,
    );
    match res.status {
      SurfaceStatus::ExecutionError { message } => {
        assert!(message.contains("TYPECHECKFAIL"), "real cause surfaced");
        assert!(message.contains("0 issues."), "banner stream not dropped");
      }
      other => panic!("expected ExecutionError, got {other:?}"),
    }
  }

  #[test]
  fn test_run_tool_command_classified_violation_exit() {
    let mut cmd = scripted_command("ONEISSUE", "", 1);
    let res = run_tool_command_classified(
      "t",
      &mut cmd,
      classify_exit_one_as_violation,
    );
    match res.status {
      SurfaceStatus::ViolationsFound { message, .. } => {
        assert_eq!(message, "ONEISSUE");
      }
      other => panic!("expected ViolationsFound, got {other:?}"),
    }
  }
}
