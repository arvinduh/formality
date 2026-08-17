pub mod cpp;
pub mod json;
pub mod markdown;
pub mod native;
pub mod python;
pub mod rust;
pub mod toml;
pub mod typst;
pub mod yaml;

pub use native::{
  AUTO_GENERATED_HEADER, EDITORCONFIG_FILE_NAME, NativeConfig,
  generate_editorconfig, generate_editorconfig_from_config,
  render_native_config, serialize_json_pretty,
  serialize_toml_with_header, serialize_yaml_with_header,
  sync_editorconfig,
};

use crate::config::{
  FormalityConfig, ResolvedGlobalConfig, ResolvedLangConfig,
};
use crate::diff::render_diff;
pub use crate::facets::{DeclaresFacets, Facet, FacetSupport};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ExecutionContext {
  pub root: PathBuf,
  pub paths: Vec<PathBuf>,
  pub global_config: ResolvedGlobalConfig,
  pub lang_config: ResolvedLangConfig,
  pub check_only: bool,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
  pub binary: &'static str,
  pub description: &'static str,
  pub install_hint: &'static str,
  pub is_required_for_fmt: bool,
  pub is_required_for_lint: bool,
}

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
}

impl InstallMethod {
  fn is_available(&self) -> bool {
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
      InstallMethod::Brew(_) => check_binary_exists("brew"),
      InstallMethod::Scoop(_) => check_binary_exists("scoop"),
      InstallMethod::WingetName(_) | InstallMethod::WingetId(_) => {
        check_binary_exists("winget")
      }
      InstallMethod::Cargo { .. } => check_binary_exists("cargo"),
      InstallMethod::Rustup(_) => check_binary_exists("rustup"),
    }
  }

  fn command(&self) -> (String, Vec<String>) {
    fn strs(v: &[&str]) -> Vec<String> {
      v.iter().map(|s| s.to_string()).collect()
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
  InstallMethod::Brew("yamllint"),
  InstallMethod::Scoop("yamllint"),
  InstallMethod::WingetName("yamllint"),
];

const CLANG_FORMAT_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("clang-format"),
  InstallMethod::Pip("clang-format"),
  InstallMethod::Pip3("clang-format"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

const CLANG_TIDY_CHAIN: &[InstallMethod] = &[
  InstallMethod::Brew("llvm"),
  InstallMethod::WingetName("LLVM.LLVM"),
  InstallMethod::Scoop("llvm"),
];

const RUSTFMT_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("rustfmt")];
const CLIPPY_CHAIN: &[InstallMethod] = &[InstallMethod::Rustup("clippy")];

/// Looks up the ordered installer preference chain for a tool binary name.
/// This is the single place that maps a tool to its installers — adding a
/// new tool means adding a chain constant and one arm here, not copying a
/// whole if/else-if cascade.
fn install_chain_for(binary: &str) -> Option<&'static [InstallMethod]> {
  match binary {
    "taplo" => Some(TAPLO_CHAIN),
    "typstyle" => Some(TYPSTYLE_CHAIN),
    "tinymist" => Some(TINYMIST_CHAIN),
    "ruff" => Some(RUFF_CHAIN),
    "prettier" => Some(PRETTIER_CHAIN),
    "markdownlint-cli2" | "markdownlint" => Some(MARKDOWNLINT_CHAIN),
    "yamllint" => Some(YAMLLINT_CHAIN),
    "clang-format" => Some(CLANG_FORMAT_CHAIN),
    "clang-tidy" => Some(CLANG_TIDY_CHAIN),
    "rustfmt" => Some(RUSTFMT_CHAIN),
    "clippy-driver" => Some(CLIPPY_CHAIN),
    _ => None,
  }
}

impl ToolInfo {
  /// Returns the (program, args) for the first available installer in this
  /// tool's preference chain: prebuilt binary package managers first,
  /// falling back to `cargo install ... --locked` source compilation where
  /// the tool ships as a crate.
  pub fn get_auto_install_cmd(&self) -> Option<(String, Vec<String>)> {
    install_chain_for(self.binary)?
      .iter()
      .find(|method| method.is_available())
      .map(InstallMethod::command)
  }
}

#[derive(Debug, Clone)]
pub enum SurfaceStatus {
  Passed,
  ViolationsFound {
    message: String,
    diff: Option<String>,
  },
  ToolMissing {
    binary: String,
    install_hint: String,
  },
  ExecutionError {
    message: String,
  },
  Skipped {
    reason: String,
  },
  ConfigSynced {
    file: String,
    created: bool,
  },
  ConfigDrifted {
    file: String,
    diff: String,
  },
  /// Existing native config lacks the auto-generation header — it was written
  /// by hand. Overwriting silently would destroy intentional customization.
  ManualConfig {
    file: String,
    suggestion: String,
  },
}

#[derive(Debug, Clone)]
pub struct SurfaceResult {
  pub surface_name: &'static str,
  pub status: SurfaceStatus,
  pub duration: Duration,
}

