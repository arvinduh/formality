//! Tool version detection, split into two layers with one explicit crossing:
//!
//! - **Extraction (custom, kept):** `probe_raw_tool_version_uncached` and
//!   `classify_token` scrape a version out of frequently-non-semver CLI output
//!   (`go version go1.27.0 ...`, `0.44`, `18.1.8-0ubuntu1~22.04.1`,
//!   `1.35.1.post1`). A non-semver suffix or 4th component is salvaged down to
//!   the bare `MAJOR.MINOR.PATCH`; a malformed core (`01.2.3`) is not.
//! - **Comparison (`semver`-backed):** [`Version`] ordering/precedence and the
//!   MSTV "installed >= minimum" check are delegated to the `semver` crate via
//!   the sole crossing, [`Version::to_semver`].
//!
//! A version-shaped token that is invalid semver even bare surfaces as
//! [`ToolStatus::UnknownVersion`] — never a silently-satisfied MSTV.

/// Minimum Supported Tool Version (MSTV) definitions and registry.
pub mod mstv;

pub use mstv::{
  MSTV_BIOME, MSTV_CHECKSTYLE, MSTV_CLANG_FORMAT, MSTV_CLANG_TIDY, MSTV_CLIPPY,
  MSTV_GOFMT, MSTV_GOLANGCI_LINT, MSTV_KTFMT, MSTV_KTLINT,
  MSTV_MARKDOWNLINT_CLI2, MSTV_PRETTIER, MSTV_RUFF, MSTV_RUSTFMT, MSTV_TAPLO,
  MSTV_TYPSTYLE, MSTV_YAMLLINT, TOOL_MSTV_REGISTRY, ToolMstvEntry,
  all_mstv_entries, get_tool_mstv_entry,
};

use crate::surfaces::create_tool_command;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Cache TTL for probed tool versions: 24 hours.
pub const TOOL_VERSION_CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// An on-disk cache entry recording the probed version output and metadata for a tool binary.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ToolVersionEntry {
  /// The raw stdout/stderr output banner obtained from the tool.
  pub raw_version: String,
  /// Timestamp (seconds since UNIX epoch) when this entry was probed.
  pub last_checked_unix: u64,
  /// File modification timestamp (seconds since UNIX epoch) of the binary at probe time.
  pub binary_mtime_unix: u64,
  /// Absolute or resolved path to the tool binary executable at probe time.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub binary_path: Option<String>,
}

/// The collection of cached tool versions stored in `tool_versions.json`.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct ToolVersionStore {
  /// Map of tool binary names to their cached version entry.
  #[serde(default)]
  pub tools: BTreeMap<String, ToolVersionEntry>,
}

/// Returns the full path to the `tool_versions.json` cache file in formality's cache directory.
#[must_use]
pub fn get_tool_versions_cache_path() -> PathBuf {
  crate::engine::cache_path("tool_versions.json")
}

/// Reads and deserializes the tool versions cache from the given path.
/// Returns `None` if the file cannot be read or parsed.
#[must_use]
pub fn read_tool_version_cache_at(path: &Path) -> Option<ToolVersionStore> {
  let data = std::fs::read_to_string(path).ok()?;
  serde_json::from_str(&data).ok()
}

/// Serializes and writes the tool versions cache to the given path.
/// Creates parent directories as needed and ignores I/O errors to ensure resilient operation.
pub fn write_tool_version_cache_at(path: &Path, store: &ToolVersionStore) {
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_string_pretty(store) {
    let _ = std::fs::write(path, json);
  }
}

/// Resolves the binary path on PATH and retrieves its file modification timestamp (`mtime`).
#[must_use]
pub fn resolve_binary_info(binary: &str) -> Option<(PathBuf, u64)> {
  let path = if matches!(binary, "clippy" | "clippy-driver" | "cargo-clippy") {
    which::which("clippy-driver")
      .or_else(|_| which::which("cargo"))
      .or_else(|_| which::which(binary))
      .ok()?
  } else {
    which::which(binary).ok()?
  };

  let mtime = std::fs::metadata(&path)
    .and_then(|m| m.modified())
    .ok()
    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    .map_or(0, |d| d.as_secs());

  Some((path, mtime))
}

