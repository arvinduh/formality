//! Tool version detection, split into two layers with one explicit crossing:
//!
//! - **Extraction (custom, kept):** `probe_raw_tool_version_uncached` and
//!   `classify_token` scrape a version out of frequently-non-semver CLI output
//!   (`go version go1.27.0 ...`, `0.44`, `18.1.8-0ubuntu1~22.04.1`,
//!   `1.35.1.post1`). A non-semver suffix or 4th component is salvaged down to
//!   the bare `MAJOR.MINOR.PATCH`; a malformed core (`01.2.3`) is not.
//! - **Comparison (`semver`-backed):** `Version` ordering/precedence and the
//!   MSTV "installed >= minimum" check are delegated to the `semver` crate via
//!   the sole crossing, `Version::to_semver`.
//!
//! A version-shaped token that is invalid semver even bare surfaces as
//! `ToolStatus::UnknownVersion` — never a silently-satisfied MSTV.

/// Minimum Supported Tool Version (MSTV) definitions and registry.
pub mod mstv;

use std::cmp;
use std::collections;
use std::ffi;
use std::fmt;
use std::path;
use std::str;
use std::time;

use serde;

use crate::engine;
use crate::surfaces::tooling;

/// Cache TTL for probed tool versions: 24 hours.
const TOOL_VERSION_CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// An on-disk cache entry recording the probed version output and metadata for a tool binary.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
struct ToolVersionEntry {
  /// The raw stdout/stderr output banner obtained from the tool.
  raw_version: String,
  /// Timestamp (seconds since UNIX epoch) when this entry was probed.
  last_checked_unix: u64,
  /// File modification timestamp (seconds since UNIX epoch) of the binary at probe time.
  binary_mtime_unix: u64,
  /// Absolute or resolved path to the tool binary executable at probe time.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  binary_path: Option<String>,
}

/// The collection of cached tool versions stored in `tool_versions.json`.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Default)]
struct ToolVersionStore {
  /// Map of tool binary names to their cached version entry.
  #[serde(default)]
  tools: collections::BTreeMap<String, ToolVersionEntry>,
}

/// Returns the full path to the `tool_versions.json` cache file in formality's cache directory.
#[must_use]
fn get_tool_versions_cache_path() -> path::PathBuf {
  engine::cache_path("tool_versions.json")
}

/// Reads and deserializes the tool versions cache from the given path.
/// Returns `None` if the file cannot be read or parsed.
#[must_use]
fn read_tool_version_cache_at(path: &path::Path) -> Option<ToolVersionStore> {
  let data = std::fs::read_to_string(path).ok()?;
  serde_json::from_str(&data).ok()
}

/// Serializes and writes the tool versions cache to the given path.
fn write_tool_version_cache_at(path: &path::Path, store: &ToolVersionStore) {
  engine::write_cache(path, store);
}

/// Resolves the binary path on PATH and retrieves its file modification timestamp (`mtime`).
#[must_use]
fn resolve_binary_info(binary: &str) -> Option<(path::PathBuf, u64)> {
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
    .and_then(|t| t.duration_since(time::UNIX_EPOCH).ok())
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
    matches!(
      classify_token(tok),
      TokenParse::Ok(_, _) | TokenParse::Rejected
    )
  })
}

/// The [`mstv::VersionProbe`] declared for `binary`, falling back to
/// [`mstv::DEFAULT_VERSION_PROBE`] for a binary with no registry entry.
#[must_use]
fn version_probe_for(binary: &str) -> mstv::VersionProbe {
  mstv::get_tool_mstv_entry(binary)
    .map_or(mstv::DEFAULT_VERSION_PROBE, |entry| entry.probe)
}

/// Picks the version line out of a finished probe command's streams.
///
/// A non-zero exit is not by itself evidence that the output is garbage: the
/// npm `@taplo/cli` build writes `taplo 0.9.0` to stdout and *then* exits 1,
/// so gating the scrape on the exit status read a perfectly good version and
/// threw it away (Fixes #176). What keeps that relaxation honest is #114's
/// [`line_carries_version_token`], which requires a genuinely version-shaped
/// token rather than "contains a digit" — usage text and error messages carry
/// no such token and still yield `None`.
///
/// The relaxation is deliberately asymmetric across the two streams:
///
/// - **Exit 0** — stdout, then stderr. Unchanged; `google-java-format` is the
///   one tool in the fleet that reports its version on stderr.
/// - **Non-zero exit** — stdout *only*. A failed command's stderr is where it
///   explains its failure, and scraping that is the path #167 deliberately
///   removed. Nothing here brings it back.
fn version_line_from_probe_output(
  succeeded: bool,
  stdout: &str,
  stderr: &str,
) -> Option<String> {
  let from_stdout = first_versionish_line(stdout);
  if !succeeded {
    return from_stdout;
  }
  from_stdout.or_else(|| first_versionish_line(stderr))
}

/// Extracts the module version from the `mod` line of `go version -m` output.
///
/// Note: The version reported is the `golang.org/x/tools` module version that
/// `goimports` was built from, not a goimports-specific release version (Fixes #178).
///
/// Returns `None` if:
/// - There is no `mod` line in the output.
/// - The module version is `(devel)` (built from a local working copy).
/// - The module version token is not a valid semantic version.
#[must_use]
fn parse_go_version_m(output: &str) -> Option<String> {
  for line in output.lines() {
    let mut parts = line.split_whitespace();
    if parts.next() == Some("mod") {
      let _mod_path = parts.next()?;
      let version = parts.next()?;
      if version == "(devel)" {
        return None;
      }
      if let TokenParse::Ok(_, _) = classify_token(version) {
        return Some(version.to_string());
      }
      return None;
    }
  }
  None
}

/// Renders a list of [`ProbeArg`]s into arguments for command execution.
/// Resolves [`mstv::ProbeArg::ToolPath`] using the path to `binary` found on PATH.
/// Returns `None` if [`mstv::ProbeArg::ToolPath`] is needed but the binary cannot be resolved.
#[must_use]
fn render_probe_args(
  binary: &str,
  args: &[mstv::ProbeArg],
) -> Option<Vec<ffi::OsString>> {
  let mut rendered = Vec::with_capacity(args.len());
  let mut resolved_path: Option<path::PathBuf> = None;

  for arg in args {
    match arg {
      mstv::ProbeArg::Literal(s) => rendered.push(ffi::OsString::from(s)),
      mstv::ProbeArg::ToolPath => {
        let path = if let Some(p) = &resolved_path {
          p.clone()
        } else {
          let p = which::which(binary).ok()?;
          resolved_path = Some(p.clone());
          p
        };
        rendered.push(path.into_os_string());
      }
    }
  }

  Some(rendered)
}

/// Runs one probe command and extracts the raw version line using `extractor`.
/// `None` when the command cannot be spawned, or the extractor yields `None`.
fn run_probe_command<I, S>(
  bin: &str,
  args: I,
  extractor: mstv::ProbeExtractor,
) -> Option<String>
where
  I: IntoIterator<Item = S>,
  S: AsRef<ffi::OsStr>,
{
  let output = tooling::create_tool_command(bin).args(args).output().ok()?;
  match extractor {
    mstv::ProbeExtractor::FirstVersionishLine => {
      version_line_from_probe_output(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
      )
    }
    mstv::ProbeExtractor::GoModuleVersion => {
      if !output.status.success() {
        return None;
      }
      parse_go_version_m(&String::from_utf8_lossy(&output.stdout)).or_else(
        || parse_go_version_m(&String::from_utf8_lossy(&output.stderr)),
      )
    }
  }
}

/// Executes `probe` against `binary` and extracts the raw version line.
///
/// The function carries no policy of its own: it is ignorant of *which* tool
/// it is probing, and runs exactly the commands the [`mstv::VersionProbe`] names —
/// no implicit second attempt, so a registry entry always describes what
/// actually runs. A tool that reports its version unusually is an entry here,
/// not a branch (Fixes #177).
fn run_probe(binary: &str, probe: &mstv::VersionProbe) -> Option<String> {
  match probe {
    mstv::VersionProbe::OwnFlags(flags) => run_probe_command(
      binary,
      *flags,
      mstv::ProbeExtractor::FirstVersionishLine,
    ),
    mstv::VersionProbe::ViaBinary {
      bin,
      args,
      extractor,
    } => {
      let rendered = render_probe_args(binary, args)?;
      run_probe_command(bin, &rendered, *extractor)
    }
    mstv::VersionProbe::FirstOf(probes) => {
      probes.iter().find_map(|probe| run_probe(binary, probe))
    }
  }
}

/// Executes the version probe the registry declares for `binary`, uncached,
/// and extracts the raw output line.
#[must_use]
fn probe_raw_tool_version_uncached(binary: &str) -> Option<String> {
  run_probe(binary, &version_probe_for(binary))
}

