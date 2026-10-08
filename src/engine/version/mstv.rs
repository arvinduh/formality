//! How a tool's version is probed.
//!
//! Defines the probe strategies a tool declares in
//! `crate::surfaces::tooling::TOOLS`, alongside its install chain and
//! minimum supported version. Version scraping and semver comparison are
//! owned by `super`.

/// How an argument to a [`VersionProbe::ViaBinary`] command is supplied.
#[derive(Debug, PartialEq)]
pub enum ProbeArg {
  /// Literal string argument passed directly.
  Literal(&'static str),
  /// The resolved path of the tool binary being probed.
  ToolPath,
}

/// How the raw version string is extracted from the probe command's output.
#[derive(Debug, Clone, Copy, PartialEq)]
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
/// here, and `probe_raw_tool_version_uncached` executes whatever it finds
/// without knowing which tool it is looking at. Adding a tool in the same
/// situation is a registry entry, not another branch.
///
#[derive(Debug, Clone, Copy, PartialEq)]
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