/// First line of `text` that carries a plausibly version-shaped token —
/// judged by [`classify_token`], the same strict predicate the parse path
/// uses (#137), not "contains an ASCII digit". A line that merely has a
/// number in it (`gofmt`'s `-e  report all errors (not just the first 10 on
/// different lines)` usage text) is not a version and must never be scraped
/// as one. `None` when no line carries such a token.
fn first_versionish_line(text: &str) -> Option<String> {
  text
    .lines()
    .find(|l| line_carries_version_token(l))
    .map(|l| l.trim().to_string())
}

/// Whether any whitespace-separated token on `line` is version-shaped under
/// [`classify_token`]: a real `MAJOR.MINOR[.PATCH]` core (with an optional
/// `v`/`go` marker and semver suffix), either parsed clean (`Ok`) or
/// version-shaped-but-malformed (`Rejected`, e.g. a leading-zero core). A
/// line with neither — help text, a bare option list — is not versionish.
fn line_carries_version_token(line: &str) -> bool {
  line.split_whitespace().any(|tok| {
    matches!(classify_token(tok), TokenParse::Ok(_) | TokenParse::Rejected)
  })
}

/// `gofmt` has no version flag of its own — it ships with the Go toolchain
/// and carries that toolchain's version, which only `go version` reports
/// (`go version go1.27.0 windows/amd64`). Probe that explicitly instead of
/// letting a failed `gofmt --version` fall through to scraping its usage
/// text. `None` when `go` is not on PATH or the call fails — the caller then
/// reports `(version unprobeable)`, never scraped help text (Fixes #114).
fn probe_gofmt_version_via_go_toolchain() -> Option<String> {
  let output = create_tool_command("go").arg("version").output().ok()?;
  if !output.status.success() {
    return None;
  }
  first_versionish_line(&String::from_utf8_lossy(&output.stdout))
}

/// Executes the tool binary with `--version` or `-v` uncached and extracts the raw output line.
#[must_use]
pub fn probe_raw_tool_version_uncached(binary: &str) -> Option<String> {
  // `gofmt` is sourced from the Go toolchain, not its own (nonexistent)
  // `--version` flag — see [`probe_gofmt_version_via_go_toolchain`].
  if binary == "gofmt" {
    return probe_gofmt_version_via_go_toolchain();
  }

  let output = if binary == "clippy" {
    if let Ok(out) = create_tool_command("clippy-driver")
      .arg("--version")
      .output()
    {
      if out.status.success() {
        Some(out)
      } else {
        create_tool_command("cargo")
          .args(["clippy", "--version"])
          .output()
          .ok()
      }
    } else {
      create_tool_command("cargo")
        .args(["clippy", "--version"])
        .output()
        .ok()
    }
  } else {
    let args = tool_version_args(binary).unwrap_or(&["--version"]);
    create_tool_command(binary).args(args).output().ok()
  }?;

  if output.status.success() {
    if let Some(v) =
      first_versionish_line(&String::from_utf8_lossy(&output.stdout))
    {
      return Some(v);
    }
    if let Some(v) =
      first_versionish_line(&String::from_utf8_lossy(&output.stderr))
    {
      return Some(v);
    }
  }

  // Fallback for tools expecting -v or -version
  if let Ok(output_v) = create_tool_command(binary).arg("-v").output()
    && output_v.status.success()
    && let Some(v) =
      first_versionish_line(&String::from_utf8_lossy(&output_v.stdout))
  {
    return Some(v);
  }

  None
}

