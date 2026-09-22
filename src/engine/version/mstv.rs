//! Minimum Supported Tool Version (MSTV) registry: per-tool minimum
//! versions and version-probing metadata.

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
/// MSTV for ktlint.
pub const MSTV_KTLINT: Version = Version::new(1, 0, 0);
/// MSTV for gofmt.
pub const MSTV_GOFMT: Version = Version::new(1, 18, 0);
/// MSTV for golangci-lint.
pub const MSTV_GOLANGCI_LINT: Version = Version::new(1, 50, 0);

/// How an argument to a [`VersionProbe::ViaBinary`] command is supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeArg {
  /// Literal string argument passed directly.
  Literal(&'static str),
  /// The resolved path of the tool binary being probed.
  ToolPath,
}

/// How the raw version string is extracted from the probe command's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeExtractor {
  /// First line carrying a plausibly version-shaped token.
  FirstVersionishLine,
  /// Go module version extracted from the `mod` line of `go version -m <path>`.
  GoModuleVersion,
}

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
  /// one of its own, or whose version is queried via an external inspector
  /// (such as `go version -m <path>`).
  ViaBinary {
    /// The binary to execute in the tool's place.
    bin: &'static str,
    /// Arguments passed to `bin`.
    args: &'static [ProbeArg],
    /// How to extract the version string from the command output.
    extractor: ProbeExtractor,
  },
  /// Try each probe in order, taking the first that yields a version — for a
  /// tool reachable under more than one distribution shape, or answering to
  /// more than one flag.
  ///
  /// "Yields a version" means version-shaped output, not a zero exit status
  /// (Fixes #176): a link that prints a real version and then exits non-zero
  /// answers the chain rather than falling through to the next link. Falling
  /// through still happens for the cases the chains here are built on — a
  /// binary that cannot be spawned, and a command that prints nothing
  /// version-shaped.
  FirstOf(&'static [VersionProbe]),
}

/// The probe shared by every tool that reports its own version conventionally,
/// and the assumption made for a binary with no registry entry at all: the
/// long flag, then the short one for tools that only answer to that.
pub const DEFAULT_VERSION_PROBE: VersionProbe = VersionProbe::FirstOf(&[
  VersionProbe::OwnFlags(&["--version"]),
  VersionProbe::OwnFlags(&["-v"]),
]);

/// Minimum Supported Tool Version entry with metadata and version-probing
/// strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMstvEntry {
  /// Name of the binary executable.
  pub binary: &'static str,
  /// Declared MSTV minimum required version, if one is enforced.
  pub min_version: Option<Version>,
  /// How this tool's version string is obtained.
  pub probe: VersionProbe,
}

/// Registry table of all declared Minimum Supported Tool Version entries.
pub const TOOL_MSTV_REGISTRY: &[ToolMstvEntry] = &[
  ToolMstvEntry {
    binary: "rustfmt",
    min_version: Some(MSTV_RUSTFMT),
    // To upgrade an already-installed toolchain component past this floor,
    // `rustup update` — not a fresh `rustup component add` — is the real
    // move; the install chain (`ALL_CHAINS`, `src/surfaces/tooling.rs`)
    // doesn't express that distinction.
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "clippy",
    min_version: Some(MSTV_CLIPPY),
    // Rustup ships no `clippy` binary: the component is reachable as the
    // `clippy-driver` shim, or through `cargo clippy`. Try both, in that
    // order.
    //
    // As with rustfmt above, `rustup update` — not a fresh `rustup
    // component add` — is how an existing install is actually upgraded
    // past this floor.
    probe: VersionProbe::FirstOf(&[
      VersionProbe::ViaBinary {
        bin: "clippy-driver",
        args: &[ProbeArg::Literal("--version")],
        extractor: ProbeExtractor::FirstVersionishLine,
      },
      VersionProbe::ViaBinary {
        bin: "cargo",
        args: &[ProbeArg::Literal("clippy"), ProbeArg::Literal("--version")],
        extractor: ProbeExtractor::FirstVersionishLine,
      },
    ]),
  },
  ToolMstvEntry {
    binary: "ruff",
    min_version: Some(MSTV_RUFF),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "clang-format",
    min_version: Some(MSTV_CLANG_FORMAT),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "clang-tidy",
    min_version: Some(MSTV_CLANG_TIDY),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "prettier",
    min_version: Some(MSTV_PRETTIER),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "taplo",
    min_version: Some(MSTV_TAPLO),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "markdownlint-cli2",
    min_version: Some(MSTV_MARKDOWNLINT_CLI2),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "typstyle",
    min_version: Some(MSTV_TYPSTYLE),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "yamllint",
    min_version: Some(MSTV_YAMLLINT),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "biome",
    min_version: Some(MSTV_BIOME),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "checkstyle",
    min_version: Some(MSTV_CHECKSTYLE),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "ktlint",
    min_version: Some(MSTV_KTLINT),
    probe: DEFAULT_VERSION_PROBE,
  },
  ToolMstvEntry {
    binary: "gofmt",
    min_version: Some(MSTV_GOFMT),
    // `gofmt` has no version flag; it ships with the Go toolchain and
    // carries that toolchain's version, which only `go version` reports
    // (Fixes #114). With `go` absent the probe yields nothing and the tool
    // reports `(version unprobeable)` — never scraped `gofmt` usage text.
    //
    // `gofmt` ships inside the Go toolchain rather than through a package
    // manager of its own, so it has no `ALL_CHAINS` row: upgrading it means
    // updating the Go toolchain itself, via https://go.dev/dl/.
    probe: VersionProbe::ViaBinary {
      bin: "go",
      args: &[ProbeArg::Literal("version")],
      extractor: ProbeExtractor::FirstVersionishLine,
    },
  },
  ToolMstvEntry {
    binary: "goimports",
    // `goimports` has no version flag; its module version is reported by
    // `go version -m <path>` from the `mod` line (Fixes #178).
    // Note: the version reported is the golang.org/x/tools module version
    // that goimports was built from, not goimports' own release version.
    // No MSTV floor is enforced against it today.
    min_version: None,
    probe: VersionProbe::ViaBinary {
      bin: "go",
      args: &[
        ProbeArg::Literal("version"),
        ProbeArg::Literal("-m"),
        ProbeArg::ToolPath,
      ],
      extractor: ProbeExtractor::GoModuleVersion,
    },
  },
  ToolMstvEntry {
    // A bare `version` subcommand, not a flag: `golangci-lint --version` is
    // not recognised. This is the whole probe — no `-v` behind it, because
    // the entry is what runs.
    //
    // Release notes for upgrading an existing install: https://golangci-lint.run
    binary: "golangci-lint",
    min_version: Some(MSTV_GOLANGCI_LINT),
    probe: VersionProbe::OwnFlags(&["version"]),
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
