use crate::surfaces::create_tool_command;
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Minimum Supported Tool Version declarations for tools in the Formality fleet.
pub const MSTV_RUSTFMT: Version = Version::new(1, 4, 0);
pub const MSTV_CLIPPY: Version = Version::new(1, 65, 0);
pub const MSTV_RUFF: Version = Version::new(0, 1, 0);
pub const MSTV_CLANG_FORMAT: Version = Version::new(14, 0, 0);
pub const MSTV_CLANG_TIDY: Version = Version::new(14, 0, 0);
pub const MSTV_PRETTIER: Version = Version::new(2, 0, 0);
pub const MSTV_TAPLO: Version = Version::new(0, 8, 0);
pub const MSTV_MARKDOWNLINT_CLI2: Version = Version::new(0, 4, 0);
pub const MSTV_TYPSTYLE: Version = Version::new(0, 11, 0);

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

  // Strip leading 'v' or 'V' if immediately followed by a digit
  let s = if (cleaned.starts_with('v') || cleaned.starts_with('V'))
    && cleaned.len() > 1
    && cleaned.as_bytes()[1].is_ascii_digit()
  {
    &cleaned[1..]
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
  match binary {
    "rustfmt" => Some(MSTV_RUSTFMT),
    "clippy" | "clippy-driver" | "cargo-clippy" => Some(MSTV_CLIPPY),
    "ruff" => Some(MSTV_RUFF),
    "clang-format" => Some(MSTV_CLANG_FORMAT),
    "clang-tidy" => Some(MSTV_CLANG_TIDY),
    "prettier" => Some(MSTV_PRETTIER),
    "taplo" => Some(MSTV_TAPLO),
    "markdownlint-cli2" => Some(MSTV_MARKDOWNLINT_CLI2),
    "typstyle" => Some(MSTV_TYPSTYLE),
    _ => None,
  }
}

