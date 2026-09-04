//! Minimum Supported Tool Version (MSTV) registry: per-tool minimum
//! versions, upgrade advice, and version-probing metadata.

use super::Version;

/// Minimum Supported Tool Version declarations for tools in the Formality fleet.
/// MSTV for rustfmt.
pub const MSTV_RUSTFMT: Version = Version::new(1, 4, 0);
/// MSTV for clippy.
pub const MSTV_CLIPPY: Version = Version::new(1, 65, 0);
/// MSTV for ruff.
pub const MSTV_RUFF: Version = Version::new(0, 1, 0);
/// MSTV for clang-format.
pub const MSTV_CLANG_FORMAT: Version = Version::new(14, 0, 0);
/// MSTV for clang-tidy.
pub const MSTV_CLANG_TIDY: Version = Version::new(14, 0, 0);
/// MSTV for prettier.
pub const MSTV_PRETTIER: Version = Version::new(2, 0, 0);
/// MSTV for taplo.
pub const MSTV_TAPLO: Version = Version::new(0, 8, 0);
/// MSTV for markdownlint-cli2.
pub const MSTV_MARKDOWNLINT_CLI2: Version = Version::new(0, 4, 0);
/// MSTV for typstyle.
pub const MSTV_TYPSTYLE: Version = Version::new(0, 11, 0);
/// MSTV for yamllint.
pub const MSTV_YAMLLINT: Version = Version::new(1, 20, 0);
/// MSTV for biome.
pub const MSTV_BIOME: Version = Version::new(1, 5, 0);
/// MSTV for checkstyle.
pub const MSTV_CHECKSTYLE: Version = Version::new(10, 0, 0);
/// MSTV for ktfmt.
pub const MSTV_KTFMT: Version = Version::new(0, 44, 0);
/// MSTV for ktlint.
pub const MSTV_KTLINT: Version = Version::new(1, 0, 0);
/// MSTV for gofmt.
pub const MSTV_GOFMT: Version = Version::new(1, 18, 0);
/// MSTV for golangci-lint.
pub const MSTV_GOLANGCI_LINT: Version = Version::new(1, 50, 0);

/// How a tool's version string is obtained.
///
/// This is registry *data*, not a special case inside the probing function: a
/// tool whose version does not come from its own `--version` declares that
/// here, and [`probe_raw_tool_version_uncached`] executes whatever it finds
/// without knowing which tool it is looking at. Adding a tool in the same
/// situation is a registry entry, not another branch.
///
/// [`probe_raw_tool_version_uncached`]: super::probe_raw_tool_version_uncached
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionProbe {
  /// Run the tool itself with exactly these flags — nothing more. A tool that
  /// also needs a second attempt declares it with [`VersionProbe::FirstOf`],
  /// so what runs is always what the entry says runs.
  OwnFlags(&'static [&'static str]),
  /// Run a *different* binary to learn this tool's version — for a tool that
  /// ships inside a toolchain and carries the toolchain's version rather than
  /// one of its own.
  ViaBinary {
    /// The binary to execute in the tool's place.
    bin: &'static str,
    /// Arguments passed to `bin`.
    args: &'static [&'static str],
  },
  /// Try each probe in order, taking the first that yields a version — for a
  /// tool reachable under more than one distribution shape, or answering to
  /// more than one flag.
  FirstOf(&'static [VersionProbe]),
}

/// The probe shared by every tool that reports its own version conventionally,
/// and the assumption made for a binary with no registry entry at all: the
/// long flag, then the short one for tools that only answer to that.
pub const DEFAULT_VERSION_PROBE: VersionProbe = VersionProbe::FirstOf(&[
  VersionProbe::OwnFlags(&["--version"]),
  VersionProbe::OwnFlags(&["-v"]),
]);

/// Minimum Supported Tool Version entry with metadata, version-probing
/// strategy, and upgrade advice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMstvEntry {
  /// Name of the binary executable.
  pub binary: &'static str,
  /// Declared MSTV minimum required version.
  pub min_version: Version,
  /// How this tool's version string is obtained.
  pub probe: VersionProbe,
  /// Upgrade advice message shown when tool is outdated.
  pub advice: &'static str,
}