impl SurfaceResult {
  pub fn is_success(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::Passed
        | SurfaceStatus::Skipped { .. }
        | SurfaceStatus::ConfigSynced { .. }
    )
  }

  pub fn is_violation(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ViolationsFound { .. }
        | SurfaceStatus::ConfigDrifted { .. }
        | SurfaceStatus::ManualConfig { .. }
    )
  }

  pub fn is_error(&self) -> bool {
    matches!(
      self.status,
      SurfaceStatus::ToolMissing { .. } | SurfaceStatus::ExecutionError { .. }
    )
  }
}

pub trait LanguageSurface: DeclaresFacets + Send + Sync {
  fn name(&self) -> &'static str;
  fn display_name(&self) -> &'static str {
    self.name()
  }
  fn aliases(&self) -> &[&'static str] {
    &[]
  }
  fn file_extensions(&self) -> &[&'static str] {
    &[]
  }
  fn detect(&self, root: &Path) -> bool;
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo>;
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult;
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult;
  fn supports_lint_fix(&self) -> bool {
    false
  }
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult;
  fn clone_box(&self) -> Box<dyn LanguageSurface>;
}

impl Clone for Box<dyn LanguageSurface> {
  fn clone(&self) -> Self {
    self.clone_box()
  }
}

/// A constructor function pointer for instantiating a boxed `LanguageSurface`.
pub type SurfaceConstructor = fn() -> Box<dyn LanguageSurface>;

/// Helper function to create a boxed instance of any `Default + LanguageSurface`.
pub fn create_surface<S: LanguageSurface + Default + 'static>()
-> Box<dyn LanguageSurface> {
  Box::new(S::default())
}

/// Canonical table of default fleet surface constructors.
pub static DEFAULT_SURFACE_CONSTRUCTORS: &[SurfaceConstructor] = &[
  create_surface::<rust::RustSurface>,
  create_surface::<python::PythonSurface>,
  create_surface::<cpp::CppSurface>,
  create_surface::<markdown::MarkdownSurface>,
  create_surface::<yaml::YamlSurface>,
  create_surface::<json::JsonSurface>,
  create_surface::<toml::TomlSurface>,
  create_surface::<typst::TypstSurface>,
];

/// Registry for managing, querying, and discovering language surfaces.
#[derive(Clone)]
pub struct SurfaceRegistry {
  surfaces: Vec<Box<dyn LanguageSurface>>,
}

impl Default for SurfaceRegistry {
  fn default() -> Self {
    let mut reg = Self::empty();
    reg.register_surface::<rust::RustSurface>();
    reg.register_surface::<python::PythonSurface>();
    reg.register_surface::<cpp::CppSurface>();
    reg.register_surface::<markdown::MarkdownSurface>();
    reg.register_surface::<yaml::YamlSurface>();
    reg.register_surface::<json::JsonSurface>();
    reg.register_surface::<toml::TomlSurface>();
    reg.register_surface::<typst::TypstSurface>();
    reg
  }
}

impl SurfaceRegistry {
  /// Creates an empty registry with no registered surfaces.
  pub const fn empty() -> Self {
    Self {
      surfaces: Vec::new(),
    }
  }

  /// Creates a registry pre-populated with the default fleet of 8 language surfaces.
  pub fn new() -> Self {
    Self::default()
  }

  /// Registers a concrete boxed surface instance in the registry.
  pub fn register(&mut self, surface: Box<dyn LanguageSurface>) {
    self.surfaces.push(surface);
  }

  /// Registers a surface type that implements `LanguageSurface` and `Default`.
  pub fn register_surface<S: LanguageSurface + Default + 'static>(&mut self) {
    self.surfaces.push(Box::new(S::default()));
  }

  /// Returns a slice of references to all registered surfaces.
  pub fn surfaces(&self) -> &[Box<dyn LanguageSurface>] {
    &self.surfaces
  }

  /// Returns cloned boxed instances of all registered language surfaces.
  pub fn all_surfaces(&self) -> Vec<Box<dyn LanguageSurface>> {
    self.surfaces.clone()
  }

  /// Looks up a surface by canonical name or alias (case-insensitive, trimmed).
  pub fn get_surface_by_name(
    &self,
    name: &str,
  ) -> Option<Box<dyn LanguageSurface>> {
    let query = name.trim();
    self
      .surfaces
      .iter()
      .find(|s| {
        s.name().eq_ignore_ascii_case(query)
          || s.aliases().iter().any(|a| a.eq_ignore_ascii_case(query))
      })
      .cloned()
  }

  /// Resolves an alias or surface name to its canonical surface name (e.g. "rs" -> "rust").
  pub fn resolve_canonical_name(
    &self,
    name_or_alias: &str,
  ) -> Option<&'static str> {
    let query = name_or_alias.trim();
    self
      .surfaces
      .iter()
      .find(|s| {
        s.name().eq_ignore_ascii_case(query)
          || s.aliases().iter().any(|a| a.eq_ignore_ascii_case(query))
      })
      .map(|s| s.name())
  }

  /// Returns the canonical names of all registered surfaces.
  pub fn supported_languages(&self) -> Vec<&'static str> {
    self.surfaces.iter().map(|s| s.name()).collect()
  }

  /// Returns the number of registered surfaces.
  pub fn len(&self) -> usize {
    self.surfaces.len()
  }

  /// Returns whether the registry is empty.
  pub fn is_empty(&self) -> bool {
    self.surfaces.is_empty()
  }

  /// Detects active surfaces within `root` based on filesystem heuristics.
  pub fn detect_surfaces(&self, root: &Path) -> Vec<Box<dyn LanguageSurface>> {
    self
      .surfaces
      .iter()
      .filter(|s| s.detect(root))
      .cloned()
      .collect()
  }

  /// Performs smart detection respecting configuration allowlists and ignore rules.
  pub fn detect_surfaces_smart(
    &self,
    root: &Path,
    config: &FormalityConfig,
  ) -> Vec<Box<dyn LanguageSurface>> {
    let global = config.resolve_global();

    let is_ignored = |name: &str, aliases: &[&'static str]| -> bool {
      if let Some(ref ignores) = global.ignore_languages {
        ignores.iter().any(|ig| {
          ig.eq_ignore_ascii_case(name)
            || aliases.iter().any(|a| a.eq_ignore_ascii_case(ig))
        })
      } else {
        false
      }
    };

    // 1. If explicit `languages` allowlist is defined, use that minus ignore_languages
    if let Some(ref explicit_langs) = global.languages {
      let mut selected = Vec::new();
      for lang_name in explicit_langs {
        if let Some(s) = self.get_surface_by_name(lang_name)
          && !is_ignored(s.name(), s.aliases())
        {
          let resolved = config.resolve_for_lang(s.name());
          if resolved.enabled {
            selected.push(s);
          }
        }
      }
      return selected;
    }

    // 2. Otherwise auto-detect all project surfaces minus ignore_languages
    self
      .surfaces
      .iter()
      .filter(|surface| {
        if is_ignored(surface.name(), surface.aliases()) {
          return false;
        }
        let resolved = config.resolve_for_lang(surface.name());
        if !resolved.enabled {
          return false;
        }
        surface.detect(root)
      })
      .cloned()
      .collect()
  }
}