/// Alias for `minimum_supported_tool_version`.
pub fn get_mstv(binary: &str) -> Option<Version> {
  minimum_supported_tool_version(binary)
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
    _ => create_tool_command(binary).arg("--version").output().ok(),
  }?;

  if output.status.success() {
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
mod tests {
  use super::*;

  #[test]
  fn test_version_constructors_and_display() {
    let v1 = Version::new(1, 4, 0);
    assert_eq!(v1.to_string(), "1.4.0");
    assert_eq!(v1.major, 1);
    assert_eq!(v1.minor, 4);
    assert_eq!(v1.patch, 0);
    assert!(v1.prerelease.is_none());

    let v2 = Version::with_prerelease(1, 7, 0, "nightly");
    assert_eq!(v2.to_string(), "1.7.0-nightly");
    assert_eq!(v2.prerelease.as_deref(), Some("nightly"));
  }

  #[test]
  fn test_version_parsing_direct() {
    assert_eq!(Version::parse("1.4.0"), Some(Version::new(1, 4, 0)));
    assert_eq!(Version::parse("v0.17.2"), Some(Version::new(0, 17, 2)));
    assert_eq!(Version::parse("V18.1.8"), Some(Version::new(18, 1, 8)));
    assert_eq!(Version::parse("1.4"), Some(Version::new(1, 4, 0)));
    assert_eq!(
      Version::parse("1.7.0-nightly"),
      Some(Version::with_prerelease(1, 7, 0, "nightly"))
    );
    assert_eq!(
      Version::parse("1.0.0-beta.2+20230101"),
      Some(Version::with_prerelease(1, 0, 0, "beta.2"))
    );
    assert_eq!(Version::parse(""), None);
    assert_eq!(Version::parse("invalid"), None);
  }

  #[test]
  fn test_version_extraction_from_tool_banners() {
    let rustfmt = "rustfmt 1.7.0-nightly (7576e26b 2024-05-07)";
    assert_eq!(
      Version::extract(rustfmt),
      Some(Version::with_prerelease(1, 7, 0, "nightly"))
    );

    let ruff = "ruff 0.9.6";
    assert_eq!(Version::extract(ruff), Some(Version::new(0, 9, 6)));

    let clang_fmt = "clang-format version 18.1.8";
    assert_eq!(Version::extract(clang_fmt), Some(Version::new(18, 1, 8)));

    let clang_tidy = "clang-tidy version 14.0.0-1ubuntu1";
    assert_eq!(
      Version::extract(clang_tidy),
      Some(Version::with_prerelease(14, 0, 0, "1ubuntu1"))
    );

    let prettier = "prettier 3.5.1";
    assert_eq!(Version::extract(prettier), Some(Version::new(3, 5, 1)));

    let taplo = "taplo 0.9.3";
    assert_eq!(Version::extract(taplo), Some(Version::new(0, 9, 3)));

    let typstyle = "typstyle 0.12.0";
    assert_eq!(Version::extract(typstyle), Some(Version::new(0, 12, 0)));

    let markdownlint_cli2 = "markdownlint-cli2 v0.17.2 (markdownlint v0.37.0)";
    assert_eq!(
      Version::extract(markdownlint_cli2),
      Some(Version::new(0, 17, 2))
    );

    let clippy = "clippy 0.1.65 (rustc 1.65.0)";
    assert_eq!(Version::extract(clippy), Some(Version::new(0, 1, 65)));
  }

  #[test]
  fn test_version_ordering() {
    let v1_4_0 = Version::new(1, 4, 0);
    let v1_4_1 = Version::new(1, 4, 1);
    let v1_5_0 = Version::new(1, 5, 0);
    let v2_0_0 = Version::new(2, 0, 0);

    assert!(v1_4_0 < v1_4_1);
    assert!(v1_4_1 < v1_5_0);
    assert!(v1_5_0 < v2_0_0);
    assert!(v1_4_0 <= v1_4_0);
    assert!(v1_4_0 == v1_4_0);

    let v1_0_0 = Version::new(1, 0, 0);
    let v1_0_0_alpha = Version::with_prerelease(1, 0, 0, "alpha");
    let v1_0_0_alpha_1 = Version::with_prerelease(1, 0, 0, "alpha.1");
    let v1_0_0_alpha_beta = Version::with_prerelease(1, 0, 0, "alpha.beta");
    let v1_0_0_beta = Version::with_prerelease(1, 0, 0, "beta");
    let v1_0_0_beta_2 = Version::with_prerelease(1, 0, 0, "beta.2");
    let v1_0_0_beta_11 = Version::with_prerelease(1, 0, 0, "beta.11");
    let v1_0_0_rc_1 = Version::with_prerelease(1, 0, 0, "rc.1");

    // SemVer 2.0.0 Section 11 Specification ordering chain:
    // 1.0.0-alpha < 1.0.0-alpha.1 < 1.0.0-alpha.beta < 1.0.0-beta < 1.0.0-beta.2 < 1.0.0-beta.11 < 1.0.0-rc.1 < 1.0.0
    assert!(v1_0_0_alpha < v1_0_0_alpha_1);
    assert!(v1_0_0_alpha_1 < v1_0_0_alpha_beta);
    assert!(v1_0_0_alpha_beta < v1_0_0_beta);
    assert!(v1_0_0_beta < v1_0_0_beta_2);
    assert!(v1_0_0_beta_2 < v1_0_0_beta_11);
    assert!(v1_0_0_beta_11 < v1_0_0_rc_1);
    assert!(v1_0_0_rc_1 < v1_0_0);

    // Higher major/minor with prerelease is still greater than lower version
    let v1_7_0_nightly = Version::with_prerelease(1, 7, 0, "nightly");
    assert!(v1_7_0_nightly > v1_4_0);
  }

  #[test]
  fn test_mstv_fleet_declarations() {
    assert_eq!(
      minimum_supported_tool_version("rustfmt"),
      Some(Version::new(1, 4, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("clippy"),
      Some(Version::new(1, 65, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("clippy-driver"),
      Some(Version::new(1, 65, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("cargo-clippy"),
      Some(Version::new(1, 65, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("ruff"),
      Some(Version::new(0, 1, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("clang-format"),
      Some(Version::new(14, 0, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("clang-tidy"),
      Some(Version::new(14, 0, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("prettier"),
      Some(Version::new(2, 0, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("taplo"),
      Some(Version::new(0, 8, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("markdownlint-cli2"),
      Some(Version::new(0, 4, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("typstyle"),
      Some(Version::new(0, 11, 0))
    );
    assert_eq!(minimum_supported_tool_version("unknown-tool"), None);

    assert_eq!(get_mstv("rustfmt"), Some(Version::new(1, 4, 0)));
  }

  #[test]
  fn test_compatibility_policy_evaluation() {
    let min = Version::new(1, 4, 0);

    let v_ok = Version::new(1, 7, 0);
    let status_ok = CompatibilityPolicy::evaluate(Some(&v_ok), &min);
    assert!(status_ok.is_compatible());
    assert!(!status_ok.is_outdated());
    assert!(!status_ok.is_not_found());
    assert!(!status_ok.is_unknown_version());
    assert_eq!(
      status_ok,
      ToolStatus::Compatible {
        current: v_ok.clone(),
        minimum: min.clone()
      }
    );
    assert_eq!(
      status_ok.to_string(),
      format!("Compatible ({} >= MSTV {})", v_ok, min)
    );

    let v_old = Version::new(1, 3, 9);
    let status_old = CompatibilityPolicy::evaluate(Some(&v_old), &min);
    assert!(!status_old.is_compatible());
    assert!(status_old.is_outdated());
    assert_eq!(
      status_old,
      ToolStatus::Outdated {
        current: v_old.clone(),
        minimum: min.clone()
      }
    );
    assert_eq!(
      status_old.to_string(),
      format!("Outdated ({} < MSTV {})", v_old, min)
    );

    let status_none = CompatibilityPolicy::evaluate(None, &min);
    assert!(status_none.is_not_found());
    assert_eq!(status_none.to_string(), "Not Found");

    let status_unknown = CompatibilityPolicy::evaluate_with_raw(
      None,
      Some("custom build vX.Y".to_string()),
      &min,
    );
    assert!(status_unknown.is_unknown_version());
    assert_eq!(
      status_unknown.to_string(),
      "Unknown Version (custom build vX.Y)"
    );
  }

  #[test]
  fn test_from_str_trait() {
    let parsed: Result<Version, _> = "3.5.1".parse();
    assert_eq!(parsed, Ok(Version::new(3, 5, 1)));

    let bad: Result<Version, _> = "invalid-ver".parse();
    assert!(bad.is_err());
  }

  #[test]
  fn test_check_tool_compatibility_missing_tool() {
    let status = check_tool_compatibility(
      "nonexistent_binary_xyz_123",
      &Version::new(1, 0, 0),
    );
    assert_eq!(status, ToolStatus::NotFound);
  }

  #[test]
  fn test_live_probe_rustfmt() {
    if which::which("rustfmt").is_ok() {
      let ver = probe_tool_version("rustfmt");
      assert!(ver.is_some(), "Expected rustfmt version to be parsed");
      let mstv = minimum_supported_tool_version("rustfmt").unwrap();
      let status = check_tool_compatibility("rustfmt", &mstv);
      assert!(status.is_compatible(), "rustfmt should satisfy MSTV 1.4.0");
    }
  }
}