/// Registry table of all declared Minimum Supported Tool Version entries.
pub const TOOL_MSTV_REGISTRY: &[ToolMstvEntry] = &[
  ToolMstvEntry {
    binary: "rustfmt",
    min_version: MSTV_RUSTFMT,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'rustup component add rustfmt' or 'rustup update'",
  },
  ToolMstvEntry {
    binary: "clippy",
    min_version: MSTV_CLIPPY,
    // Rustup ships no `clippy` binary: the component is reachable as the
    // `clippy-driver` shim, or through `cargo clippy`. Try both, in that
    // order.
    probe: VersionProbe::FirstOf(&[
      VersionProbe::ViaBinary {
        bin: "clippy-driver",
        args: &["--version"],
      },
      VersionProbe::ViaBinary {
        bin: "cargo",
        args: &["clippy", "--version"],
      },
    ]),
    advice: "Run 'rustup component add clippy' or 'rustup update'",
  },
  ToolMstvEntry {
    binary: "ruff",
    min_version: MSTV_RUFF,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'pip install -U ruff' or 'brew install ruff'",
  },
  ToolMstvEntry {
    binary: "clang-format",
    min_version: MSTV_CLANG_FORMAT,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Install clang-format >= 14 via system package manager or LLVM toolchain",
  },
  ToolMstvEntry {
    binary: "clang-tidy",
    min_version: MSTV_CLANG_TIDY,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Install clang-tidy >= 14 via system package manager or LLVM toolchain",
  },
  ToolMstvEntry {
    binary: "prettier",
    min_version: MSTV_PRETTIER,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'npm install -g prettier' or 'brew install prettier'",
  },
  ToolMstvEntry {
    binary: "taplo",
    min_version: MSTV_TAPLO,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'cargo binstall taplo-cli' or 'brew install taplo' or 'cargo install --locked taplo-cli'",
  },
  ToolMstvEntry {
    binary: "markdownlint-cli2",
    min_version: MSTV_MARKDOWNLINT_CLI2,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'npm install -g markdownlint-cli2' or 'brew install markdownlint-cli2'",
  },
  ToolMstvEntry {
    binary: "typstyle",
    min_version: MSTV_TYPSTYLE,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'cargo install --locked typstyle' or 'brew install typstyle'",
  },
  ToolMstvEntry {
    binary: "yamllint",
    min_version: MSTV_YAMLLINT,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'pip install -U yamllint' or 'brew install yamllint'",
  },
  ToolMstvEntry {
    binary: "biome",
    min_version: MSTV_BIOME,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'npm install -g @biomejs/biome' or 'brew install biome'",
  },
  ToolMstvEntry {
    binary: "checkstyle",
    min_version: MSTV_CHECKSTYLE,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'brew install checkstyle' or update your checkstyle jar",
  },
  ToolMstvEntry {
    binary: "ktfmt",
    min_version: MSTV_KTFMT,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'brew install ktfmt'",
  },
  ToolMstvEntry {
    binary: "ktlint",
    min_version: MSTV_KTLINT,
    probe: DEFAULT_VERSION_PROBE,
    advice: "Run 'brew install ktlint'",
  },
  ToolMstvEntry {
    binary: "gofmt",
    min_version: MSTV_GOFMT,
    // `gofmt` has no version flag; it ships with the Go toolchain and
    // carries that toolchain's version, which only `go version` reports
    // (Fixes #114). With `go` absent the probe yields nothing and the tool
    // reports `(version unprobeable)` — never scraped `gofmt` usage text.
    probe: VersionProbe::ViaBinary {
      bin: "go",
      args: &["version"],
    },
    advice: "Update Go toolchain via https://go.dev/dl/",
  },
  ToolMstvEntry {
    // A bare `version` subcommand, not a flag: `golangci-lint --version` is
    // not recognised. This is the whole probe — no `-v` behind it, because
    // the entry is what runs.
    binary: "golangci-lint",
    min_version: MSTV_GOLANGCI_LINT,
    probe: VersionProbe::OwnFlags(&["version"]),
    advice: "Run 'brew install golangci-lint' or update via https://golangci-lint.run",
  },
];

/// Returns the [`ToolMstvEntry`] for `binary` if declared in the registry.
#[must_use]
pub fn get_tool_mstv_entry(binary: &str) -> Option<&'static ToolMstvEntry> {
  let lookup_bin = match binary {
    "clippy-driver" | "cargo-clippy" => "clippy",
    other => other,
  };
  TOOL_MSTV_REGISTRY
    .iter()
    .find(|entry| entry.binary == lookup_bin)
}

/// Returns a slice of all declared [`ToolMstvEntry`] entries.
#[must_use]
pub fn all_mstv_entries() -> &'static [ToolMstvEntry] {
  TOOL_MSTV_REGISTRY
}