/// Retrieve the raw output line from executing the tool with `--version` or `-v`,
/// checking the on-disk cache at `cache_path` first.
/// If cached version is fresh (TTL valid) and binary modification time matches,
/// returns the cached version string without spawning a subprocess.
/// Otherwise, invokes the tool CLI, updates the cache, and returns the result.
#[must_use]
pub fn get_raw_tool_version_at(
  binary: &str,
  cache_path: &Path,
) -> Option<String> {
  let bin_info = resolve_binary_info(binary);

  if std::env::var("FORMALITY_NO_VERSION_CACHE").is_err()
    && let Some((ref bin_path, bin_mtime)) = bin_info
    && let Some(store) = read_tool_version_cache_at(cache_path)
    && let Some(entry) = store.tools.get(binary)
  {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_or(0, |d| d.as_secs());

    let is_fresh =
      now.saturating_sub(entry.last_checked_unix) < TOOL_VERSION_CACHE_TTL_SECS;
    let mtime_matches = entry.binary_mtime_unix == bin_mtime;
    let path_matches = entry
      .binary_path
      .as_deref()
      .is_none_or(|p| p == bin_path.to_string_lossy().as_ref());

    if is_fresh && mtime_matches && path_matches {
      return Some(entry.raw_version.clone());
    }
  }

  let raw = probe_raw_tool_version_uncached(binary)?;

  if let Some((ref bin_path, bin_mtime)) = bin_info {
    let mut store = read_tool_version_cache_at(cache_path).unwrap_or_default();
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_or(0, |d| d.as_secs());

    store.tools.insert(
      binary.to_string(),
      ToolVersionEntry {
        raw_version: raw.clone(),
        last_checked_unix: now,
        binary_mtime_unix: bin_mtime,
        binary_path: Some(bin_path.to_string_lossy().to_string()),
      },
    );
    write_tool_version_cache_at(cache_path, &store);
  }

  Some(raw)
}

/// Retrieve the raw output line from executing the tool with `--version` or `-v`,
/// using the default on-disk cache store (`tool_versions.json`) in `cache_dir()`.
#[must_use]
pub fn get_raw_tool_version(binary: &str) -> Option<String> {
  get_raw_tool_version_at(binary, &get_tool_versions_cache_path())
}

/// Probe a tool's version at an explicit cache path.
#[must_use]
pub fn probe_tool_version_at(
  binary: &str,
  cache_path: &Path,
) -> Option<Version> {
  let raw_output = get_raw_tool_version_at(binary, cache_path)?;
  normalize_probed_version(binary, &raw_output)
}

/// Probe a tool's version by invoking its CLI (`--version` / `-v`) and parsing the output,
/// checking the on-disk cache store before spawning subprocesses.
#[must_use]
pub fn probe_tool_version(binary: &str) -> Option<Version> {
  probe_tool_version_at(binary, &get_tool_versions_cache_path())
}

/// A version *scraped* from a tool's `--version` banner. Owns no ordering
/// logic of its own: comparison, precedence and the MSTV check are delegated
/// to `semver` via [`Version::to_semver`].
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Version {
  /// Major version component.
  pub major: u64,
  /// Minor version component.
  pub minor: u64,
  /// Patch version component.
  pub patch: u64,
  /// Optional prerelease metadata string.
  pub prerelease: Option<String>,
}

impl Version {
  /// Create a new `Version` without prerelease metadata.
  #[must_use]
  pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
    Self {
      major,
      minor,
      patch,
      prerelease: None,
    }
  }

  /// Create a `Version` with prerelease metadata. The prerelease must be a
  /// valid SemVer identifier (what the parse path always yields); one `semver`
  /// rejects still constructs but makes [`Version::to_semver`] lossy, so
  /// ordering stops matching structural equality — a `debug_assert` catches it.
  pub fn with_prerelease(
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: impl Into<String>,
  ) -> Self {
    let prerelease = prerelease.into();
    debug_assert!(
      semver::Prerelease::new(&prerelease).is_ok(),
      "invalid SemVer prerelease identifier {prerelease:?}"
    );
    Self {
      major,
      minor,
      patch,
      prerelease: Some(prerelease),
    }
  }

  /// Parse a version string directly, or extract one from a tool banner.
  /// `None` when there is no version, or when the only version-shaped token is
  /// malformed beyond salvage (e.g. leading-zero core `01.2.3`) — the caller
  /// then surfaces [`ToolStatus::UnknownVersion`], not a fabricated number.
  #[must_use]
  pub fn parse(input: &str) -> Option<Self> {
    let trimmed = input.trim();
    match classify_token(trimmed) {
      TokenParse::Ok(v) => Some(v),
      TokenParse::Rejected => None,
      TokenParse::NotVersion => Self::extract(trimmed),
    }
  }

  /// Scan a multi-token banner. The first *version-shaped* token decides the
  /// result: if valid it wins; if malformed beyond salvage the scan aborts
  /// with `None` rather than walking on to a later, unrelated number.
  #[must_use]
  pub fn extract(input: &str) -> Option<Self> {
    for token in input.split_whitespace() {
      match classify_token(token) {
        TokenParse::Ok(v) => return Some(v),
        TokenParse::Rejected => return None,
        TokenParse::NotVersion => {}
      }
    }
    None
  }
}