/// Retrieve the raw output line from executing the tool's registry-declared
/// [`VersionProbe`], checking the on-disk cache at `cache_path` first.
/// If cached version is fresh (TTL valid) and binary modification time matches,
/// returns the cached version string without spawning a subprocess.
/// Otherwise, invokes the tool CLI, updates the cache, and returns the result.
#[must_use]
fn get_raw_tool_version_at(
  binary: &str,
  cache_path: &path::Path,
) -> Option<String> {
  let bin_info = resolve_binary_info(binary);

  if std::env::var("FORMALITY_NO_VERSION_CACHE").is_err()
    && let Some((ref bin_path, bin_mtime)) = bin_info
    && let Some(store) = read_tool_version_cache_at(cache_path)
    && let Some(entry) = store.tools.get(binary)
  {
    let now = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
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
    let now = time::SystemTime::now()
      .duration_since(time::UNIX_EPOCH)
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
fn probe_tool_version_at(
  binary: &str,
  cache_path: &path::Path,
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
/// to `semver` via `Version::to_semver`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
  /// Major version component.
  major: u64,
  /// Minor version component.
  minor: u64,
  /// Patch version component.
  patch: u64,
  /// Optional prerelease metadata string.
  prerelease: Option<String>,
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

  /// Create a `Version` with prerelease metadata. The prerelease must be
  /// a valid `SemVer` identifier (what the parse path always yields); one
  /// `semver` rejects still constructs but makes [`Version::to_semver`] lossy,
  /// so ordering stops matching structural equality — a `debug_assert`
  /// catches it.
  #[cfg(test)]
  fn with_prerelease(
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
      TokenParse::Ok(v, _) => Some(v),
      TokenParse::Rejected => None,
      TokenParse::NotVersion => Self::extract(trimmed),
    }
  }

  /// Scan a multi-token banner. The first *version-shaped* token decides the
  /// result: if valid it wins; if malformed beyond salvage the scan aborts
  /// with `None` rather than walking on to a later, unrelated number.
  #[must_use]
  fn extract(input: &str) -> Option<Self> {
    Self::extract_with_raw(input).map(|(v, _)| v)
  }

  /// Extracts the raw version string token from a multi-token banner, if
  /// a valid version-shaped token is found.
  #[must_use]
  fn extract_raw(input: &str) -> Option<&str> {
    Self::extract_with_raw(input).map(|(_, raw)| raw)
  }

  /// Scan a multi-token banner and extract both the parsed [`Version`] and
  /// the raw version token string from the banner.
  #[must_use]
  fn extract_with_raw(input: &str) -> Option<(Self, &str)> {
    for token in input.split_whitespace() {
      match classify_token(token) {
        TokenParse::Ok(v, raw) => return Some((v, raw)),
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

enum TokenParse<'a> {
  /// Parsed — strict (suffix preserved) or salvaged to the bare `M.M.P` core,
  /// paired with the raw version token extracted from the input text.
  Ok(Version, &'a str),
  /// Not version-shaped: keep scanning.
  NotVersion,
  /// Version-shaped but invalid semver even bare (leading-zero core): abort.
  Rejected,
}

/// Read a version out of one token. Strips surrounding punctuation and a
/// leading `v`/`go` marker, then tries, in order: the 3-part core plus a
/// genuine `-pre`/`+build` suffix (keeps `1.7.0-nightly`); then the bare core
/// alone, dropping a packaging-revision suffix (`14.0.0-1ubuntu1`), a `-`/`+`
/// suffix, or a clean 4th component `semver` rejects
/// (`18.1.8-0ubuntu1~22.04.1`, `1.35.1.post1`, `0.9.6.dev0` — which the
/// pre-`semver` parser also ignored). A non-numeric 3rd component
/// (`0.9.6rc1`, `1.2.x`) is rejected, never zeroed.
fn classify_token(token: &str) -> TokenParse<'_> {
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
    // A `-`-suffix that parses as valid semver but is classified as a
    // packaging/distro revision (`-1ubuntu1`, `-4.fc39`, a bare `-1`,
    // `-ubuntu1`, `-deb1`) rather than a genuine prerelease (`-rc1`, `-m1`,
    // `-next`): drop it and keep the bare core, same salvage the invalid-semver
    // suffixes above already get.
    Ok(sv) if !sv.pre.is_empty() && !is_genuine_prerelease(sv.pre.as_str()) => {
      match semver::Version::parse(&core) {
        Ok(bare) => TokenParse::Ok(
          Version {
            major: bare.major,
            minor: bare.minor,
            patch: bare.patch,
            prerelease: None,
          },
          s,
        ),
        Err(_) => TokenParse::Rejected,
      }
    }
    Ok(sv) => TokenParse::Ok(
      Version {
        major: sv.major,
        minor: sv.minor,
        patch: sv.patch,
        prerelease: (!sv.pre.is_empty()).then(|| sv.pre.as_str().to_string()),
      },
      s,
    ),
    Err(_) => TokenParse::Rejected,
  }
}

/// Distro/packaging revision prefixes and post-release qualifiers (case-insensitive),
/// matched against the leading alphabetic run of a semver prerelease's *first*
/// dot-separated identifier (`"ubuntu1"` -> `"ubuntu"`, `"fc39"` -> `"fc"`,
/// `"build5"` -> `"build"`, `"final"` -> `"final"`).
///
/// Suffixes whose leading alphabetic run matches this blocklist are treated as
/// distro/packaging revisions or post-release qualifiers and stripped down to
/// the bare core release (Fixes #149, #171). Unknown alphabetic prefixes default
/// to genuine prereleases.
const PACKAGING_BLOCKLIST: &[&str] = &[
  "ubuntu", "deb", "el", "fc", "build", "alt", "mga", "bp", "lp", "ga",
  "final", "release",
];

/// Whether a semver prerelease string (e.g. `sv.pre.as_str()`) reads as a
/// genuine prerelease rather than a distro/packaging revision suffix.
///
/// This uses a hybrid strategy with opposite approaches for the two halves
/// (Fixes #171):
///
/// 1. **Numeric-leading half (structural, list-free):**
///    Inspects the *first* dot-separated identifier. A purely numeric leading
///    identifier (`-1` Arch-style, `-1ubuntu1`'s `1ubuntu1` Debian/Ubuntu-style,
///    `-4.fc39`'s `4` Fedora/RPM-style, `-2` Homebrew-style) has no leading
///    alphabetic run at all (`leading_alpha.is_empty()`) and is structurally
///    classified as a distro revision without needing any list. Every genuine
///    prerelease convention leads with a letter or keyword, never a bare digit.
///
/// 2. **Alphabetic-leading half (blocklist, defaulting to prerelease):**
///    For suffixes whose first identifier starts with letters (`-m1`, `-M1`,
///    `-a1`, `-b2`, `-next`, `-beta.2`, `-ubuntu1`), an allowlist has an
///    asymmetric failure mode: an allowlist miss is silent and fail-unsafe (a
///    prerelease is falsely declared compatible with an MSTV floor it does not
///    meet). Conversely, a blocklist miss is loud and self-diagnosing in CLI
///    output (`v14.0.0-foo < MSTV v14.0.0`). Furthermore, the population in
///    this bucket is lopsided: packaging spellings are few and bounded
///    (`ubuntu`, `deb`, `el`, `fc`, `build`, etc.), while prerelease keywords
///    are open-ended and constantly growing (`m`, `M`, `a`, `b`, `rc`, `next`,
///    `experimental`, `unstable`, `insiders`, `devel`, `milestone`, etc.).
///    Therefore, the alphabetic-leading branch blocks known packaging and
///    release qualifiers and defaults all other alphabetic suffixes to
///    genuine prereleases.
#[must_use]
fn is_genuine_prerelease(pre: &str) -> bool {
  let Some(first) = pre.split('.').next() else {
    return false;
  };
  let leading_alpha: String = first
    .chars()
    .take_while(char::is_ascii_alphabetic)
    .collect();
  if leading_alpha.is_empty() {
    return false;
  }
  let lower = leading_alpha.to_ascii_lowercase();
  !PACKAGING_BLOCKLIST.contains(&lower.as_str())
}

// === Comparison layer (delegated wholesale to the `semver` crate) ===========

impl Version {
  /// The sole crossing from the extraction layer into `semver`-backed
  /// comparison. The parse path only stores `semver`-valid prereleases, so the
  /// `"0"` fallback is unreachable in practice; if a caller hand-builds an
  /// invalid one anyway it sorts below the matching release (rule 9) but two
  /// such strings then compare `Equal` while `PartialEq` sees them distinct —
  /// the test-only `Version::with_prerelease`'s `debug_assert` guards that.
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
  fn cmp(&self, other: &Self) -> cmp::Ordering {
    self.to_semver().cmp(&other.to_semver())
  }
}

impl PartialOrd for Version {
  fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
    Some(self.cmp(other))
  }
}

impl fmt::Display for Version {
  /// Rendered by `semver`, so the text matches what the comparison layer sees.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.to_semver())
  }
}

impl str::FromStr for Version {
  type Err = String;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Self::parse(s).ok_or_else(|| format!("Invalid semantic version: '{s}'"))
  }
}

