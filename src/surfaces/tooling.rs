//! Tool-binary discovery and installation: the `InstallMethod` preference
//! chains for each supported CLI tool, binary-on-PATH detection, and
//! Windows-aware `Command` construction.

use super::{SurfaceResult, SurfaceStatus};
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
  CargoBinstall(&'static str),
  Npm(&'static str),
  Pnpm(&'static str),
  Yarn(&'static str),
  Bun(&'static str),
  Uv(&'static str),
  Pipx(&'static str),
  Pip(&'static str),
  Pip3(&'static str),
  Apt(&'static str),
  Brew(&'static str),
  Scoop(&'static str),
  /// winget resolves the package by fuzzy name/id match.
  WingetName(&'static str),
  /// winget resolves the package via `--id=<id> -e`, an exact,
  /// unambiguous match.
  WingetId(&'static str),
  Cargo {
    package: &'static str,
    locked: bool,
  },
  Rustup(&'static str),
  /// `go install <package>@latest`. Requires the Go toolchain (`go`) on PATH.
  GoInstall(&'static str),
}

impl InstallMethod {
  pub(super) fn is_available(&self) -> bool {
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

  pub(super) fn command(&self) -> (String, Vec<String>) {
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
        vec!["install".to_string(), format!("{pkg}@latest")],
      ),
    }
  }
}

const TAPLO_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("taplo-cli"),
  InstallMethod::Npm("@taplo/cli"),
  InstallMethod::Pnpm("@taplo/cli"),
  InstallMethod::Yarn("@taplo/cli"),
  InstallMethod::Bun("@taplo/cli"),
  InstallMethod::Brew("taplo"),
  InstallMethod::Scoop("taplo"),
  InstallMethod::WingetId("tamasfe.taplo"),
  InstallMethod::Cargo {
    package: "taplo-cli",
    locked: true,
  },
];

const TYPSTYLE_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("typstyle"),
  InstallMethod::Brew("typstyle"),
  InstallMethod::Scoop("typstyle"),
  InstallMethod::WingetName("typstyle"),
  InstallMethod::Cargo {
    package: "typstyle",
    locked: true,
  },
];

const TINYMIST_CHAIN: &[InstallMethod] = &[
  InstallMethod::CargoBinstall("tinymist"),
  InstallMethod::Npm("@myriaddreamin/tinymist"),
  InstallMethod::Brew("tinymist"),
  InstallMethod::Scoop("tinymist"),
  InstallMethod::WingetName("Myriad-Dreamin.tinymist"),
  InstallMethod::Cargo {
    package: "tinymist",
    locked: true,
  },
];

const RUFF_CHAIN: &[InstallMethod] = &[
  InstallMethod::Uv("ruff"),
  InstallMethod::Pipx("ruff"),
  InstallMethod::Pip("ruff"),
  InstallMethod::Pip3("ruff"),
  InstallMethod::Brew("ruff"),
  InstallMethod::CargoBinstall("ruff"),
  InstallMethod::Scoop("ruff"),
  InstallMethod::WingetName("Astral-sh.ruff"),
  InstallMethod::Cargo {
    package: "ruff",
    locked: true,
  },
];

const PRETTIER_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("prettier"),
  InstallMethod::Pnpm("prettier"),
  InstallMethod::Yarn("prettier"),
  InstallMethod::Bun("prettier"),
  InstallMethod::Brew("prettier"),
  InstallMethod::Scoop("prettier"),
  InstallMethod::WingetName("Prettier.Prettier"),
];

const BIOME_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("@biomejs/biome"),
  InstallMethod::Pnpm("@biomejs/biome"),
  InstallMethod::Yarn("@biomejs/biome"),
  InstallMethod::Bun("@biomejs/biome"),
  InstallMethod::Brew("biome"),
  InstallMethod::Scoop("biome"),
];

const MARKDOWNLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Npm("markdownlint-cli2"),
  InstallMethod::Pnpm("markdownlint-cli2"),
  InstallMethod::Yarn("markdownlint-cli2"),
  InstallMethod::Bun("markdownlint-cli2"),
  InstallMethod::Brew("markdownlint-cli2"),
  InstallMethod::Scoop("markdownlint-cli2"),
];

const YAMLLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Uv("yamllint"),
  InstallMethod::Pipx("yamllint"),
  InstallMethod::Pip("yamllint"),
  InstallMethod::Pip3("yamllint"),
  InstallMethod::Apt("yamllint"),
  InstallMethod::Brew("yamllint"),
  InstallMethod::Scoop("yamllint"),
  InstallMethod::WingetName("yamllint"),
];

const CLANG_FORMAT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Apt("clang-format"),
  InstallMethod::Brew("clang-format"),
  InstallMethod::Pipx("clang-format"),
  InstallMethod::Pip("clang-format"),
  InstallMethod::Pip3("clang-format"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

const CLANG_TIDY_CHAIN: &[InstallMethod] = &[
  InstallMethod::Apt("clang-tidy"),
  InstallMethod::Brew("llvm"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

const GOOGLE_JAVA_FORMAT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("google-java-format"),
  InstallMethod::Npm("google-java-format"),
  InstallMethod::Pipx("google-java-format"),
];

const CHECKSTYLE_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("checkstyle"),
  InstallMethod::Apt("checkstyle"),
  InstallMethod::Npm("checkstyle"),
];

const RUSTFMT_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("rustfmt")];
const CLIPPY_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("clippy")];

const GOIMPORTS_CHAIN: &[InstallMethod] =
  &[InstallMethod::GoInstall("golang.org/x/tools/cmd/goimports")];

const GOLANGCI_LINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("golangci-lint"),
  InstallMethod::Scoop("golangci-lint"),
  InstallMethod::GoInstall(
    "github.com/golangci/golangci-lint/v2/cmd/golangci-lint",
  ),
  InstallMethod::GoInstall(
    "github.com/golangci/golangci-lint/cmd/golangci-lint",
  ),
];

// ktlint ships as a prebuilt executable jar; there is no cargo/npm fallback,
// so the chain is limited to system package managers (mirrors the
// CLANG_FORMAT_CHAIN / CLANG_TIDY_CHAIN pattern above).
const KTLINT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("ktlint"),
  InstallMethod::Scoop("ktlint"),
  InstallMethod::Npm("ktlint"),
  InstallMethod::Apt("ktlint"),
];

/// Looks up the ordered installer preference chain for a tool binary name.
/// This is the single place that maps a tool to its installers — adding a
/// new tool means adding a chain constant and one arm here, not copying a
/// whole if/else-if cascade.
pub(super) fn install_chain_for(
  binary: &str,
) -> Option<&'static [InstallMethod]> {
  match binary {
    "taplo" => Some(TAPLO_CHAIN),
    "typstyle" => Some(TYPSTYLE_CHAIN),
    "tinymist" => Some(TINYMIST_CHAIN),
    "ruff" => Some(RUFF_CHAIN),
    "prettier" => Some(PRETTIER_CHAIN),
    "biome" => Some(BIOME_CHAIN),
    "markdownlint-cli2" | "markdownlint" => Some(MARKDOWNLINT_CHAIN),
    "yamllint" => Some(YAMLLINT_CHAIN),
    "clang-format" => Some(CLANG_FORMAT_CHAIN),
    "clang-tidy" => Some(CLANG_TIDY_CHAIN),
    "google-java-format" => Some(GOOGLE_JAVA_FORMAT_CHAIN),
    "checkstyle" => Some(CHECKSTYLE_CHAIN),
    "rustfmt" => Some(RUSTFMT_CHAIN),
    "clippy" | "clippy-driver" => Some(CLIPPY_CHAIN),
    "goimports" => Some(GOIMPORTS_CHAIN),
    "golangci-lint" => Some(GOLANGCI_LINT_CHAIN),
    "ktlint" => Some(KTLINT_CHAIN),
    _ => None,
  }
}

static BINARY_CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

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

#[cfg(test)]
#[path = "tooling_tests.rs"]
mod tests;