// === Extraction / scraping layer (custom domain code — kept, not delegated) ==
// `semver::Version::parse` can't be pointed straight at tool output, so this
// layer scrapes a `MAJOR.MINOR.PATCH` core out of the token and hands that to
// `semver` for the real parse. No ordering semantics live here.

enum TokenParse {
  /// Parsed — strict (suffix preserved) or salvaged to the bare `M.M.P` core.
  Ok(Version),
  /// Not version-shaped: keep scanning.
  NotVersion,
  /// Version-shaped but invalid semver even bare (leading-zero core): abort.
  Rejected,
}

/// Read a version out of one token. Strips surrounding punctuation and a
/// leading `v`/`go` marker, then tries, in order: the 3-part core plus any
/// real `-pre`/`+build` suffix (keeps `1.7.0-nightly`, `14.0.0-1ubuntu1`);
/// then the bare core alone, dropping a `-`/`+` suffix or a clean 4th component
/// `semver` rejects (`18.1.8-0ubuntu1~22.04.1`, `1.35.1.post1`, `0.9.6.dev0` —
/// which the pre-`semver` parser also ignored). A non-numeric 3rd component
/// (`0.9.6rc1`, `1.2.x`) is rejected, never zeroed.
fn classify_token(token: &str) -> TokenParse {
  let cleaned = token.trim_matches(|c: char| "()[]{}<>\"',:;".contains(c));

  // Strip a leading `v`/`V`/`go`/`Go` marker, kept only if a digit follows.
  let s = ["v", "V", "go", "Go"]
    .iter()
    .find_map(|p| cleaned.strip_prefix(p))
    .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    .unwrap_or(cleaned);
  if !s.starts_with(|c: char| c.is_ascii_digit()) {
    return TokenParse::NotVersion;
  }

  let (numeric_zone, suffix) =
    s.split_at(s.find(['-', '+']).unwrap_or(s.len()));
  let comps: Vec<&str> = numeric_zone.split('.').collect();
  let numeric =
    |c: &&str| !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit());
  // Version-shaped: 2..=4 dot components, first two plain integers. Otherwise
  // it is just a digit-leading token (git short-hash, date fragment).
  if !(2..=4).contains(&comps.len()) || !comps[..2].iter().all(numeric) {
    return TokenParse::NotVersion;
  }
  // Patch = a plain-integer 3rd component (any 4th is dropped). A non-numeric
  // 3rd can't become a patch without fabricating one, so reject rather than
  // zero it. Rebuild from original text so a leading-zero core still fails.
  let core = match comps.get(2) {
    Some(p) if numeric(p) => format!("{}.{}.{}", comps[0], comps[1], p),
    Some(_) => return TokenParse::Rejected,
    None => format!("{}.{}.0", comps[0], comps[1]),
  };

  let parsed = semver::Version::parse(&format!("{core}{suffix}"))
    .or_else(|_| semver::Version::parse(&core));
  match parsed {
    Ok(sv) => TokenParse::Ok(Version {
      major: sv.major,
      minor: sv.minor,
      patch: sv.patch,
      prerelease: (!sv.pre.is_empty()).then(|| sv.pre.as_str().to_string()),
    }),
    Err(_) => TokenParse::Rejected,
  }
}