pub fn all_surfaces() -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().all_surfaces()
}

pub fn detect_surfaces(root: &Path) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces(root)
}

pub fn detect_surfaces_smart(
  root: &Path,
  config: &FormalityConfig,
) -> Vec<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().detect_surfaces_smart(root, config)
}

pub fn get_surface_by_name(name: &str) -> Option<Box<dyn LanguageSurface>> {
  SurfaceRegistry::default().get_surface_by_name(name)
}

pub fn resolve_canonical_name(name_or_alias: &str) -> Option<&'static str> {
  SurfaceRegistry::default().resolve_canonical_name(name_or_alias)
}

/// Helper function to find matching files within a directory ignoring .git, target, node_modules, etc.
pub fn find_files_with_ext(
  root: &Path,
  extensions: &[&str],
  specific_paths: &[PathBuf],
  files_override: &[PathBuf],
  exclude: &[PathBuf],
) -> Vec<PathBuf> {
  let targets = if !specific_paths.is_empty() {
    specific_paths
  } else if !files_override.is_empty() {
    files_override
  } else {
    &[]
  };

  let raw_files = if !targets.is_empty() {
    let mut out = Vec::new();
    for p in targets {
      let full_p = if p.is_absolute() {
        p.clone()
      } else {
        root.join(p)
      };
      if full_p.is_file()
        && let Some(ext) = full_p.extension().and_then(|e| e.to_str())
        && extensions
          .iter()
          .any(|&target| target.eq_ignore_ascii_case(ext))
      {
        out.push(full_p);
      } else if full_p.is_dir() {
        out.extend(walk_dir_ext(&full_p, extensions));
      }
    }
    out
  } else {
    walk_dir_ext(root, extensions)
  };

  if exclude.is_empty() {
    raw_files
  } else {
    raw_files
      .into_iter()
      .filter(|file| !is_excluded(file, root, exclude))
      .collect()
  }
}

pub fn simple_glob_match(pattern: &str, text: &str) -> bool {
  let norm_pattern = pattern.replace('\\', "/");
  let norm_text = text.replace('\\', "/");
  glob_match_slices(norm_pattern.as_bytes(), norm_text.as_bytes())
}

fn glob_match_slices(pattern: &[u8], text: &[u8]) -> bool {
  if pattern.is_empty() {
    return text.is_empty();
  }

  if pattern.starts_with(b"**") {
    let mut rest_pat = &pattern[2..];
    if rest_pat.starts_with(b"/") {
      rest_pat = &rest_pat[1..];
    }
    for i in 0..=text.len() {
      if glob_match_slices(rest_pat, &text[i..]) {
        return true;
      }
    }
    return false;
  }

  if pattern[0] == b'*' {
    let rest_pat = &pattern[1..];
    for i in 0..=text.len() {
      if i > 0 && text[i - 1] == b'/' {
        break;
      }
      if glob_match_slices(rest_pat, &text[i..]) {
        return true;
      }
    }
    return false;
  }

  if text.is_empty() {
    return false;
  }

  if pattern[0] == b'?' {
    if text[0] == b'/' {
      return false;
    }
    return glob_match_slices(&pattern[1..], &text[1..]);
  }

  if pattern[0] == text[0] {
    return glob_match_slices(&pattern[1..], &text[1..]);
  }

  false
}