/// Returns the Minimum Supported Tool Version (MSTV) for a given tool binary, if defined.
#[must_use]
pub fn minimum_supported_tool_version(binary: &str) -> Option<Version> {
  mstv::get_tool_mstv_entry(binary).and_then(|e| e.min_version.clone())
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

/// Returns the raw version string token from `raw_banner` if it differs from
/// the normalized `current` version.
///
/// Returns `None` if:
/// - `raw_banner` is `None` or contains no version-shaped token.
/// - The extracted raw version token is identical to `current`'s rendered
///   version (e.g. `1.2.3` or `v1.2.3` matching `1.2.3`).
#[must_use]
pub fn reported_raw_version_if_differing<'a>(
  current: &Version,
  raw_banner: Option<&'a str>,
) -> Option<&'a str> {
  let raw = raw_banner.and_then(Version::extract_raw)?;
  if raw == current.to_string() {
    None
  } else {
    Some(raw)
  }
}

/// Status of a tool relative to its minimum required version.
#[derive(Debug, PartialEq)]
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
  /// floor, but does not match the exact version `fml doctor --install` pins for
  /// this tool (`src/surfaces/tooling.rs`'s install chains) — e.g. a stale
  /// system-wide install that predates the pin. Distinct from `Outdated`:
  /// an `Outdated` tool may not even work; a `Stale` one works, it's just
  /// not the bit-for-bit version CI will run, so its formatting/linting
  /// output can silently disagree with CI's.
  Stale {
    /// Currently installed version.
    current: Version,
    /// Exact version `fml doctor --install` pins this tool to.
    pinned: Version,
  },
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
mod tests {
  use super::*;

