//! Tool version detection: `SemVer` parsing, MSRV/MSTV compatibility tables,
//! and CLI version probing.

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
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Represents a Semantic Version (`SemVer`) with optional prerelease identifier.
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

  /// Create a new `Version` with prerelease metadata.
  pub fn with_prerelease(
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: impl Into<String>,
  ) -> Self {
    Self {
      major,
      minor,
      patch,
      prerelease: Some(prerelease.into()),
    }
  }

  /// Parse a version string directly or extract it from a tool output banner.
  #[must_use]
  pub fn parse(input: &str) -> Option<Self> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
      return None;
    }

    if let Some(v) = parse_single_token(trimmed) {
      return Some(v);
    }

    Self::extract(trimmed)
  }

  /// Extract the first valid semantic version from a multi-token text string.
  #[must_use]
  pub fn extract(input: &str) -> Option<Self> {
    for token in input.split_whitespace() {
      if let Some(v) = parse_single_token(token) {
        return Some(v);
      }
    }
    None
  }
}

fn parse_single_token(token: &str) -> Option<Version> {
  let cleaned = token.trim_matches(|c: char| {
    c == '('
      || c == ')'
      || c == '['
      || c == ']'
      || c == '{'
      || c == '}'
      || c == '<'
      || c == '>'
      || c == '"'
      || c == '\''
      || c == ','
      || c == ':'
      || c == ';'
  });

  if cleaned.is_empty() {
    return None;
  }

  // Strip leading 'v' or 'V' or 'go'/'Go' if immediately followed by a digit
  let s = if (cleaned.starts_with('v') || cleaned.starts_with('V'))
    && cleaned.len() > 1
    && cleaned.as_bytes()[1].is_ascii_digit()
  {
    &cleaned[1..]
  } else if (cleaned.starts_with("go") || cleaned.starts_with("Go"))
    && cleaned.len() > 2
    && cleaned.as_bytes()[2].is_ascii_digit()
  {
    &cleaned[2..]
  } else {
    cleaned
  };

  // Must start with an ASCII digit
  if !s.starts_with(|c: char| c.is_ascii_digit()) {
    return None;
  }

  // Strip build metadata after '+'
  let s_no_build = if let Some((base, _)) = s.split_once('+') {
    base
  } else {
    s
  };

  // Extract prerelease after '-'
  let (base, prerelease) = if let Some((ver, pre)) = s_no_build.split_once('-')
  {
    (ver, Some(pre.to_string()))
  } else {
    (s_no_build, None)
  };

  // Base must consist of 2 to 4 dot-separated integer components
  let parts: Vec<&str> = base.split('.').collect();
  if parts.len() < 2 || parts.len() > 4 {
    return None;
  }

  let major = parts[0].parse::<u64>().ok()?;
  let minor = parts[1].parse::<u64>().ok()?;
  let patch = if parts.len() >= 3 {
    parts[2].parse::<u64>().ok()?
  } else {
    0
  };

  Some(Version {
    major,
    minor,
    patch,
    prerelease,
  })
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
  let a_parts = a.split('.');
  let b_parts = b.split('.');

  for (p_a, p_b) in a_parts.zip(b_parts) {
    let ord = match (p_a.parse::<u64>(), p_b.parse::<u64>()) {
      (Ok(num_a), Ok(num_b)) => num_a.cmp(&num_b),
      (Ok(_), Err(_)) => Ordering::Less,
      (Err(_), Ok(_)) => Ordering::Greater,
      (Err(_), Err(_)) => p_a.cmp(p_b),
    };
    if ord != Ordering::Equal {
      return ord;
    }
  }

  a.split('.').count().cmp(&b.split('.').count())
}

impl Ord for Version {
  fn cmp(&self, other: &Self) -> Ordering {
    match (self.major, self.minor, self.patch).cmp(&(
      other.major,
      other.minor,
      other.patch,
    )) {
      Ordering::Equal => match (&self.prerelease, &other.prerelease) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(a), Some(b)) => compare_prerelease(a, b),
      },
      ord => ord,
    }
  }
}

impl PartialOrd for Version {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl fmt::Display for Version {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(ref pre) = self.prerelease {
      write!(f, "{}.{}.{}-{}", self.major, self.minor, self.patch, pre)
    } else {
      write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
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

/// Probe a tool's version by invoking its CLI (`--version` / `-v`) and parsing the output.
#[must_use]
pub fn probe_tool_version(binary: &str) -> Option<Version> {
  let raw_output = get_raw_tool_version(binary)?;
  normalize_probed_version(binary, &raw_output)
}

/// Retrieve the raw output line from executing the tool with `--version` or `-v`.
#[must_use]
pub fn get_raw_tool_version(binary: &str) -> Option<String> {
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

  if output.status.success() || (binary == "gofmt" && !output.stderr.is_empty())
  {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !stdout.trim().is_empty() {
      if let Some(line) = stdout
        .lines()
        .find(|l| l.chars().any(|c| c.is_ascii_digit()))
      {
        return Some(line.trim().to_string());
      }
      if let Some(first_line) = stdout.lines().find(|l| !l.trim().is_empty()) {
        return Some(first_line.trim().to_string());
      }
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !stderr.trim().is_empty() {
      if let Some(line) = stderr
        .lines()
        .find(|l| l.chars().any(|c| c.is_ascii_digit()))
      {
        return Some(line.trim().to_string());
      }
      if let Some(first_line) = stderr.lines().find(|l| !l.trim().is_empty()) {
        return Some(first_line.trim().to_string());
      }
    }
  }

  // Fallback for tools expecting -v or -version
  if let Ok(output_v) = create_tool_command(binary).arg("-v").output()
    && output_v.status.success()
  {
    let stdout = String::from_utf8_lossy(&output_v.stdout).to_string();
    if !stdout.trim().is_empty() {
      if let Some(line) = stdout
        .lines()
        .find(|l| l.chars().any(|c| c.is_ascii_digit()))
      {
        return Some(line.trim().to_string());
      }
      if let Some(first_line) = stdout.lines().find(|l| !l.trim().is_empty()) {
        return Some(first_line.trim().to_string());
      }
    }
  }

  None
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
        && curr < *min
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