// === Comparison layer (delegated wholesale to the `semver` crate) ===========

impl Version {
  /// The sole crossing from the extraction layer into `semver`-backed
  /// comparison. The parse path only stores `semver`-valid prereleases, so the
  /// `"0"` fallback is unreachable in practice; if a caller hand-builds an
  /// invalid one anyway it sorts below the matching release (rule 9) but two
  /// such strings then compare `Equal` while `PartialEq` sees them distinct —
  /// [`Version::with_prerelease`]'s `debug_assert` guards that.
  fn to_semver(&self) -> semver::Version {
    let pre = match self.prerelease.as_deref() {
      None | Some("") => semver::Prerelease::EMPTY,
      Some(p) => semver::Prerelease::new(p)
        .unwrap_or_else(|_| semver::Prerelease::new("0").unwrap()),
    };
    semver::Version {
      major: self.major,
      minor: self.minor,
      patch: self.patch,
      pre,
      build: semver::BuildMetadata::EMPTY,
    }
  }
}

impl Ord for Version {
  /// Delegated to `semver` (precedence rules 9-11, prerelease chain included).
  fn cmp(&self, other: &Self) -> Ordering {
    self.to_semver().cmp(&other.to_semver())
  }
}

impl PartialOrd for Version {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl fmt::Display for Version {
  /// Rendered by `semver`, so the text matches what the comparison layer sees.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.to_semver())
  }
}

impl FromStr for Version {
  type Err = String;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Self::parse(s).ok_or_else(|| format!("Invalid semantic version: '{s}'"))
  }
}

/// Returns the Minimum Supported Tool Version (MSTV) for a given tool binary, if defined.
#[must_use]
pub fn minimum_supported_tool_version(binary: &str) -> Option<Version> {
  get_tool_mstv_entry(binary).map(|e| e.min_version.clone())
}

/// Retrieve version query arguments for a tool binary.
#[must_use]
pub fn tool_version_args(binary: &str) -> Option<&'static [&'static str]> {
  get_tool_mstv_entry(binary).map(|e| e.version_args)
}

/// Normalize a raw version output string probed from a tool into a semver [`Version`],
/// applying tool-specific version remappings (such as Clippy `0.1.x -> 1.x.0`).
#[must_use]
pub fn normalize_probed_version(binary: &str, raw: &str) -> Option<Version> {
  let mut ver = Version::extract(raw)?;

  // Clippy 0.1.X corresponds to Rust toolchain 1.X.0
  if (binary == "clippy"
    || binary == "clippy-driver"
    || binary == "cargo-clippy")
    && ver.major == 0
    && ver.minor == 1
  {
    ver = Version {
      major: 1,
      minor: ver.patch,
      patch: 0,
      prerelease: ver.prerelease,
    };
  }

  Some(ver)
}

/// Status of a tool relative to its minimum required version.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ToolStatus {
  /// Installed tool version satisfies or exceeds minimum supported tool version.
  Compatible {
    /// Currently installed version.
    current: Version,
    /// Minimum required version.
    minimum: Version,
  },
  /// Installed tool version is below minimum supported tool version.
  Outdated {
    /// Currently installed version.
    current: Version,
    /// Minimum required version.
    minimum: Version,
  },
  /// Tool binary was not found on PATH.
  NotFound,
  /// Tool version string could not be parsed into semver.
  UnknownVersion(String),
  /// Installed tool version is present, executable, and at/above the MSTV
  /// floor, but does not match the exact version `fml install` pins for
  /// this tool (`src/surfaces/tooling.rs`'s install chains) — e.g. a stale
  /// system-wide install that predates the pin. Distinct from `Outdated`:
  /// an `Outdated` tool may not even work; a `Stale` one works, it's just
  /// not the bit-for-bit version CI will run, so its formatting/linting
  /// output can silently disagree with CI's.
  Stale {
    /// Currently installed version.
    current: Version,
    /// Exact version `fml install` pins this tool to.
    pinned: Version,
  },
}

