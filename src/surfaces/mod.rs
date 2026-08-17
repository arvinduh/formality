pub mod cpp;
pub mod json;
pub mod markdown;
pub mod python;
pub mod rust;
pub mod toml;
pub mod typst;
pub mod yaml;

use crate::config::{
  FormalityConfig, ResolvedGlobalConfig, ResolvedLangConfig,
};
use crate::diff::render_diff;
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

pub trait LanguageSurface: Send + Sync {
  fn name(&self) -> &'static str;
  fn display_name(&self) -> &'static str {
    self.name()
  }
  fn aliases(&self) -> &[&'static str] {
    &[]
  }
  fn detect(&self, root: &Path) -> bool;
  fn tool_info(&self, config: &ResolvedLangConfig) -> Vec<ToolInfo>;
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult;
  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult;
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult;
}

pub fn all_surfaces() -> Vec<Box<dyn LanguageSurface>> {
  vec![
    Box::new(rust::RustSurface),
    Box::new(python::PythonSurface),
    Box::new(cpp::CppSurface),
    Box::new(markdown::MarkdownSurface),
    Box::new(yaml::YamlSurface),
    Box::new(json::JsonSurface),
    Box::new(toml::TomlSurface),
    Box::new(typst::TypstSurface),
  ]
}

/// Smart surface discovery:
/// 1. Prioritizes explicit `languages = [...]` allowlist if defined.
/// 2. Filters out any `ignore_languages = [...]` blocklist.
/// 3. Otherwise auto-detects active project surfaces (ignoring target/, fixtures/, etc.).
pub fn detect_surfaces_smart(
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
      if let Some(s) = get_surface_by_name(lang_name)
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
  all_surfaces()
    .into_iter()
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
    .collect()
}

pub fn detect_surfaces(root: &Path) -> Vec<Box<dyn LanguageSurface>> {
  all_surfaces()
    .into_iter()
    .filter(|surface| surface.detect(root))
    .collect()
}

pub fn get_surface_by_name(name: &str) -> Option<Box<dyn LanguageSurface>> {
  let lower = name.to_lowercase();
  all_surfaces().into_iter().find(|s| {
    s.name().eq_ignore_ascii_case(&lower)
      || s.aliases().iter().any(|a| a.eq_ignore_ascii_case(&lower))
  })
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
  let p_chars: Vec<char> = pattern.chars().collect();
  let t_chars: Vec<char> = text.chars().collect();
  let (p_len, t_len) = (p_chars.len(), t_chars.len());

  let mut p_idx = 0;
  let mut t_idx = 0;
  let mut star_idx = None;
  let mut match_idx = 0;

  while t_idx < t_len {
    if p_idx < p_len
      && (p_chars[p_idx] == '?' || p_chars[p_idx] == t_chars[t_idx])
    {
      p_idx += 1;
      t_idx += 1;
    } else if p_idx < p_len && p_chars[p_idx] == '*' {
      star_idx = Some(p_idx);
      match_idx = t_idx;
      p_idx += 1;
    } else if let Some(star) = star_idx {
      p_idx = star + 1;
      match_idx += 1;
      t_idx = match_idx;
    } else {
      return false;
    }
  }

  while p_idx < p_len && p_chars[p_idx] == '*' {
    p_idx += 1;
  }

  p_idx == p_len
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
  content.contains("WARNING: DO NOT EDIT THIS FILE DIRECTLY!")
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

#[cfg(test)]
mod tests {
  use super::*;

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
    assert!(simple_glob_match("src/*.rs", "src/main.rs"));
    assert!(simple_glob_match("src/**/api.rs", "src/gen/api.rs"));
    assert!(simple_glob_match("test?.rs", "test1.rs"));
    assert!(!simple_glob_match("*.py", "main.rs"));
    assert!(!simple_glob_match("test?.rs", "test12.rs"));
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
}
