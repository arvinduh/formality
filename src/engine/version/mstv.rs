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

/// Upgrade advice for a tool that has fallen below its MSTV floor.
///
/// Deliberately distinct from `install_hint_for` (`src/surfaces/tooling.rs`,
/// #264): that function renders `ALL_CHAINS`, the answer to "how do I
/// install this tool for the first time". This type answers a narrower
/// question -- "how do I move an *already-installed* copy of this tool past
/// its MSTV floor" -- which for a few rows is a genuinely different action
/// (`rustup update`, not a fresh `rustup component add`; bumping a Go
/// toolchain in place rather than reinstalling a CLI).
///
/// #294: for every row *except* those, the honest answer to "how do I
/// upgrade" is exactly the same command as "how do I install" -- so the
/// install portion is always [`MstvAdvice::Derived`], sourced from the one
/// `ALL_CHAINS` table `install_hint_for` already renders, never a second
/// hand-copied string. That is what let this table drift the way #264 was
/// filed over: this entry's `taplo` row used to lead with `cargo binstall`
/// after `TAPLO_CHAIN` was deliberately reordered npm-first, and this
/// entry's `goimports` row used to pin `@latest` while `GOIMPORTS_CHAIN`
/// pins an exact `@v0.49.0` -- two more copies of the same fact, silently
/// disagreeing with the source of truth. [`MstvAdvice::Bespoke`] exists only
/// for a binary with no `ALL_CHAINS` row at all to derive from (`gofmt`,
/// which ships inside the Go toolchain rather than through a package
/// manager of its own) -- `tests/registry_agreement.rs` fails if a new
/// entry reaches for `Bespoke` on a binary that *does* have a chain row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MstvAdvice {
  /// Render `tooling::install_hint_for(binary)` and, when `upgrade_note` is
  /// present, append it as a parenthetical -- the one piece of guidance
  /// `ALL_CHAINS` cannot express on its own, because it is about upgrading
  /// an *existing* install rather than performing a fresh one.
  Derived {
    /// Bespoke upgrade-specific text with no `ALL_CHAINS` equivalent,
    /// appended after the derived install hint.
    upgrade_note: Option<&'static str>,
  },
  /// Fully bespoke upgrade prose for a binary with no `ALL_CHAINS` row to
  /// derive from.
  Bespoke(&'static str),
}

/// Minimum Supported Tool Version entry with metadata, version-probing
/// strategy, and upgrade advice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolMstvEntry {
  /// Name of the binary executable.
  pub binary: &'static str,
  /// Declared MSTV minimum required version, if one is enforced.
  pub min_version: Option<Version>,
  /// How this tool's version string is obtained.
  pub probe: VersionProbe,
  /// Upgrade advice shown when the tool is outdated. See [`MstvAdvice`].
  pub advice: MstvAdvice,
}

impl ToolMstvEntry {
  /// Renders this entry's upgrade advice as a display string: the
  /// `ALL_CHAINS`-derived install hint (plus any bespoke upgrade note) for
  /// [`MstvAdvice::Derived`], or the bespoke prose verbatim for
  /// [`MstvAdvice::Bespoke`].
  #[must_use]
  pub fn advice_text(&self) -> String {
    match self.advice {
      MstvAdvice::Derived { upgrade_note: None } => {
        crate::surfaces::tooling::install_hint_for(self.binary)
      }
      MstvAdvice::Derived {
        upgrade_note: Some(note),
      } => format!(
        "{} (or {note})",
        crate::surfaces::tooling::install_hint_for(self.binary)
      ),
      MstvAdvice::Bespoke(text) => text.to_string(),
    }
  }
}

/// Registry table of all declared Minimum Supported Tool Version entries.
pub const TOOL_MSTV_REGISTRY: &[ToolMstvEntry] = &[
  ToolMstvEntry {
    binary: "rustfmt",
    min_version: Some(MSTV_RUSTFMT),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived {
      upgrade_note: Some(
        "'rustup update' to upgrade an already-installed toolchain \
         component, rather than adding it fresh",
      ),
    },
  },
  ToolMstvEntry {
    binary: "clippy",
    min_version: Some(MSTV_CLIPPY),
    // Rustup ships no `clippy` binary: the component is reachable as the
    // `clippy-driver` shim, or through `cargo clippy`. Try both, in that
    // order.
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
    advice: MstvAdvice::Derived {
      upgrade_note: Some(
        "'rustup update' to upgrade an already-installed toolchain \
         component, rather than adding it fresh",
      ),
    },
  },
  ToolMstvEntry {
    binary: "ruff",
    min_version: Some(MSTV_RUFF),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "clang-format",
    min_version: Some(MSTV_CLANG_FORMAT),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "clang-tidy",
    min_version: Some(MSTV_CLANG_TIDY),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "prettier",
    min_version: Some(MSTV_PRETTIER),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "taplo",
    min_version: Some(MSTV_TAPLO),
    probe: DEFAULT_VERSION_PROBE,
    // npm-first, matching TAPLO_CHAIN's own deliberate order (see
    // src/surfaces/tooling.rs's comment above TAPLO_CHAIN): cargo-binstall
    // has no prebuilt for this pin and falls through to a slow source
    // build, so it is demoted below npm there. This string used to lead
    // with `cargo binstall` and disagree with that order -- the exact
    // headline symptom #264 was filed over, reproduced here on the MSTV
    // upgrade-advice path (Fixes #264).
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "markdownlint-cli2",
    min_version: Some(MSTV_MARKDOWNLINT_CLI2),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "typstyle",
    min_version: Some(MSTV_TYPSTYLE),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "yamllint",
    min_version: Some(MSTV_YAMLLINT),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "biome",
    min_version: Some(MSTV_BIOME),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "checkstyle",
    min_version: Some(MSTV_CHECKSTYLE),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "ktlint",
    min_version: Some(MSTV_KTLINT),
    probe: DEFAULT_VERSION_PROBE,
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    binary: "gofmt",
    min_version: Some(MSTV_GOFMT),
    // `gofmt` has no version flag; it ships with the Go toolchain and
    // carries that toolchain's version, which only `go version` reports
    // (Fixes #114). With `go` absent the probe yields nothing and the tool
    // reports `(version unprobeable)` — never scraped `gofmt` usage text.
    probe: VersionProbe::ViaBinary {
      bin: "go",
      args: &[ProbeArg::Literal("version")],
      extractor: ProbeExtractor::FirstVersionishLine,
    },
    advice: MstvAdvice::Bespoke("Update Go toolchain via https://go.dev/dl/"),
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
    advice: MstvAdvice::Derived { upgrade_note: None },
  },
  ToolMstvEntry {
    // A bare `version` subcommand, not a flag: `golangci-lint --version` is
    // not recognised. This is the whole probe — no `-v` behind it, because
    // the entry is what runs.
    binary: "golangci-lint",
    min_version: Some(MSTV_GOLANGCI_LINT),
    probe: VersionProbe::OwnFlags(&["version"]),
    advice: MstvAdvice::Derived {
      upgrade_note: Some("https://golangci-lint.run for release notes"),
    },
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