impl ToolStatus {
  /// Returns `true` if tool status is [`ToolStatus::Compatible`].
  #[must_use]
  pub fn is_compatible(&self) -> bool {
    matches!(self, ToolStatus::Compatible { .. })
  }

  /// Returns `true` if tool status is [`ToolStatus::Outdated`].
  #[must_use]
  pub fn is_outdated(&self) -> bool {
    matches!(self, ToolStatus::Outdated { .. })
  }

  /// Returns `true` if tool status is [`ToolStatus::NotFound`].
  #[must_use]
  pub fn is_not_found(&self) -> bool {
    matches!(self, ToolStatus::NotFound)
  }

  /// Returns `true` if tool status is [`ToolStatus::UnknownVersion`].
  #[must_use]
  pub fn is_unknown_version(&self) -> bool {
    matches!(self, ToolStatus::UnknownVersion(_))
  }

  /// Returns `true` if tool status is [`ToolStatus::Stale`].
  #[must_use]
  pub fn is_stale(&self) -> bool {
    matches!(self, ToolStatus::Stale { .. })
  }
}

impl fmt::Display for ToolStatus {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      ToolStatus::Compatible { current, minimum } => {
        write!(f, "Compatible ({current} >= MSTV {minimum})")
      }
      ToolStatus::Outdated { current, minimum } => {
        write!(f, "Outdated ({current} < MSTV {minimum})")
      }
      ToolStatus::NotFound => write!(f, "Not Found"),
      ToolStatus::UnknownVersion(raw) => {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
          write!(f, "Unknown Version (probe failed)")
        } else {
          write!(f, "Unknown Version ({trimmed})")
        }
      }
      ToolStatus::Stale { current, pinned } => {
        write!(f, "Stale ({current} != pinned {pinned})")
      }
    }
  }
}

/// The MSTV-floor predicate — `current >= minimum` under [`semver::Version`]
/// ordering. (`semver::VersionReq` is avoided on purpose: its Cargo-style rule
/// that `>=1.4.0` never matches a prerelease would flip nightly tools to
/// Outdated.)
fn satisfies_minimum(current: &Version, minimum: &Version) -> bool {
  current.to_semver() >= minimum.to_semver()
}

/// Combines the MSTV-floor check and the exact-pin check into a single
/// status, given an already-probed current version and raw version banner
/// (callers that already have these from a prior probe pass them straight
/// through instead of re-spawning the tool's `--version` subprocess).
///
/// `minimum` and `pinned` are both optional and independent — a tool may
/// have an MSTV entry but no pin (or vice versa; see
/// `src/surfaces/tooling.rs`'s pinned-versions note for why some install
/// chains carry no inline version at all). Precedence when a tool trips both
/// checks: [`ToolStatus::Outdated`] (below the floor — may not even work)
/// wins over [`ToolStatus::Stale`] (present, above the floor, just not the
/// exact pin) — both are worse than [`ToolStatus::Compatible`].
#[must_use]
pub fn evaluate_tool_status(
  current: Option<Version>,
  raw_output: Option<String>,
  minimum: Option<&Version>,
  pinned: Option<&Version>,
) -> ToolStatus {
  match current {
    Some(curr) => {
      if let Some(min) = minimum
        && !satisfies_minimum(&curr, min)
      {
        return ToolStatus::Outdated {
          current: curr,
          minimum: min.clone(),
        };
      }
      if let Some(pin) = pinned
        && curr != *pin
      {
        return ToolStatus::Stale {
          current: curr,
          pinned: pin.clone(),
        };
      }
      ToolStatus::Compatible {
        current: curr.clone(),
        minimum: minimum.cloned().unwrap_or(curr),
      }
    }
    None => match raw_output {
      Some(raw) if !raw.trim().is_empty() => ToolStatus::UnknownVersion(raw),
      _ => ToolStatus::NotFound,
    },
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