pub fn is_excluded(path: &Path, root: &Path, exclude: &[PathBuf]) -> bool {
  if exclude.is_empty() {
    return false;
  }
  let rel_path = path.strip_prefix(root).unwrap_or(path);
  let rel_str = rel_path.to_string_lossy().replace('\\', "/");
  let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

  for ex in exclude {
    let ex_str_raw = ex.to_string_lossy();
    let ex_str = ex_str_raw.replace('\\', "/");
    let ex_trimmed = ex_str.trim_matches('/');

    // 1. Direct path prefix or exact match with full / root-relative path
    if path.starts_with(ex) || rel_path.starts_with(ex) {
      return true;
    }
    let full_ex = if ex.is_absolute() {
      ex.clone()
    } else {
      root.join(ex)
    };
    if path.starts_with(&full_ex) {
      return true;
    }

    // 2. Relative prefix, exact relative string match, or directory match
    if rel_str == ex_trimmed || rel_str.starts_with(&format!("{}/", ex_trimmed))
    {
      return true;
    }

    // 3. Filename match
    if file_name == ex_trimmed || file_name == ex_str_raw {
      return true;
    }

    // 4. Any path component matches
    if rel_path.components().any(|c| {
      c.as_os_str().to_string_lossy() == ex_trimmed
        || c.as_os_str() == ex.as_os_str()
    }) {
      return true;
    }

    // 5. Glob / wildcard pattern matching
    if (ex_trimmed.contains('*') || ex_trimmed.contains('?'))
      && (simple_glob_match(ex_trimmed, &rel_str)
        || simple_glob_match(ex_trimmed, file_name))
    {
      return true;
    }
  }

  false
}

fn walk_dir_ext(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
  let mut results = Vec::new();
  let walker = ignore::WalkBuilder::new(dir)
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .filter_entry(|entry| {
      let name = entry.file_name().to_string_lossy();
      name != "target"
        && name != "node_modules"
        && name != ".git"
        && name != ".venv"
        && name != "vendor"
        && name != "fixtures"
    })
    .build();

  for entry in walker.filter_map(Result::ok) {
    let path = entry.path();
    if path.is_file()
      && let Some(ext) = path.extension().and_then(|e| e.to_str())
      && extensions
        .iter()
        .any(|&target| target.eq_ignore_ascii_case(ext))
    {
      results.push(path.to_path_buf());
    }
  }

  results
}

pub fn check_binary_exists(binary: &str) -> bool {
  which::which(binary).is_ok()
}

pub fn has_cargo_binstall() -> bool {
  check_binary_exists("cargo") && check_binary_exists("cargo-binstall")
}

/// Creates a `Command` with proper handling for Windows batch files (.cmd/.bat)
/// such as `npm`, `pnpm`, `yarn`, `npx`, and globally installed node CLIs.
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

/// Returns true if `content` was written by `fml sync` (contains the
/// auto-generation sentinel comment). Used to guard against silently
/// overwriting hand-written configs.
pub fn is_auto_generated(content: &str) -> bool {
  content.contains("DO NOT EDIT")
    || content.contains("Auto-generated by formality")
    || content.contains("WARNING: DO NOT EDIT THIS FILE DIRECTLY!")
}

pub fn sync_file_helper(
  file_path: &Path,
  file_name: &str,
  expected_content: &str,
  check: bool,
  start: Instant,
  surface_name: &'static str,
) -> SurfaceResult {
  let exists = file_path.is_file();
  let current_content = if exists {
    std::fs::read_to_string(file_path).unwrap_or_default()
  } else {
    String::new()
  };

  if current_content.trim() == expected_content.trim() {
    return SurfaceResult {
      surface_name,
      status: SurfaceStatus::Passed,
      duration: start.elapsed(),
    };
  }

  // File exists but was not written by fml — protect it from silent overwrite.
  if exists
    && !current_content.is_empty()
    && !is_auto_generated(&current_content)
  {
    let suggestion = format!(
      "'{file_name}' exists but was not generated by formality.\n\
       It will not be overwritten automatically to avoid destroying manual settings.\n\
       \n\
       To resolve, choose one of:\n\
       \n\
       Option A — Let formality manage the file:\n\
         1. Back up your current settings.\n\
         2. Delete '{file_name}' and run 'fml sync' to generate a clean copy.\n\
         3. Migrate any custom settings you need into formality.toml using\n\
            [lang.<name>] overrides (indent_size, line_length, extra_args, …).\n\
       \n\
       Option B — Keep managing the file yourself:\n\
         Add the following header as the very first block of '{file_name}'\n\
         to suppress this warning and opt out of sync for this file:\n\
         (You will need to run 'fml sync' again after adding the header,\n\
          formality will then leave that file untouched.)\n\
       \n\
       The header that formality looks for:\n\
         # WARNING: DO NOT EDIT THIS FILE DIRECTLY!\n\
         (or the JSON equivalent: a top-level \"$comment\" containing the same text)\n\
       \n\
       Generated config for reference:\n\
       ---\n\
       {expected_content}\n\
       ---"
    );
    return SurfaceResult {
      surface_name,
      status: SurfaceStatus::ManualConfig {
        file: file_name.to_string(),
        suggestion,
      },
      duration: start.elapsed(),
    };
  }

  if check {
    let diff = render_diff(
      &current_content,
      expected_content,
      if exists { file_name } else { "(missing)" },
      &format!("{} (expected)", file_name),
    );
    SurfaceResult {
      surface_name,
      status: SurfaceStatus::ConfigDrifted {
        file: file_name.to_string(),
        diff,
      },
      duration: start.elapsed(),
    }
  } else {
    if let Some(parent) = file_path.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(file_path, expected_content) {
      Ok(_) => SurfaceResult {
        surface_name,
        status: SurfaceStatus::ConfigSynced {
          file: file_name.to_string(),
          created: !exists,
        },
        duration: start.elapsed(),
      },
      Err(e) => SurfaceResult {
        surface_name,
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to write {}: {}", file_name, e),
        },
        duration: start.elapsed(),
      },
    }
  }
}

