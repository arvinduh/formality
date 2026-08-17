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

impl ToolInfo {
  pub fn get_auto_install_cmd(&self) -> Option<(String, Vec<String>)> {
    match self.binary {
      "taplo" => {
        if check_binary_exists("cargo") {
          Some((
            "cargo".to_string(),
            vec![
              "install".to_string(),
              "taplo-cli".to_string(),
              "--locked".to_string(),
            ],
          ))
        } else if check_binary_exists("npm") {
          Some((
            "npm".to_string(),
            vec![
              "install".to_string(),
              "-g".to_string(),
              "@taplo/cli".to_string(),
            ],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "taplo".to_string()],
          ))
        } else {
          None
        }
      }
      "typstyle" => {
        if check_binary_exists("cargo") {
          Some((
            "cargo".to_string(),
            vec![
              "install".to_string(),
              "typstyle".to_string(),
              "--locked".to_string(),
            ],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "typstyle".to_string()],
          ))
        } else {
          None
        }
      }
      "ruff" => {
        if check_binary_exists("pip") {
          Some((
            "pip".to_string(),
            vec!["install".to_string(), "ruff".to_string()],
          ))
        } else if check_binary_exists("pip3") {
          Some((
            "pip3".to_string(),
            vec!["install".to_string(), "ruff".to_string()],
          ))
        } else if check_binary_exists("pipx") {
          Some((
            "pipx".to_string(),
            vec!["install".to_string(), "ruff".to_string()],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "ruff".to_string()],
          ))
        } else if check_binary_exists("cargo") {
          Some((
            "cargo".to_string(),
            vec!["install".to_string(), "ruff".to_string()],
          ))
        } else {
          None
        }
      }
      "prettier" => {
        if check_binary_exists("npm") {
          Some((
            "npm".to_string(),
            vec![
              "install".to_string(),
              "-g".to_string(),
              "prettier".to_string(),
            ],
          ))
        } else if check_binary_exists("pnpm") {
          Some((
            "pnpm".to_string(),
            vec!["add".to_string(), "-g".to_string(), "prettier".to_string()],
          ))
        } else if check_binary_exists("yarn") {
          Some((
            "yarn".to_string(),
            vec![
              "global".to_string(),
              "add".to_string(),
              "prettier".to_string(),
            ],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "prettier".to_string()],
          ))
        } else {
          None
        }
      }
      "markdownlint-cli2" | "markdownlint" => {
        if check_binary_exists("npm") {
          Some((
            "npm".to_string(),
            vec![
              "install".to_string(),
              "-g".to_string(),
              "markdownlint-cli2".to_string(),
            ],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "markdownlint-cli2".to_string()],
          ))
        } else {
          None
        }
      }
      "yamllint" => {
        if check_binary_exists("pip") {
          Some((
            "pip".to_string(),
            vec!["install".to_string(), "yamllint".to_string()],
          ))
        } else if check_binary_exists("pip3") {
          Some((
            "pip3".to_string(),
            vec!["install".to_string(), "yamllint".to_string()],
          ))
        } else if check_binary_exists("brew") {
          Some((
            "brew".to_string(),
            vec!["install".to_string(), "yamllint".to_string()],
          ))
        } else {
          None
        }
      }
      "rustfmt" | "clippy-driver" => {
        if check_binary_exists("rustup") {
          let comp = if self.binary == "rustfmt" {
            "rustfmt"
          } else {
            "clippy"
          };
          Some((
            "rustup".to_string(),
            vec!["component".to_string(), "add".to_string(), comp.to_string()],
          ))
        } else {
          None
        }
      }
      _ => None,
    }
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
) -> Vec<PathBuf> {
  if !specific_paths.is_empty() {
    let mut out = Vec::new();
    for p in specific_paths {
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
    return out;
  }

  walk_dir_ext(root, extensions)
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
