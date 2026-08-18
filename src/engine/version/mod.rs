//! Tool version detection: SemVer parsing, MSRV/MSTV compatibility tables,
//! and CLI version probing.

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

/// Represents a Semantic Version (SemVer) with optional prerelease identifier.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Version {
  pub major: u64,
  pub minor: u64,
  pub patch: u64,
  pub prerelease: Option<String>,
}

impl Version {
  /// Create a new `Version` without prerelease metadata.
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
    Self::parse(s).ok_or_else(|| format!("Invalid semantic version: '{}'", s))
  }
}

/// Returns the Minimum Supported Tool Version (MSTV) for a given tool binary, if defined.
pub fn minimum_supported_tool_version(binary: &str) -> Option<Version> {
  get_tool_mstv_entry(binary).map(|e| e.min_version.clone())
}

/// Alias for `minimum_supported_tool_version`.
pub fn get_mstv(binary: &str) -> Option<Version> {
  minimum_supported_tool_version(binary)
}

/// Retrieve upgrade advice for a given tool binary.
pub fn get_upgrade_advice(binary: &str) -> Option<&'static str> {
  get_tool_mstv_entry(binary).map(|e| e.advice)
}

/// Alias for `get_upgrade_advice`.
pub fn tool_upgrade_advice(binary: &str) -> Option<&'static str> {
  get_upgrade_advice(binary)
}

/// Retrieve version query arguments for a tool binary.
pub fn tool_version_args(binary: &str) -> Option<&'static [&'static str]> {
  get_tool_mstv_entry(binary).map(|e| e.version_args)
}

/// Probe a tool's version by invoking its CLI (`--version` / `-v`) and parsing the output.
pub fn probe_tool_version(binary: &str) -> Option<Version> {
  let raw_output = get_raw_tool_version(binary)?;
  let mut ver = Version::extract(&raw_output)?;

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

/// Retrieve the raw output line from executing the tool with `--version` or `-v`.
pub fn get_raw_tool_version(binary: &str) -> Option<String> {
  let output = match binary {
    "clippy" => {
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
    }
    _ => {
      let args = tool_version_args(binary).unwrap_or(&["--version"]);
      create_tool_command(binary).args(args).output().ok()
    }
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
  Compatible { current: Version, minimum: Version },
  Outdated { current: Version, minimum: Version },
  NotFound,
  UnknownVersion(String),
}

impl ToolStatus {
  pub fn is_compatible(&self) -> bool {
    matches!(self, ToolStatus::Compatible { .. })
  }

  pub fn is_outdated(&self) -> bool {
    matches!(self, ToolStatus::Outdated { .. })
  }

  pub fn is_not_found(&self) -> bool {
    matches!(self, ToolStatus::NotFound)
  }

  pub fn is_unknown_version(&self) -> bool {
    matches!(self, ToolStatus::UnknownVersion(_))
  }
}

impl fmt::Display for ToolStatus {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      ToolStatus::Compatible { current, minimum } => {
        write!(f, "Compatible ({} >= MSTV {})", current, minimum)
      }
      ToolStatus::Outdated { current, minimum } => {
        write!(f, "Outdated ({} < MSTV {})", current, minimum)
      }
      ToolStatus::NotFound => write!(f, "Not Found"),
      ToolStatus::UnknownVersion(raw) => {
        write!(f, "Unknown Version ({})", raw.trim())
      }
    }
  }
}

/// Engine to evaluate tool compatibility policies.
#[derive(Debug, Clone, Default)]
pub struct CompatibilityPolicy;

impl CompatibilityPolicy {
  pub fn check(binary: &str, minimum: &Version) -> ToolStatus {
    check_tool_compatibility(binary, minimum)
  }

  pub fn check_mstv(binary: &str) -> Option<ToolStatus> {
    let min = minimum_supported_tool_version(binary)?;
    Some(Self::check(binary, &min))
  }

  pub fn evaluate(current: Option<&Version>, minimum: &Version) -> ToolStatus {
    match current {
      Some(curr) => {
        if *curr >= *minimum {
          ToolStatus::Compatible {
            current: curr.clone(),
            minimum: minimum.clone(),
          }
        } else {
          ToolStatus::Outdated {
            current: curr.clone(),
            minimum: minimum.clone(),
          }
        }
      }
      None => ToolStatus::NotFound,
    }
  }

  pub fn evaluate_with_raw(
    current: Option<Version>,
    raw_output: Option<String>,
    minimum: &Version,
  ) -> ToolStatus {
    match (current, raw_output) {
      (Some(curr), _) => {
        if curr >= *minimum {
          ToolStatus::Compatible {
            current: curr,
            minimum: minimum.clone(),
          }
        } else {
          ToolStatus::Outdated {
            current: curr,
            minimum: minimum.clone(),
          }
        }
      }
      (None, Some(raw)) if !raw.trim().is_empty() => {
        ToolStatus::UnknownVersion(raw)
      }
      _ => ToolStatus::NotFound,
    }
  }
}

/// Check the compatibility status of an installed tool against a minimum required version.
pub fn check_tool_compatibility(binary: &str, minimum: &Version) -> ToolStatus {
  if which::which(binary).is_err() {
    if binary == "clippy" {
      if which::which("clippy-driver").is_err()
        && which::which("cargo").is_err()
      {
        return ToolStatus::NotFound;
      }
    } else {
      return ToolStatus::NotFound;
    }
  }

  let raw = get_raw_tool_version(binary);
  let probed = probe_tool_version(binary);

  CompatibilityPolicy::evaluate_with_raw(probed, raw, minimum)
}

#[cfg(test)]
mod tests;