struct TempFileGuard<'a>(&'a Path);

impl<'a> Drop for TempFileGuard<'a> {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(self.0);
  }
}

/// Executes an in-place formatter on temporary copies of the given files and generates
/// unified diffs between the original content and the formatted content.
///
/// Uses an RAII guard to guarantee that `.fml-check.tmp` files are cleaned up on all exit paths.
pub fn diff_check_via_tempcopy(
  files: &[PathBuf],
  run_in_place: impl Fn(&Path) -> std::io::Result<std::process::Output>,
  surface_name: &'static str,
  start: Instant,
) -> SurfaceResult {
  if files.is_empty() {
    return SurfaceResult {
      surface_name,
      status: SurfaceStatus::Passed,
      duration: start.elapsed(),
    };
  }

  let mut combined_diff = String::new();

  for original in files {
    let original_content = match std::fs::read_to_string(original) {
      Ok(c) => c,
      Err(e) => {
        return SurfaceResult {
          surface_name,
          status: SurfaceStatus::ExecutionError {
            message: format!("Failed to read {}: {}", original.display(), e),
          },
          duration: start.elapsed(),
        };
      }
    };

    let ext = original.extension().and_then(|e| e.to_str()).unwrap_or("");
    let scratch = if ext.is_empty() {
      original.with_extension("fml-check.tmp")
    } else {
      original.with_extension(format!("{}.fml-check.tmp", ext))
    };

    if let Err(e) = std::fs::write(&scratch, &original_content) {
      return SurfaceResult {
        surface_name,
        status: SurfaceStatus::ExecutionError {
          message: format!(
            "Failed to write temp file {}: {}",
            scratch.display(),
            e
          ),
        },
        duration: start.elapsed(),
      };
    }

    let _guard = TempFileGuard(&scratch);

    let output = match run_in_place(&scratch) {
      Ok(out) => out,
      Err(e) => {
        return SurfaceResult {
          surface_name,
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to run formatter for {}: {}",
              original.display(),
              e
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    if !output.status.success() {
      let stderr = String::from_utf8_lossy(&output.stderr).to_string();
      let stdout = String::from_utf8_lossy(&output.stdout).to_string();
      let msg = if !stderr.trim().is_empty() {
        stderr
      } else if !stdout.trim().is_empty() {
        stdout
      } else {
        format!("Formatter failed for {}", original.display())
      };
      return SurfaceResult {
        surface_name,
        status: SurfaceStatus::ViolationsFound {
          message: msg,
          diff: None,
        },
        duration: start.elapsed(),
      };
    }

    let formatted = match std::fs::read_to_string(&scratch) {
      Ok(f) => f,
      Err(e) => {
        return SurfaceResult {
          surface_name,
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to read formatted temp file {}: {}",
              scratch.display(),
              e
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    if formatted != original_content {
      let diff = render_diff(
        &original_content,
        &formatted,
        &original.display().to_string(),
        &format!("{} (formatted)", original.display()),
      );
      if !combined_diff.is_empty() {
        combined_diff.push('\n');
      }
      combined_diff.push_str(&diff);
    }
  }

  if !combined_diff.is_empty() {
    SurfaceResult {
      surface_name,
      status: SurfaceStatus::ViolationsFound {
        message: String::new(),
        diff: Some(combined_diff),
      },
      duration: start.elapsed(),
    }
  } else {
    SurfaceResult {
      surface_name,
      status: SurfaceStatus::Passed,
      duration: start.elapsed(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn create_dummy_success_output() -> std::process::Output {
    #[cfg(windows)]
    {
      std::process::Command::new("cmd")
        .args(["/C", "exit 0"])
        .output()
        .expect("cmd exit 0 failed")
    }
    #[cfg(not(windows))]
    {
      std::process::Command::new("true")
        .output()
        .expect("true failed")
    }
  }

  #[test]
  fn test_diff_check_via_tempcopy_clean() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("clean.rs");
    std::fs::write(&file, "fn main() {\n  println!(\"clean\");\n}\n").unwrap();

    let start = Instant::now();
    let res = diff_check_via_tempcopy(
      std::slice::from_ref(&file),
      |_scratch| Ok(create_dummy_success_output()),
      "rust",
      start,
    );

    assert!(matches!(res.status, SurfaceStatus::Passed));

    let ext = file.extension().unwrap().to_str().unwrap();
    let scratch = file.with_extension(format!("{}.fml-check.tmp", ext));
    assert!(!scratch.exists());
  }

  #[test]
  fn test_diff_check_via_tempcopy_with_diff() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("dirty.rs");
    std::fs::write(&file, "fn main() {let x=1;}").unwrap();

    let start = Instant::now();
    let res = diff_check_via_tempcopy(
      std::slice::from_ref(&file),
      |scratch| {
        std::fs::write(scratch, "fn main() {\n  let x = 1;\n}\n")?;
        Ok(create_dummy_success_output())
      },
      "rust",
      start,
    );

    match res.status {
      SurfaceStatus::ViolationsFound { message, diff } => {
        assert!(message.is_empty());
        let diff_str = diff.expect("diff should be present");
        assert!(diff_str.contains("dirty.rs"));
        assert!(diff_str.contains("(formatted)"));
      }
      other => panic!("Expected ViolationsFound, got {:?}", other),
    }

    let ext = file.extension().unwrap().to_str().unwrap();
    let scratch = file.with_extension(format!("{}.fml-check.tmp", ext));
    assert!(!scratch.exists());
  }

  #[test]
  fn test_diff_check_via_tempcopy_raii_cleanup_on_error() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("error_case.rs");
    std::fs::write(&file, "invalid syntax").unwrap();

    let start = Instant::now();
    let res = diff_check_via_tempcopy(
      std::slice::from_ref(&file),
      |_scratch| Err(std::io::Error::other("mock execution error")),
      "rust",
      start,
    );

    assert!(matches!(res.status, SurfaceStatus::ExecutionError { .. }));

    let ext = file.extension().unwrap().to_str().unwrap();
    let scratch = file.with_extension(format!("{}.fml-check.tmp", ext));
    assert!(!scratch.exists());
  }

  #[test]
  fn test_diff_check_via_tempcopy_raii_cleanup_on_panic() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("panic_case.rs");
    std::fs::write(&file, "panic content").unwrap();

    let start = Instant::now();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      diff_check_via_tempcopy(
        std::slice::from_ref(&file),
        |_scratch| {
          panic!("simulated panic inside run_in_place");
        },
        "rust",
        start,
      );
    }));

    let ext = file.extension().unwrap().to_str().unwrap();
    let scratch = file.with_extension(format!("{}.fml-check.tmp", ext));
    assert!(!scratch.exists());
  }

  #[test]
  fn test_tool_info_auto_install_cmd_coverage() {
    let tools = [
      "taplo",
      "typstyle",
      "tinymist",
      "ruff",
      "prettier",
      "markdownlint-cli2",
      "yamllint",
      "clang-format",
      "clang-tidy",
      "rustfmt",
      "clippy-driver",
    ];

    for binary in tools {
      let info = ToolInfo {
        binary,
        description: "test tool",
        install_hint: "test hint",
        is_required_for_fmt: true,
        is_required_for_lint: true,
      };

      // Ensure get_auto_install_cmd executes without error
      let cmd = info.get_auto_install_cmd();
      if let Some((program, args)) = cmd {
        assert!(!program.is_empty());
        assert!(!args.is_empty());
      }
    }
  }

  #[test]
  fn test_unknown_tool_has_no_install_chain() {
    let info = ToolInfo {
      binary: "not-a-real-tool",
      description: "test tool",
      install_hint: "test hint",
      is_required_for_fmt: false,
      is_required_for_lint: false,
    };
    assert!(info.get_auto_install_cmd().is_none());
  }

  // Command-shape tests below are pure and environment-independent: they
  // exercise InstallMethod::command() directly rather than going through
  // is_available(), so they don't depend on what's actually installed on
  // the machine running the tests.

  #[test]
  fn test_install_method_command_shapes() {
    assert_eq!(
      InstallMethod::CargoBinstall("ruff").command(),
      (
        "cargo".to_string(),
        vec!["binstall".to_string(), "-y".to_string(), "ruff".to_string()]
      )
    );
    assert_eq!(
      InstallMethod::Npm("@taplo/cli").command(),
      (
        "npm".to_string(),
        vec![
          "install".to_string(),
          "-g".to_string(),
          "@taplo/cli".to_string()
        ]
      )
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "typstyle",
        locked: true
      }
      .command(),
      (
        "cargo".to_string(),
        vec![
          "install".to_string(),
          "typstyle".to_string(),
          "--locked".to_string()
        ]
      )
    );
    assert_eq!(
      InstallMethod::Cargo {
        package: "some-tool",
        locked: false
      }
      .command(),
      (
        "cargo".to_string(),
        vec!["install".to_string(), "some-tool".to_string()]
      )
    );
    assert_eq!(
      InstallMethod::WingetId("tamasfe.taplo").command(),
      (
        "winget".to_string(),
        vec![
          "install".to_string(),
          "--id=tamasfe.taplo".to_string(),
          "-e".to_string(),
          "--accept-source-agreements".to_string(),
          "--accept-package-agreements".to_string(),
        ]
      )
    );
    assert_eq!(
      InstallMethod::WingetName("LLVM.LLVM").command(),
      (
        "winget".to_string(),
        vec![
          "install".to_string(),
          "LLVM.LLVM".to_string(),
          "--accept-source-agreements".to_string(),
          "--accept-package-agreements".to_string(),
        ]
      )
    );
    assert_eq!(
      InstallMethod::Rustup("clippy").command(),
      (
        "rustup".to_string(),
        vec![
          "component".to_string(),
          "add".to_string(),
          "clippy".to_string()
        ]
      )
    );
  }

  #[test]
  fn test_find_files_with_ext_files_override() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");
    let file_c = root.join("c.rs");
    std::fs::write(&file_a, "fn a() {}").unwrap();
    std::fs::write(&file_b, "fn b() {}").unwrap();
    std::fs::write(&file_c, "fn c() {}").unwrap();

    let files_override = vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")];
    let matched = find_files_with_ext(root, &["rs"], &[], &files_override, &[]);
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&file_a));
    assert!(matched.contains(&file_c));
    assert!(!matched.contains(&file_b));
  }

  #[test]
  fn test_find_files_with_ext_exclude_patterns() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let src_dir = root.join("src");
    let gen_dir = src_dir.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();

    let normal = src_dir.join("main.rs");
    let generated = gen_dir.join("api.rs");
    let ignored = src_dir.join("ignored.rs");
    std::fs::write(&normal, "fn main() {}").unwrap();
    std::fs::write(&generated, "fn api() {}").unwrap();
    std::fs::write(&ignored, "fn ignored() {}").unwrap();

    let exclude =
      vec![PathBuf::from("src/generated"), PathBuf::from("ignored.rs")];
    let matched = find_files_with_ext(root, &["rs"], &[], &[], &exclude);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0], normal);
  }

  #[test]
  fn test_find_files_with_ext_specific_paths_precedence() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    let file_a = root.join("a.rs");
    let file_b = root.join("b.rs");
    std::fs::write(&file_a, "fn a() {}").unwrap();
    std::fs::write(&file_b, "fn b() {}").unwrap();

    let specific = vec![PathBuf::from("a.rs")];
    let files_override = vec![PathBuf::from("b.rs")];
    let matched =
      find_files_with_ext(root, &["rs"], &specific, &files_override, &[]);
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0], file_a);
  }

  #[test]
  fn test_simple_glob_match() {
    assert!(simple_glob_match("*.rs", "main.rs"));
    assert!(!simple_glob_match("*.rs", "src/main.rs"));
    assert!(!simple_glob_match("*.rs", "src\\main.rs"));
    assert!(simple_glob_match("src/*.rs", "src/main.rs"));
    assert!(simple_glob_match("src/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("src/*.rs", "src\\lib.rs"));
    assert!(simple_glob_match("src\\*.rs", "src/lib.rs"));
    assert!(!simple_glob_match("src/*.rs", "src/sub/lib.rs"));
    assert!(!simple_glob_match("src/*.rs", "src\\sub\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/sub/lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src\\sub\\lib.rs"));
    assert!(simple_glob_match("src/**/*.rs", "src/gen/api.rs"));
    assert!(simple_glob_match("src/**/api.rs", "src/gen/api.rs"));
    assert!(simple_glob_match("*.toml", "Cargo.toml"));
    assert!(!simple_glob_match("*.toml", "src/Cargo.toml"));
    assert!(simple_glob_match("target/*", "target/debug"));
    assert!(simple_glob_match("target/*", "target\\debug"));
    assert!(!simple_glob_match("target/*", "target/debug/app"));
    assert!(!simple_glob_match("target/*", "target\\debug\\app"));
    assert!(simple_glob_match("target/**", "target/debug/app"));
    assert!(simple_glob_match("target/**", "target\\debug\\app"));
    assert!(simple_glob_match("**/*.rs", "main.rs"));
    assert!(simple_glob_match("**/*.rs", "src/lib.rs"));
    assert!(simple_glob_match("**/*.rs", "src/sub/lib.rs"));
    assert!(simple_glob_match("test?.rs", "test1.rs"));
    assert!(!simple_glob_match("*.py", "main.rs"));
    assert!(!simple_glob_match("test?.rs", "test12.rs"));
    assert!(!simple_glob_match("test?.rs", "test/a.rs"));
  }

  #[test]
  fn test_extra_args_wired_to_command() {
    let mut cmd = create_tool_command("cargo");
    let extra_args = vec!["--verbose".to_string(), "--locked".to_string()];
    cmd.args(&extra_args);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().to_string())
      .collect();
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args.contains(&"--locked".to_string()));
  }

  #[test]
  fn test_all_fleet_surfaces_present() {
    let surfaces = all_surfaces();
    assert_eq!(surfaces.len(), 8);

    let names: Vec<&str> = surfaces.iter().map(|s| s.name()).collect();
    let expected = [
      "rust", "python", "cpp", "markdown", "yaml", "json", "toml", "typst",
    ];
    for exp in expected {
      assert!(
        names.contains(&exp),
        "Surface '{}' missing from all_surfaces()",
        exp
      );
    }
  }

  #[test]
  fn test_get_surface_by_name_canonical_and_aliases() {
    let test_cases = [
      ("rust", "rust"),
      ("rs", "rust"),
      ("python", "python"),
      ("py", "python"),
      ("cpp", "cpp"),
      ("c", "cpp"),
      ("c++", "cpp"),
      ("cxx", "cpp"),
      ("markdown", "markdown"),
      ("md", "markdown"),
      ("yaml", "yaml"),
      ("yml", "yaml"),
      ("json", "json"),
      ("toml", "toml"),
      ("typst", "typst"),
      ("typ", "typst"),
    ];

    for (query, canonical) in test_cases {
      let surface = get_surface_by_name(query);
      assert!(
        surface.is_some(),
        "Failed to resolve surface for query '{}'",
        query
      );
      assert_eq!(
        surface.unwrap().name(),
        canonical,
        "Query '{}' resolved to unexpected surface name",
        query
      );

      // Verify resolve_canonical_name
      assert_eq!(
        resolve_canonical_name(query),
        Some(canonical),
        "resolve_canonical_name failed for '{}'",
        query
      );
    }
  }

  #[test]
  fn test_get_surface_by_name_case_insensitive() {
    let variations = [
      ("RUST", "rust"),
      ("Rust", "rust"),
      ("rS", "rust"),
      ("RS", "rust"),
      ("PYTHON", "python"),
      ("Python", "python"),
      ("Py", "python"),
      ("PY", "python"),
      ("CPP", "cpp"),
      ("Cpp", "cpp"),
      ("C++", "cpp"),
      ("CXX", "cpp"),
      ("Cxx", "cpp"),
      ("C", "cpp"),
      ("MARKDOWN", "markdown"),
      ("Markdown", "markdown"),
      ("MD", "markdown"),
      ("Md", "markdown"),
      ("YAML", "yaml"),
      ("Yaml", "yaml"),
      ("YML", "yaml"),
      ("Yml", "yaml"),
      ("JSON", "json"),
      ("Json", "json"),
      ("TOML", "toml"),
      ("Toml", "toml"),
      ("TYPST", "typst"),
      ("Typst", "typst"),
      ("TYP", "typst"),
      ("Typ", "typst"),
      ("  rust  ", "rust"),
      ("  C++  ", "cpp"),
    ];

    for (query, canonical) in variations {
      let surface = get_surface_by_name(query);
      assert!(
        surface.is_some(),
        "Case-insensitive lookup failed for '{}'",
        query
      );
      assert_eq!(surface.unwrap().name(), canonical);
    }
  }

  #[test]
  fn test_get_surface_by_name_nonexistent() {
    assert!(get_surface_by_name("nonexistent").is_none());
    assert!(get_surface_by_name("unknown_lang").is_none());
    assert!(get_surface_by_name("").is_none());
    assert!(resolve_canonical_name("unknown").is_none());
  }

  #[test]
  fn test_custom_surface_registry() {
    let mut reg = SurfaceRegistry::empty();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert_eq!(reg.all_surfaces().len(), 0);

    reg.register_surface::<rust::RustSurface>();
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
    assert!(reg.get_surface_by_name("rs").is_some());
    assert!(reg.get_surface_by_name("python").is_none());

    reg.register(Box::new(python::PythonSurface));
    assert_eq!(reg.len(), 2);
    assert!(reg.get_surface_by_name("py").is_some());

    assert_eq!(reg.supported_languages(), vec!["rust", "python"]);
  }

  #[test]
  fn test_surface_supports_lint_fix() {
    assert!(rust::RustSurface.supports_lint_fix());
    assert!(python::PythonSurface.supports_lint_fix());
    assert!(cpp::CppSurface.supports_lint_fix());
    assert!(!yaml::YamlSurface.supports_lint_fix());
    assert!(!toml::TomlSurface.supports_lint_fix());
    assert!(!markdown::MarkdownSurface.supports_lint_fix());
    assert!(!json::JsonSurface.supports_lint_fix());
    assert!(!typst::TypstSurface.supports_lint_fix());
  }

  #[test]
  fn test_unsupported_lint_fix_returns_skipped() {
    let dummy_ctx = ExecutionContext {
      root: PathBuf::from("."),
      paths: Vec::new(),
      global_config: ResolvedGlobalConfig::default(),
      lang_config: ResolvedLangConfig::new("dummy"),
      check_only: false,
    };

    let unsupported_surfaces: Vec<Box<dyn LanguageSurface>> = vec![
      Box::new(yaml::YamlSurface),
      Box::new(toml::TomlSurface),
      Box::new(markdown::MarkdownSurface),
      Box::new(json::JsonSurface),
      Box::new(typst::TypstSurface),
    ];

    for surface in unsupported_surfaces {
      let res = surface.lint(&dummy_ctx, true);
      match res.status {
        SurfaceStatus::Skipped { reason } => {
          assert_eq!(
            reason,
            "Tool does not support autofix; run fml fmt instead",
            "Mismatch for surface {}",
            surface.name()
          );
        }
        other => panic!(
          "Surface {} did not return Skipped on lint with fix=true: {:?}",
          surface.name(),
          other
        ),
      }
    }
  }

  #[test]
  fn test_surface_file_extensions() {
    for surface in all_surfaces() {
      let exts = surface.file_extensions();
      assert!(
        !exts.is_empty(),
        "Surface '{}' has empty file extensions",
        surface.name()
      );
    }
  }
}