  #[test]
  fn version_constructors_and_display() {
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
  fn version_parsing_direct() {
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

    // A leading-zero *core* is malformed beyond salvage: `None` (surfaced as
    // UnknownVersion downstream), never a fabricated comparable version.
    assert_eq!(Version::parse("01.2.3"), None);
    assert_eq!(Version::parse("1.2.03"), None);

    // A present-but-non-numeric 3rd component cannot become a patch without
    // fabricating a lower-than-reality `X.Y.0`: reject, don't salvage.
    // (PEP440 separator-less prereleases like `0.9.6rc1` are real for
    // pip-installed ruff / yamllint.)
    assert_eq!(Version::parse("0.9.6rc1"), None);
    assert_eq!(Version::parse("1.35.dev1"), None);
    assert_eq!(Version::parse("1.2.x"), None);

    // A non-semver *suffix* after a valid `MAJOR.MINOR.PATCH` core (or a clean
    // 4th component) is salvaged down to that core — the pre-`semver` parser
    // ignored trailing junk too, and the patch is preserved, not zeroed.
    assert_eq!(Version::parse("1.0.0-01"), Some(Version::new(1, 0, 0)));
    assert_eq!(
      Version::parse("18.1.8-0ubuntu1~22.04.1"),
      Some(Version::new(18, 1, 8))
    );
    assert_eq!(Version::parse("1.35.1.post1"), Some(Version::new(1, 35, 1)));
    assert_eq!(Version::parse("0.9.6.dev0"), Some(Version::new(0, 9, 6)));
    // Dotted separator keeps the patch; only the 4th component is dropped.
    assert_eq!(Version::parse("0.9.6.rc1"), Some(Version::new(0, 9, 6)));

    // First-match-wins is bounded: a malformed-beyond-salvage version-shaped
    // token aborts the scan rather than skipping ahead to a later number.
    assert_eq!(Version::parse("weird 01.2.3 (built 2024.1.5)"), None);
  }

  #[test]
  fn version_extraction_from_tool_banners() {
    let rustfmt = "rustfmt 1.7.0-nightly (7576e26b 2024-05-07)";
    assert_eq!(
      Version::extract(rustfmt),
      Some(Version::with_prerelease(1, 7, 0, "nightly"))
    );

    let ruff = "ruff 0.9.6";
    assert_eq!(Version::extract(ruff), Some(Version::new(0, 9, 6)));

    let clang_fmt = "clang-format version 18.1.8";
    assert_eq!(Version::extract(clang_fmt), Some(Version::new(18, 1, 8)));

    // #149: `-1ubuntu1` is a Debian/Ubuntu packaging *revision*, not a
    // prerelease -- it must salvage to the bare core, same verdict as the
    // `~`-mangled Ubuntu suffix below, not sort below `14.0.0` as a
    // prerelease would. This is a deliberate verdict change from #145, which
    // took the `semver`-valid-prerelease parse at face value here.
    let clang_tidy = "clang-tidy version 14.0.0-1ubuntu1";
    assert_eq!(Version::extract(clang_tidy), Some(Version::new(14, 0, 0)));

    // Ubuntu distro revision: `~` and a leading-zero identifier make the suffix
    // invalid semver, so it is salvaged to the bare core (Compatible, not
    // Unknown, against clang's MSTV of 14.0.0).
    let clang_fmt_ubuntu = "clang-format version 18.1.8-0ubuntu1~22.04.1";
    assert_eq!(
      Version::extract(clang_fmt_ubuntu),
      Some(Version::new(18, 1, 8))
    );
    let clang_tidy_ubuntu = "Ubuntu clang-tidy version 18.1.8-0ubuntu1~22.04.1";
    assert_eq!(
      Version::extract(clang_tidy_ubuntu),
      Some(Version::new(18, 1, 8))
    );

    // PyPI post/dev builds carry a non-numeric 4th component the pre-`semver`
    // parser ignored; the bare-core salvage keeps that behaviour.
    let yamllint_post = "yamllint 1.35.1.post1";
    assert_eq!(
      Version::extract(yamllint_post),
      Some(Version::new(1, 35, 1))
    );
    let ruff_dev = "ruff 0.9.6.dev0";
    assert_eq!(Version::extract(ruff_dev), Some(Version::new(0, 9, 6)));

    // ...but a PEP440 separator-less prerelease (`0.9.6rc1`) has no clean patch
    // to keep: it is rejected, not salvaged to a fabricated `0.9.0`.
    assert_eq!(Version::extract("ruff 0.9.6rc1"), None);
    assert_eq!(normalize_probed_version("ruff", "ruff 0.9.6rc1"), None);

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

    let yamllint = "yamllint 1.35.1";
    assert_eq!(Version::extract(yamllint), Some(Version::new(1, 35, 1)));

    let biome = "1.9.4";
    assert_eq!(Version::extract(biome), Some(Version::new(1, 9, 4)));

    let checkstyle = "Checkstyle version: 10.14.0";
    assert_eq!(Version::extract(checkstyle), Some(Version::new(10, 14, 0)));

    let checkstyle2 = "Checkstyle version 10.0.0";
    assert_eq!(Version::extract(checkstyle2), Some(Version::new(10, 0, 0)));

    let ktlint = "1.0.1";
    assert_eq!(Version::extract(ktlint), Some(Version::new(1, 0, 1)));

    let go = "go version go1.21.5 darwin/arm64";
    assert_eq!(Version::extract(go), Some(Version::new(1, 21, 5)));

    let go_simple = "go1.18.0";
    assert_eq!(Version::extract(go_simple), Some(Version::new(1, 18, 0)));

    let golangci = "golangci-lint has version 1.55.2 built with go1.21.5 from 39c1b3f on 2023-12-04T12:00:00Z";
    assert_eq!(Version::extract(golangci), Some(Version::new(1, 55, 2)));
  }

  #[expect(
    clippy::too_many_lines,
    reason = "table-driven test suite for distro revision vs prerelease heuristic"
  )]
  #[test]
  fn distro_revision_vs_genuine_prerelease() {
    // #149: table-driven coverage of the extraction-layer heuristic that
    // distinguishes a packaging/distro revision suffix (sorts *with* its base
    // release, never below it) from a genuine prerelease (sorts below).
    //
    // (input, expected parse, is a prerelease?)
    let cases: &[(&str, Version, bool)] = &[
      // Plain releases: nothing to distinguish.
      ("1.2.3", Version::new(1, 2, 3), false),
      // Debian/Ubuntu packaging revisions: numeric-leading identifier with no
      // recognised prerelease keyword -> distro revision, bare core kept.
      ("1.2.3-1ubuntu2", Version::new(1, 2, 3), false),
      ("1.2.3-0ubuntu1", Version::new(1, 2, 3), false),
      // `~` makes the suffix invalid semver outright; already salvaged to the
      // bare core before the prerelease-keyword check even runs.
      ("1.2.3-0ubuntu1~22.04.1", Version::new(1, 2, 3), false),
      // RPM/Fedora revisions (`-<rev>.<dist-tag>`): first identifier is a bare
      // digit, same bucket as the Debian revisions above.
      ("1.2.3-4.fc39", Version::new(1, 2, 3), false),
      // Arch's single-integer package-revision suffix. Ambiguous in the
      // abstract (SemVer alone would read `-1` as a prerelease numeric
      // identifier), but Arch is the only convention that emits a bare
      // numeral here, and a real prerelease is never spelled as a plain
      // integer with no keyword -- tie-break: distro revision.
      ("1.2.3-1", Version::new(1, 2, 3), false),
      // Homebrew-style single-integer bottle/formula revision -- same bucket
      // and same tie-break as the Arch case.
      ("1.2.3-2", Version::new(1, 2, 3), false),
      // A distro-style tag with a non-numeric but non-keyword leading
      // identifier (a raw distro/codename prefix, not `alpha`/`beta`/etc.)
      // is still a revision, not a prerelease.
      ("1.2.3-ubuntu1", Version::new(1, 2, 3), false),
      // Genuine prereleases: recognised keyword leads the first identifier,
      // kept and must sort *below* the bare release.
      ("1.2.3-rc1", Version::with_prerelease(1, 2, 3, "rc1"), true),
      (
        "1.2.3-rc.1",
        Version::with_prerelease(1, 2, 3, "rc.1"),
        true,
      ),
      (
        "1.2.3-beta.2",
        Version::with_prerelease(1, 2, 3, "beta.2"),
        true,
      ),
      (
        "1.2.3-alpha",
        Version::with_prerelease(1, 2, 3, "alpha"),
        true,
      ),
      ("1.2.3-pre", Version::with_prerelease(1, 2, 3, "pre"), true),
      (
        "1.2.3-nightly",
        Version::with_prerelease(1, 2, 3, "nightly"),
        true,
      ),
      // Build metadata is dropped from ordering entirely (semver rule), not a
      // prerelease either way -- parses to the bare core with no prerelease.
      ("1.2.3+build.5", Version::new(1, 2, 3), false),
      // A genuine prerelease combined with build metadata: prerelease kept,
      // build metadata still dropped.
      (
        "1.2.3-rc1+build.5",
        Version::with_prerelease(1, 2, 3, "rc1"),
        true,
      ),
      // #171: inverted alphabetic-leading branch defaults unknown alphabetic
      // prefixes to genuine prereleases (Maven milestones, npm next, devel, etc.).
      ("1.2.3-m1", Version::with_prerelease(1, 2, 3, "m1"), true),
      ("1.2.3-M1", Version::with_prerelease(1, 2, 3, "M1"), true),
      ("1.2.3-a1", Version::with_prerelease(1, 2, 3, "a1"), true),
      ("1.2.3-b2", Version::with_prerelease(1, 2, 3, "b2"), true),
      (
        "1.2.3-next",
        Version::with_prerelease(1, 2, 3, "next"),
        true,
      ),
      (
        "1.2.3-next.5",
        Version::with_prerelease(1, 2, 3, "next.5"),
        true,
      ),
      (
        "1.2.3-experimental",
        Version::with_prerelease(1, 2, 3, "experimental"),
        true,
      ),
      (
        "1.2.3-unstable",
        Version::with_prerelease(1, 2, 3, "unstable"),
        true,
      ),
      (
        "1.2.3-insiders",
        Version::with_prerelease(1, 2, 3, "insiders"),
        true,
      ),
      (
        "1.2.3-devel",
        Version::with_prerelease(1, 2, 3, "devel"),
        true,
      ),
      (
        "1.2.3-milestone1",
        Version::with_prerelease(1, 2, 3, "milestone1"),
        true,
      ),
      // #171: packaging blocklist entries are recognized and stripped to bare core.
      ("1.2.3-deb1", Version::new(1, 2, 3), false),
      ("1.2.3-el8", Version::new(1, 2, 3), false),
      ("1.2.3-fc39", Version::new(1, 2, 3), false),
      ("1.2.3-build5", Version::new(1, 2, 3), false),
      ("1.2.3-alt1", Version::new(1, 2, 3), false),
      ("1.2.3-mga8", Version::new(1, 2, 3), false),
      ("1.2.3-bp1", Version::new(1, 2, 3), false),
      ("1.2.3-lp152", Version::new(1, 2, 3), false),
      ("1.2.3-ga", Version::new(1, 2, 3), false),
      ("1.2.3-final", Version::new(1, 2, 3), false),
      ("1.2.3-FINAL", Version::new(1, 2, 3), false),
      ("1.2.3-release", Version::new(1, 2, 3), false),
    ];

    for (input, expected, is_prerelease) in cases {
      let parsed = Version::parse(input);
      assert_eq!(parsed, Some(expected.clone()), "parsing {input:?}");
      assert_eq!(
        parsed.as_ref().unwrap().prerelease.is_some(),
        *is_prerelease,
        "prerelease-ness of {input:?}"
      );

      // MSTV outcome: a distro revision must compare >= its own bare release
      // (never Outdated against an MSTV equal to that release); a genuine
      // prerelease must compare < it (Outdated against that same floor).
      let base = Version::new(1, 2, 3);
      let status = evaluate_tool_status(parsed, None, Some(&base), None);
      if *is_prerelease {
        assert!(
          matches!(status, ToolStatus::Outdated { .. }),
          "{input:?} (genuine prerelease) must be Outdated vs MSTV {base}, got {status:?}"
        );
      } else {
        assert!(
          matches!(status, ToolStatus::Compatible { .. }),
          "{input:?} (distro revision or plain release) must be Compatible vs MSTV {base}, got {status:?}"
        );
      }
    }
  }

  #[test]
  fn version_ordering() {
    let v1_4_0 = Version::new(1, 4, 0);
    let v1_4_1 = Version::new(1, 4, 1);
    let v1_5_0 = Version::new(1, 5, 0);
    let v2_0_0 = Version::new(2, 0, 0);

    assert!(v1_4_0 < v1_4_1);
    assert!(v1_4_1 < v1_5_0);
    assert!(v1_5_0 < v2_0_0);
    assert!(v1_4_0 <= v1_4_0);
    assert_eq!(v1_4_0, v1_4_0);

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
  fn mstv_fleet_declarations() {
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
    assert_eq!(
      minimum_supported_tool_version("yamllint"),
      Some(Version::new(1, 20, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("biome"),
      Some(Version::new(1, 5, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("checkstyle"),
      Some(Version::new(10, 0, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("ktlint"),
      Some(Version::new(1, 0, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("gofmt"),
      Some(Version::new(1, 18, 0))
    );
    assert_eq!(
      minimum_supported_tool_version("golangci-lint"),
      Some(Version::new(1, 50, 0))
    );
    assert_eq!(minimum_supported_tool_version("unknown-tool"), None);

    assert!(mstv::TOOL_MSTV_REGISTRY.len() >= 16);
  }

  /// The MSTV floor outranks the pin, the floor itself is inclusive, a
  /// prerelease at the floor sits below it, a parsed version beats any raw
  /// banner, and an unparseable or blank banner never passes for a version.
  #[test]
  fn evaluate_tool_status_decision_table() {
    let v = |s: &str| Version::parse(s).unwrap();
    // (current, raw banner, MSTV floor, pin, expected)
    let cases = [
      (Some("1.7.0"), None, Some("1.4.0"), None, "compatible"),
      (Some("1.4.0"), None, Some("1.4.0"), None, "compatible"),
      (Some("1.3.9"), None, Some("1.4.0"), None, "outdated"),
      (Some("1.4.0-rc.1"), None, Some("1.4.0"), None, "outdated"),
      (
        Some("2.0.0"),
        Some("garbage"),
        Some("1.0.0"),
        None,
        "compatible",
      ),
      (Some("5.0.0"), None, Some("1.4.0"), None, "compatible"),
      (
        Some("3.9.6"),
        None,
        Some("2.0.0"),
        Some("3.9.6"),
        "compatible",
      ),
      (Some("3.8.1"), None, Some("2.0.0"), Some("3.9.6"), "stale"),
      (
        Some("1.0.0"),
        None,
        Some("2.0.0"),
        Some("3.9.6"),
        "outdated",
      ),
      (None, None, Some("2.0.0"), Some("3.9.6"), "not found"),
      (None, Some("   "), Some("1.0.0"), None, "not found"),
      (None, Some("custom build"), None, Some("3.9.6"), "unknown"),
      (
        None,
        Some("custom build"),
        Some("1.4.0"),
        Some("3.9.6"),
        "unknown",
      ),
    ];
    for (current, raw, minimum, pinned, expected) in cases {
      let status = evaluate_tool_status(
        current.map(v),
        raw.map(str::to_string),
        minimum.map(v).as_ref(),
        pinned.map(v).as_ref(),
      );
      let kind = match status {
        ToolStatus::Compatible { .. } => "compatible",
        ToolStatus::Outdated { .. } => "outdated",
        ToolStatus::Stale { .. } => "stale",
        ToolStatus::NotFound => "not found",
        ToolStatus::UnknownVersion(_) => "unknown",
      };
      assert_eq!(kind, expected, "{current:?} {raw:?} {minimum:?} {pinned:?}");
    }
  }

  #[test]
  fn salvaged_distro_build_still_gets_a_real_mstv_verdict() {
    // End to end: an Ubuntu clang-format banner whose distro-revision suffix is
    // not valid semver must still yield a comparable version and a real MSTV
    // verdict -- not `UnknownVersion`, which would silently drop the check on a
    // normal Ubuntu dev box (QA finding #1).
    let raw = "clang-format version 18.1.8-0ubuntu1~22.04.1";
    let current = normalize_probed_version("clang-format", raw);
    assert_eq!(current, Some(Version::new(18, 1, 8)));

    let min = Version::new(14, 0, 0);
    let status =
      evaluate_tool_status(current, Some(raw.to_string()), Some(&min), None);
    assert!(matches!(status, ToolStatus::Compatible { .. }));
  }

  #[test]
  fn get_tool_mstv_entry_clippy_aliases_resolve_to_same_entry() {
    // clippy-driver / cargo-clippy are alternate binary names for the same
    // logical "clippy" tool; mstv::get_tool_mstv_entry must alias them to the
    // single `clippy` registry entry rather than treating them as unknown.
    let canonical =
      mstv::get_tool_mstv_entry("clippy").expect("clippy registered");
    let via_driver = mstv::get_tool_mstv_entry("clippy-driver")
      .expect("clippy-driver aliases");
    let via_cargo =
      mstv::get_tool_mstv_entry("cargo-clippy").expect("cargo-clippy aliases");

    assert_eq!(canonical.binary, "clippy");
    assert_eq!(via_driver.binary, "clippy");
    assert_eq!(via_cargo.binary, "clippy");
    assert_eq!(canonical.min_version, via_driver.min_version);
    assert_eq!(canonical.min_version, via_cargo.min_version);
  }

  #[test]
  fn from_str_trait() {
    let parsed: Result<Version, _> = "3.5.1".parse();
    assert_eq!(parsed, Ok(Version::new(3, 5, 1)));

    let bad: Result<Version, _> = "invalid-ver".parse();
    assert!(bad.is_err());
  }

  #[test]
  fn live_probe_rustfmt() {
    if which::which("rustfmt").is_ok() {
      let ver = probe_tool_version("rustfmt");
      assert!(ver.is_some(), "Expected rustfmt version to be parsed");
      let mstv = minimum_supported_tool_version("rustfmt").unwrap();
      let raw = get_raw_tool_version("rustfmt");
      let status = evaluate_tool_status(ver, raw, Some(&mstv), None);
      assert!(
        matches!(status, ToolStatus::Compatible { .. }),
        "rustfmt should satisfy MSTV 1.4.0"
      );
    }
  }

  #[test]
  fn test_normalize_probed_version() {
    assert_eq!(
      normalize_probed_version(
        "rustfmt",
        "rustfmt 1.7.0 (7576e26b 2024-05-07)"
      ),
      Some(Version::new(1, 7, 0))
    );
    assert_eq!(
      normalize_probed_version("ruff", "ruff 0.9.6"),
      Some(Version::new(0, 9, 6))
    );

    // Clippy 0.1.X is remapped to 1.X.0 for clippy, clippy-driver, and cargo-clippy
    assert_eq!(
      normalize_probed_version("clippy", "clippy 0.1.65 (rustc 1.65.0)"),
      Some(Version::new(1, 65, 0))
    );
    assert_eq!(
      normalize_probed_version("clippy-driver", "clippy 0.1.70"),
      Some(Version::new(1, 70, 0))
    );
    assert_eq!(
      normalize_probed_version(
        "cargo-clippy",
        "clippy 0.1.80-nightly (rustc 1.80.0)"
      ),
      Some(Version::with_prerelease(1, 80, 0, "nightly"))
    );

    // Non-clippy tools with 0.1.X are NOT remapped
    assert_eq!(
      normalize_probed_version("other-tool", "other-tool 0.1.65"),
      Some(Version::new(0, 1, 65))
    );

    // Invalid strings return None
    assert_eq!(normalize_probed_version("rustfmt", ""), None);
    assert_eq!(normalize_probed_version("clippy", "invalid banner"), None);
  }

  #[test]
  fn tool_version_store_serialization_roundtrip() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    let mut store = ToolVersionStore::default();
    store.tools.insert(
      "rustfmt".to_string(),
      ToolVersionEntry {
        raw_version: "rustfmt 1.7.0".to_string(),
        last_checked_unix: 1_700_000_000,
        binary_mtime_unix: 1_699_999_000,
        binary_path: Some("/bin/rustfmt".to_string()),
      },
    );
    store.tools.insert(
      "ruff".to_string(),
      ToolVersionEntry {
        raw_version: "ruff 0.9.6".to_string(),
        last_checked_unix: 1_700_000_100,
        binary_mtime_unix: 1_699_999_100,
        binary_path: None,
      },
    );

    write_tool_version_cache_at(&cache_path, &store);
    let loaded = read_tool_version_cache_at(&cache_path).expect("valid cache");
    assert_eq!(loaded, store);
    assert_eq!(loaded.tools.len(), 2);
    assert_eq!(
      loaded.tools.get("rustfmt").unwrap().raw_version,
      "rustfmt 1.7.0"
    );
    assert_eq!(loaded.tools.get("ruff").unwrap().raw_version, "ruff 0.9.6");
  }

  #[test]
  fn tool_version_cache_miss_populates_cache() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    assert!(read_tool_version_cache_at(&cache_path).is_none());

    if which::which("rustfmt").is_ok() {
      let raw = get_raw_tool_version_at("rustfmt", &cache_path);
      assert!(raw.is_some(), "Expected rustfmt raw output");

      let cache = read_tool_version_cache_at(&cache_path)
        .expect("cache file should be created on miss");
      let entry = cache
        .tools
        .get("rustfmt")
        .expect("rustfmt entry should be cached");
      assert_eq!(Some(&entry.raw_version), raw.as_ref());
      assert!(entry.last_checked_unix > 0);
    }
  }

  #[test]
  fn tool_version_cache_hit_avoids_subprocess() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    if which::which("rustfmt").is_ok() {
      let (bin_path, bin_mtime) =
        resolve_binary_info("rustfmt").expect("rustfmt binary info");

      let now = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

      let mut store = ToolVersionStore::default();
      store.tools.insert(
        "rustfmt".to_string(),
        ToolVersionEntry {
          raw_version: "rustfmt 99.88.77 (mocked cache hit)".to_string(),
          last_checked_unix: now,
          binary_mtime_unix: bin_mtime,
          binary_path: Some(bin_path.to_string_lossy().to_string()),
        },
      );
      write_tool_version_cache_at(&cache_path, &store);

      let raw = get_raw_tool_version_at("rustfmt", &cache_path);
      assert_eq!(raw, Some("rustfmt 99.88.77 (mocked cache hit)".to_string()));

      let probed = probe_tool_version_at("rustfmt", &cache_path);
      assert_eq!(probed, Some(Version::new(99, 88, 77)));
    }
  }

  #[test]
  fn tool_version_cache_mtime_invalidation() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    if which::which("rustfmt").is_ok() {
      let (bin_path, bin_mtime) =
        resolve_binary_info("rustfmt").expect("rustfmt binary info");

      let now = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

      // Cache has mismatched mtime (e.g. tool binary was upgraded on disk)
      let stale_mtime = bin_mtime.wrapping_sub(500);
      let mut store = ToolVersionStore::default();
      store.tools.insert(
        "rustfmt".to_string(),
        ToolVersionEntry {
          raw_version: "rustfmt 99.88.77 (stale mtime)".to_string(),
          last_checked_unix: now,
          binary_mtime_unix: stale_mtime,
          binary_path: Some(bin_path.to_string_lossy().to_string()),
        },
      );
      write_tool_version_cache_at(&cache_path, &store);

      let raw = get_raw_tool_version_at("rustfmt", &cache_path);
      assert_ne!(
        raw,
        Some("rustfmt 99.88.77 (stale mtime)".to_string()),
        "mismatched mtime should invalidate cached version"
      );

      let updated_cache = read_tool_version_cache_at(&cache_path)
        .expect("cache file should be updated");
      let entry = updated_cache.tools.get("rustfmt").unwrap();
      assert_eq!(entry.binary_mtime_unix, bin_mtime);
    }
  }

  #[test]
  fn tool_version_cache_ttl_invalidation() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    if which::which("rustfmt").is_ok() {
      let (bin_path, bin_mtime) =
        resolve_binary_info("rustfmt").expect("rustfmt binary info");

      let now = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

      // Cache has expired timestamp (older than TTL)
      let expired_time = now - (TOOL_VERSION_CACHE_TTL_SECS + 120);
      let mut store = ToolVersionStore::default();
      store.tools.insert(
        "rustfmt".to_string(),
        ToolVersionEntry {
          raw_version: "rustfmt 99.88.77 (expired TTL)".to_string(),
          last_checked_unix: expired_time,
          binary_mtime_unix: bin_mtime,
          binary_path: Some(bin_path.to_string_lossy().to_string()),
        },
      );
      write_tool_version_cache_at(&cache_path, &store);

      let raw = get_raw_tool_version_at("rustfmt", &cache_path);
      assert_ne!(
        raw,
        Some("rustfmt 99.88.77 (expired TTL)".to_string()),
        "expired TTL should invalidate cached version"
      );

      let updated_cache = read_tool_version_cache_at(&cache_path)
        .expect("cache file should be updated");
      let entry = updated_cache.tools.get("rustfmt").unwrap();
      assert!(entry.last_checked_unix >= now);
    }
  }

  #[test]
  fn tool_version_cache_corrupted_json_resilience() {
    let temp = tempfile::TempDir::new().unwrap();
    let cache_path = temp.path().join("tool_versions.json");

    std::fs::write(&cache_path, "not valid json {{{").unwrap();
    assert_eq!(read_tool_version_cache_at(&cache_path), None);

    if which::which("rustfmt").is_ok() {
      let raw = get_raw_tool_version_at("rustfmt", &cache_path);
      assert!(raw.is_some());

      let updated = read_tool_version_cache_at(&cache_path).expect(
        "corrupted cache should be cleanly overwritten with valid JSON",
      );
      assert!(updated.tools.contains_key("rustfmt"));
    }
  }

  /// The literal usage text `gofmt` prints on a failed `--version` — one line of
  /// which contains "10" in `-e`'s description. The digit-scan predicate (#137's
  /// `classify_token`) must reject every line: none carries a version token, so
  /// no help-text line is ever scraped as a version (Fixes #114).
  #[test]
  fn gofmt_usage_text_is_never_scraped_as_a_version() {
    const GOFMT_USAGE: &str = "flag provided but not defined: -version\n\
  usage: gofmt [flags] [path ...]\n  \
  -cpuprofile string\n    \twrite cpu profile to this file\n  \
  -d\tdisplay diffs instead of rewriting files\n  \
  -e\treport all errors (not just the first 10 on different lines)\n  \
  -l\tlist files whose formatting differs from gofmt's\n  \
  -r string\n    \trewrite rule (e.g., 'a[b:len(a)] -> a[b:]')\n  \
  -s\tsimplify code\n  \
  -w\twrite result to (source) file instead of stdout";

    assert_eq!(
      first_versionish_line(GOFMT_USAGE),
      None,
      "a gofmt usage line was picked as version-shaped"
    );
    for line in GOFMT_USAGE.lines() {
      assert_eq!(
        Version::extract(line),
        None,
        "usage line parsed as a version: {line:?}"
      );
    }
  }

  /// gofmt's version is the Go toolchain's, read from `go version`
  /// (`go version goX.Y.Z <os>/<arch>` on stdout). The version-bearing line is
  /// picked and normalizes to the bare `MAJOR.MINOR.PATCH` (Fixes #114).
  #[test]
  fn gofmt_version_comes_from_go_version_output() {
    let line = first_versionish_line("go version go1.27.0 windows/amd64\n")
      .expect("the `go version` line is version-shaped");
    assert_eq!(line, "go version go1.27.0 windows/amd64");
    assert_eq!(
      normalize_probed_version("gofmt", &line),
      Some(Version::new(1, 27, 0))
    );
    assert_eq!(
      normalize_probed_version("gofmt", "go version go1.21.5 darwin/arm64"),
      Some(Version::new(1, 21, 5))
    );
  }

  /// End to end through the uncached probe: with `go` on PATH, gofmt reports a
  /// real, parseable toolchain version sourced from `go version`; with `go`
  /// absent it returns `None` (doctor renders `(version unprobeable)`), never a
  /// scraped usage line (Fixes #114).
  #[test]
  fn probe_raw_gofmt_sources_go_toolchain_or_reports_nothing() {
    let raw = probe_raw_tool_version_uncached("gofmt");
    if which::which("go").is_ok() {
      let raw = raw.expect("go on PATH: gofmt version should probe");
      assert!(
        raw.starts_with("go version") || raw.starts_with("go1"),
        "gofmt version should come from `go version`, got: {raw:?}"
      );
      assert!(
        !raw.contains("report all errors"),
        "gofmt usage text leaked into the version: {raw:?}"
      );
      assert!(
        normalize_probed_version("gofmt", &raw).is_some(),
        "probed gofmt version should parse, got: {raw:?}"
      );
    } else {
      assert_eq!(
        raw, None,
        "go absent: gofmt must be unprobeable, not scraped help text"
      );
    }
  }

  /// Pins the shape of the registry: the three tools that do not answer a plain
  /// `--version` declare exactly why, and every other entry uses the
  /// conventional flag (Fixes #177).
  ///
  /// This is a change-detector, and one-directional by construction. It catches
  /// an entry that acquires an *unexpected* probe, and the loop below forces a
  /// new non-default entry to be named here — which is where the "why" comment
  /// gets written. It cannot catch the converse: an entry wrongly declaring the
  /// default for a tool that has no `--version`, which is #114's own bug.
  /// Proving that needs the tools themselves executed, which is what
  /// `no_installed_registry_tool_prints_a_version_the_probe_discards` does
  /// — a sweep deferred out of #177 because it failed on taplo's npm build, and
  /// landed with #176 once that cause was fixed.
  #[test]
  fn registry_probe_strategies_match_what_each_tool_supports() {
    let probe_of = |binary: &str| {
      mstv::get_tool_mstv_entry(binary)
        .unwrap_or_else(|| panic!("{binary} should be in the MSTV registry"))
        .probe
    };

    // `gofmt` ships with the Go toolchain and has no version flag of its own.
    assert_eq!(
      probe_of("gofmt"),
      mstv::VersionProbe::ViaBinary {
        bin: "go",
        args: &[mstv::ProbeArg::Literal("version")],
        extractor: mstv::ProbeExtractor::FirstVersionishLine,
      }
    );
    // `goimports` has no version flag; its module version is reported by
    // `go version -m <path>` from the `mod` line (Fixes #178).
    // Note: the version reported is the golang.org/x/tools module version
    // that goimports was built from, not goimports' own release version.
    assert_eq!(
      probe_of("goimports"),
      mstv::VersionProbe::ViaBinary {
        bin: "go",
        args: &[
          mstv::ProbeArg::Literal("version"),
          mstv::ProbeArg::Literal("-m"),
          mstv::ProbeArg::ToolPath,
        ],
        extractor: mstv::ProbeExtractor::GoModuleVersion,
      }
    );
    // `golangci-lint` uses a bare `version` subcommand, not `--version` — and
    // declares only that, with no implicit `-v` second attempt behind it.
    assert_eq!(
      probe_of("golangci-lint"),
      mstv::VersionProbe::OwnFlags(&["version"])
    );
    // Rustup ships no `clippy` binary — the component answers as
    // `clippy-driver`, or through `cargo clippy`.
    assert_eq!(
      probe_of("clippy"),
      mstv::VersionProbe::FirstOf(&[
        mstv::VersionProbe::ViaBinary {
          bin: "clippy-driver",
          args: &[mstv::ProbeArg::Literal("--version")],
          extractor: mstv::ProbeExtractor::FirstVersionishLine,
        },
        mstv::VersionProbe::ViaBinary {
          bin: "cargo",
          args: &[
            mstv::ProbeArg::Literal("clippy"),
            mstv::ProbeArg::Literal("--version")
          ],
          extractor: mstv::ProbeExtractor::FirstVersionishLine,
        },
      ])
    );

    for entry in mstv::TOOL_MSTV_REGISTRY {
      if matches!(
        entry.binary,
        "gofmt" | "goimports" | "golangci-lint" | "clippy"
      ) {
        continue;
      }
      assert_eq!(
        entry.probe,
        mstv::DEFAULT_VERSION_PROBE,
        "{} declares a non-default probe with no comment explaining why",
        entry.binary
      );
    }
  }

  /// Structural invariants every declaration must hold, whatever tool it is
  /// for: a probe that runs no command, or a `ViaBinary` that just re-runs the
  /// tool itself (which is `OwnFlags`), is a malformed entry.
  #[test]
  fn every_declared_probe_is_structurally_well_formed() {
    fn check(binary: &str, probe: &mstv::VersionProbe) {
      match probe {
        mstv::VersionProbe::OwnFlags(flags) => assert!(
          !flags.is_empty(),
          "{binary} declares OwnFlags with no flags, which would run the tool bare"
        ),
        mstv::VersionProbe::ViaBinary {
          bin,
          args,
          extractor,
        } => {
          assert_ne!(
            *bin, binary,
            "{binary} declares ViaBinary against itself; that is OwnFlags"
          );
          assert!(!args.is_empty(), "{binary} declares ViaBinary with no args");
          if *extractor == mstv::ProbeExtractor::GoModuleVersion {
            assert!(
              args.contains(&mstv::ProbeArg::ToolPath),
              "{binary} declares GoModuleVersion without mstv::ProbeArg::ToolPath"
            );
          }
        }
        mstv::VersionProbe::FirstOf(probes) => {
          assert!(
            probes.len() > 1,
            "{binary} declares FirstOf with fewer than two alternatives"
          );
          for inner in *probes {
            assert!(
              !matches!(inner, mstv::VersionProbe::FirstOf(_)),
              "{binary} nests FirstOf inside FirstOf; flatten it"
            );
            check(binary, inner);
          }
        }
      }
    }

    for entry in mstv::TOOL_MSTV_REGISTRY {
      check(entry.binary, &entry.probe);
    }
    check("<default>", &mstv::DEFAULT_VERSION_PROBE);
  }

  /// The default probe is the only place the `--version` then `-v` sequence is
  /// declared, and it is declared as data: `run_probe` adds no second attempt
  /// of its own, so `OwnFlags` runs exactly one command (Fixes #177).
  #[test]
  fn default_probe_declares_its_short_flag_fallback_as_data() {
    assert_eq!(
      mstv::DEFAULT_VERSION_PROBE,
      mstv::VersionProbe::FirstOf(&[
        mstv::VersionProbe::OwnFlags(&["--version"]),
        mstv::VersionProbe::OwnFlags(&["-v"]),
      ]),
      "the -v fallback must be visible in the declaration, not hidden in run_probe"
    );
  }

  /// A binary with no registry entry falls back to the conventional
  /// `--version`, and clippy's aliases resolve to clippy's own chain rather
  /// than to that default (Fixes #177).
  #[test]
  fn version_probe_for_defaults_and_resolves_aliases() {
    assert_eq!(
      version_probe_for("some-tool-not-in-the-registry"),
      mstv::DEFAULT_VERSION_PROBE
    );
    assert_eq!(version_probe_for("rustfmt"), mstv::DEFAULT_VERSION_PROBE);
    for alias in ["clippy", "clippy-driver", "cargo-clippy"] {
      assert!(
        matches!(version_probe_for(alias), mstv::VersionProbe::FirstOf(_)),
        "{alias} should resolve to clippy's probe chain"
      );
    }
  }

  /// `FirstOf` skips a probe whose binary does not exist and takes the next
  /// one that yields a version — the property clippy's two distribution shapes
  /// and the default probe's `-v` fallback both depend on (Fixes #177).
  #[test]
  fn first_of_falls_through_to_the_next_working_probe() {
    if which::which("cargo").is_err() {
      return;
    }
    let probe = mstv::VersionProbe::FirstOf(&[
      mstv::VersionProbe::ViaBinary {
        bin: "formality-no-such-binary-exists",
        args: &[mstv::ProbeArg::Literal("--version")],
        extractor: mstv::ProbeExtractor::FirstVersionishLine,
      },
      mstv::VersionProbe::ViaBinary {
        bin: "cargo",
        args: &[mstv::ProbeArg::Literal("--version")],
        extractor: mstv::ProbeExtractor::FirstVersionishLine,
      },
    ]);
    let raw = run_probe("cargo", &probe)
      .expect("the second probe in the chain should report cargo's version");
    assert!(
      raw.contains("cargo"),
      "expected cargo's version banner, got: {raw:?}"
    );
  }

  /// The literal streams the npm `@taplo/cli` build produces for `--version`:
  /// a real version on stdout, nothing on stderr, and exit 1. Reproduced against
  /// `@taplo/cli@0.7.0` (which wraps taplo 0.9.0) on 2026-09-04.
  const TAPLO_NPM_VERSION_STDOUT: &str = "taplo 0.9.0\n";

  /// The literal stdout the same build produces for `-v` — clap's
  /// unknown-argument error, also on stdout, also exit 1. No line carries a
  /// version-shaped token, which is what keeps the relaxation below honest.
  const TAPLO_NPM_SHORT_FLAG_STDOUT: &str = "error: Found argument '-v' which \
  wasn't expected, or isn't valid in this context\n\n\tIf you tried to supply \
  `-v` as a value rather than a flag, use `-- -v`\n\nUSAGE:\n    taplo \
  [OPTIONS] <SUBCOMMAND>\n\nFor more information try --help\n";

  /// A version-shaped line on stdout is kept even when the command exits
  /// non-zero: the npm `@taplo/cli` build prints `taplo 0.9.0` and *then* exits
  /// 1, and the old exit-status gate read that version and discarded it, so
  /// `fml doctor` said `(version unprobeable)` for a tool that had just told it
  /// the answer (Fixes #176).
  ///
  /// This is the assertion the fix exists for: with the gate restored it
  /// returns `None`, which no other path in this function produces for this
  /// input.
  #[test]
  fn version_shaped_stdout_survives_a_non_zero_exit() {
    assert_eq!(
      version_line_from_probe_output(false, TAPLO_NPM_VERSION_STDOUT, ""),
      Some("taplo 0.9.0".to_string()),
      "taplo's real version was discarded because the command exited non-zero"
    );
    assert_eq!(
      normalize_probed_version("taplo", "taplo 0.9.0"),
      Some(Version::new(0, 9, 0))
    );
  }

  /// The relaxation is not "trust a failed command": a non-zero exit whose
  /// stdout carries no version-shaped token is still unprobeable. Both real
  /// shapes — the tool's own usage/error text, and no output at all — must
  /// stay `None`, or #114's garbage-scraping bug comes back through the door
  /// this fix opens (Fixes #176).
  #[test]
  fn non_zero_exit_without_a_version_token_stays_unprobeable() {
    assert_eq!(
      version_line_from_probe_output(false, TAPLO_NPM_SHORT_FLAG_STDOUT, ""),
      None,
      "clap's unknown-argument text was scraped as a version"
    );
    assert_eq!(version_line_from_probe_output(false, "", ""), None);
    assert_eq!(
      version_line_from_probe_output(
        false,
        "flag provided but not defined: -version\n",
        ""
      ),
      None
    );
  }

  /// The relaxation is stdout-only. A failed command's stderr is where it
  /// explains its failure, and scraping that is the path #167 deliberately
  /// removed — accepting non-zero-exit stdout must not bring it back
  /// (Fixes #176). On a *successful* exit stderr is still read, which is the
  /// only way `google-java-format` reports at all.
  #[test]
  fn failed_probe_never_scrapes_stderr_but_a_successful_one_still_does() {
    assert_eq!(
      version_line_from_probe_output(false, "", "some-tool 1.2.3\n"),
      None,
      "a failed probe scraped stderr; #167 removed that path"
    );
    assert_eq!(
      version_line_from_probe_output(
        true,
        "",
        "google-java-format: Version 1.28.0\n"
      ),
      Some("google-java-format: Version 1.28.0".to_string())
    );
    // stdout wins over stderr on the success path, as before.
    assert_eq!(
      version_line_from_probe_output(true, "tool 2.0.0\n", "tool 9.9.9\n"),
      Some("tool 2.0.0".to_string())
    );
  }

  /// Every `(binary, args)` pair the declared `probe` would actually execute,
  /// flattened out of any [`mstv::VersionProbe::FirstOf`] chain.
  fn probe_leaf_commands(
    binary: &str,
    probe: &mstv::VersionProbe,
  ) -> Vec<(String, Vec<String>)> {
    match probe {
      mstv::VersionProbe::OwnFlags(flags) => vec![(
        binary.to_string(),
        flags.iter().map(|a| (*a).to_string()).collect(),
      )],
      mstv::VersionProbe::ViaBinary { bin, args, .. } => {
        if let Some(rendered) = render_probe_args(binary, args) {
          vec![(
            bin.to_string(),
            rendered
              .into_iter()
              .map(|s| s.to_string_lossy().into_owned())
              .collect(),
          )]
        } else {
          vec![]
        }
      }
      mstv::VersionProbe::FirstOf(probes) => probes
        .iter()
        .flat_map(|inner| probe_leaf_commands(binary, inner))
        .collect(),
    }
  }

  /// Presence-gated execution sweep across the whole registry (#176's AC 5,
  /// deferred here from PR #195 because it failed on taplo — the bug this PR
  /// fixes).
  ///
  /// For every entry whose declared probe would run a binary that is actually
  /// installed, run those commands and assert the fleet-wide property #114 and
  /// #176 are both instances of: **if a tool prints a version-shaped line on
  /// stdout, formality must not report it as unprobeable.** Unlike
  /// `registry_probe_strategies_match_what_each_tool_supports`, this
  /// executes the tools, so it can catch an entry whose declaration silently
  /// disagrees with the tool — the #114-class bug a change-detector cannot see.
  ///
  /// It is deliberately an implication, not "every installed tool probes": a
  /// genuinely broken install (a `ktlint` shim that cannot exec its JDK, exit
  /// 126 with empty stdout) prints nothing version-shaped, so the premise is
  /// false and the entry is skipped rather than failing a test for something
  /// that is not a declaration bug.
  #[test]
  fn no_installed_registry_tool_prints_a_version_the_probe_discards() {
    for entry in mstv::TOOL_MSTV_REGISTRY {
      let leaves = probe_leaf_commands(entry.binary, &entry.probe);
      if !leaves.iter().any(|(bin, _)| which::which(bin).is_ok()) {
        continue;
      }

      let printed = leaves.iter().find_map(|(bin, args)| {
        let output = tooling::create_tool_command(bin)
          .args(args.iter().map(String::as_str))
          .output()
          .ok()?;
        let line =
          first_versionish_line(&String::from_utf8_lossy(&output.stdout))?;
        Some((format!("{bin} {}", args.join(" ")), line))
      });
      let Some((command, line)) = printed else {
        continue;
      };

      assert!(
        probe_raw_tool_version_uncached(entry.binary).is_some(),
        "`{command}` printed {line:?} on stdout, but {} still probes as \
         unprobeable",
        entry.binary
      );
    }
  }

  /// Tests `parse_go_version_m` extracts the module version from real `go version -m` output.
  /// Note: The version reported is the `golang.org/x/tools` module version, not goimports' own (Fixes #178).
  #[test]
  fn parse_go_version_m_extracts_module_version_from_real_output() {
    let output = "\
  C:\\Users\\olives\\go\\bin\\goimports.exe: go1.26.7
  \tpath\tgolang.org/x/tools/cmd/goimports
  \tmod\tgolang.org/x/tools\tv0.49.0\th1:3NI7VXzL9+1WZD52Dx2ttoPwD5DWrFGpl9mFZDlmisI=
  \tdep\tgolang.org/x/mod\tv0.39.0\th1:UF5zwQdCRRUpHfyPwr7d4UrGiVeldIsogtzWVnczL74=
  \tdep\tgolang.org/x/sync\tv0.22.0\th1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek=
  \tbuild\t-compiler=gc
  ";
    let parsed =
      parse_go_version_m(output).expect("should extract module version");
    assert_eq!(parsed, "v0.49.0");

    let ver = normalize_probed_version("goimports", &parsed)
      .expect("extracted version should normalize to semver");
    assert_eq!(ver, Version::new(0, 49, 0));
  }

  /// Tests `parse_go_version_m` with space-separated columns and without checksum hash.
  #[test]
  fn parse_go_version_m_without_hash() {
    let output = "\
  /home/user/go/bin/goimports: go1.24.7
          path    golang.org/x/tools/cmd/goimports
          mod     golang.org/x/tools      v0.28.0
  ";
    let parsed =
      parse_go_version_m(output).expect("should extract module version");
    assert_eq!(parsed, "v0.28.0");

    let ver = normalize_probed_version("goimports", &parsed)
      .expect("extracted version should normalize to semver");
    assert_eq!(ver, Version::new(0, 28, 0));
  }

  /// Tests `parse_go_version_m` extracts pseudo-versions cleanly.
  #[test]
  fn parse_go_version_m_pseudo_version() {
    let output = "\
  /path/to/goimports: go1.24.7
  \tpath\tgolang.org/x/tools/cmd/goimports
  \tmod\tgolang.org/x/tools\tv0.0.0-20260811182544-a038080d80e5\th1:ZUSxONxc981v7AW7QUg+I9WwZzSTTJ019ENBYr5pV/Q=
  ";
    let parsed =
      parse_go_version_m(output).expect("should extract pseudo-version");
    assert_eq!(parsed, "v0.0.0-20260811182544-a038080d80e5");

    let ver = normalize_probed_version("goimports", &parsed)
      .expect("extracted pseudo-version should normalize to semver");
    assert_eq!(ver.major, 0);
    assert_eq!(ver.minor, 0);
    assert_eq!(ver.patch, 0);
  }

  /// Degradation path 1: output reports `(devel)` (built from local working copy)
  /// degrades cleanly to `None` (so doctor surfaces `(version unprobeable)`) (Fixes #178).
  #[test]
  fn parse_go_version_m_degrades_cleanly_on_devel() {
    let output = "\
  /home/user/go/bin/goimports: go1.24.7
          path    golang.org/x/tools/cmd/goimports
          mod     golang.org/x/tools      (devel)
  ";
    assert_eq!(
      parse_go_version_m(output),
      None,
      "(devel) must degrade to None, never a junk string"
    );
  }

  /// Degradation path 2: output has no `mod` line (e.g. stripped or non-module binary)
  /// degrades cleanly to `None` (so doctor surfaces `(version unprobeable)`) (Fixes #178).
  #[test]
  fn parse_go_version_m_degrades_cleanly_without_mod_line() {
    let output_without_mod = "\
  /home/user/go/bin/goimports: go1.24.7
          path    golang.org/x/tools/cmd/goimports
  ";
    assert_eq!(
      parse_go_version_m(output_without_mod),
      None,
      "missing mod line must degrade to None"
    );

    assert_eq!(
      parse_go_version_m(""),
      None,
      "empty output must degrade to None"
    );

    let output_not_go = "go: /path/to/goimports: not a Go executable\n";
    assert_eq!(
      parse_go_version_m(output_not_go),
      None,
      "non-Go output must degrade to None"
    );
  }

  /// Tests `render_probe_args` handles `Literal` and `ToolPath` cleanly without
  /// requiring goimports on PATH.
  #[test]
  fn render_probe_args_resolution() {
    let literal_args = &[
      mstv::ProbeArg::Literal("version"),
      mstv::ProbeArg::Literal("-m"),
    ];
    let rendered = render_probe_args("any-binary", literal_args)
      .expect("literal args should always render");
    assert_eq!(
      rendered,
      vec![ffi::OsString::from("version"), ffi::OsString::from("-m")]
    );

    // Missing binary with ToolPath must return None (clean degradation).
    let tool_path_args = &[mstv::ProbeArg::ToolPath];
    assert_eq!(
      render_probe_args(
        "fml-nonexistent-binary-for-testing-xyz",
        tool_path_args
      ),
      None
    );

    // Existing binary (cargo) with ToolPath resolves to its location.
    if let Ok(cargo_path) = which::which("cargo") {
      let mixed_args =
        &[mstv::ProbeArg::Literal("check"), mstv::ProbeArg::ToolPath];
      let rendered = render_probe_args("cargo", mixed_args)
        .expect("cargo should resolve on PATH");
      assert_eq!(rendered.len(), 2);
      assert_eq!(rendered[0], "check");
      assert_eq!(rendered[1], cargo_path.into_os_string());
    }
  }

  /// Live uncached probe test: when both `go` and `goimports` are on PATH,
  /// goimports reports a real, parseable module version sourced from `go version -m <path>`;
  /// when either is absent it returns `None` (doctor renders `(version unprobeable)`) (Fixes #178).
  #[test]
  fn probe_raw_goimports_sources_module_version_or_reports_nothing() {
    let raw = probe_raw_tool_version_uncached("goimports");
    if which::which("go").is_ok() && which::which("goimports").is_ok() {
      let raw = raw.expect("go and goimports on PATH: version should probe");
      assert!(
        raw.starts_with('v') || raw.starts_with('V'),
        "goimports version should be a module version starting with v, got: {raw:?}"
      );
      assert!(
        normalize_probed_version("goimports", &raw).is_some(),
        "probed goimports version should parse, got: {raw:?}"
      );
    } else {
      assert_eq!(
        raw, None,
        "go or goimports absent: goimports must be unprobeable"
      );
    }
  }

  #[test]
  fn version_extract_raw_and_extract_with_raw() {
    assert_eq!(
      Version::extract_raw("clang-tidy version 14.0.0-1ubuntu1"),
      Some("14.0.0-1ubuntu1")
    );
    assert_eq!(
      Version::extract_with_raw("clang-tidy version 14.0.0-1ubuntu1"),
      Some((Version::new(14, 0, 0), "14.0.0-1ubuntu1"))
    );
    assert_eq!(
      Version::extract_raw("Ubuntu clang-tidy version 18.1.8-0ubuntu1~22.04.1"),
      Some("18.1.8-0ubuntu1~22.04.1")
    );
    assert_eq!(
      Version::extract_raw("yamllint 1.35.1.post1"),
      Some("1.35.1.post1")
    );
    assert_eq!(Version::extract_raw("ruff 0.9.6.dev0"), Some("0.9.6.dev0"));
    assert_eq!(
      Version::extract_raw("rustfmt 1.7.0-nightly"),
      Some("1.7.0-nightly")
    );
    assert_eq!(Version::extract_raw("rustfmt 1.8.0"), Some("1.8.0"));
    assert_eq!(Version::extract_raw("rustfmt v1.8.0"), Some("1.8.0"));
    assert_eq!(
      Version::extract_raw("clippy 0.1.65 (commit abc)"),
      Some("0.1.65")
    );
    assert_eq!(Version::extract_raw("no version here"), None);
  }

  #[test]
  fn test_reported_raw_version_if_differing() {
    // Distro suffix differs from normalized version
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(14, 0, 0),
        Some("clang-tidy version 14.0.0-1ubuntu1")
      ),
      Some("14.0.0-1ubuntu1")
    );
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(18, 1, 8),
        Some("Ubuntu clang-tidy version 18.1.8-0ubuntu1~22.04.1")
      ),
      Some("18.1.8-0ubuntu1~22.04.1")
    );
    // Post/dev suffix differs
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(1, 35, 1),
        Some("yamllint 1.35.1.post1")
      ),
      Some("1.35.1.post1")
    );

    // Clippy remapping: normalized 1.65.0 differs from reported 0.1.65
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(1, 65, 0),
        Some("clippy 0.1.65 (commit abc)")
      ),
      Some("0.1.65")
    );

    // Identical versions: returns None
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(14, 0, 0),
        Some("clang-tidy version 14.0.0")
      ),
      None
    );
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(14, 0, 0),
        Some("clang-tidy version v14.0.0")
      ),
      None
    );
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::with_prerelease(1, 7, 0, "nightly"),
        Some("rustfmt 1.7.0-nightly")
      ),
      None
    );

    // Banner without version or None
    assert_eq!(
      reported_raw_version_if_differing(&Version::new(14, 0, 0), None),
      None
    );
    assert_eq!(
      reported_raw_version_if_differing(
        &Version::new(14, 0, 0),
        Some("no version string")
      ),
      None
    );
  }
}
