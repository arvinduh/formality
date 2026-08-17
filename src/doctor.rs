use crate::config::FormalityConfig;
use crate::surfaces::{
  LanguageSurface, ToolInfo, all_surfaces, create_tool_command,
  detect_surfaces_smart,
};
use crate::table::{
  Cell, Column, Layout, Palette, Row, Span, Style, Table, WidthPolicy, render,
};
use crate::version::{
  ToolStatus, Version, check_tool_compatibility, get_raw_tool_version,
  minimum_supported_tool_version, probe_tool_version,
};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Install a deduplicated list of missing tools.
///
/// Prints progress to stdout and returns `true` if every tool either
/// installed successfully or already had a known auto-install command,
/// `false` if any tool could not be installed.
pub fn install_missing_tools(missing: &[ToolInfo]) -> bool {
  if missing.is_empty() {
    return true;
  }

  let separator = crate::table::separator_line(0);
  println!("\n{}", separator.dimmed());
  println!("{}", "Auto-installing Missing Toolchains:".bold().cyan());

  let mut all_ok = true;

  for tool in missing {
    if let Some((program, args)) = tool.get_auto_install_cmd() {
      println!(
        "\n  {} Installing {} via: {} {}",
        "[INSTALL]".cyan().bold(),
        tool.binary.bold(),
        program.cyan(),
        args.join(" ").cyan()
      );

      let mut cmd = create_tool_command(&program);
      cmd.args(&args);

      match cmd.status() {
        Ok(status) if status.success() => {
          println!(
            "    {} Successfully installed {}",
            "[OK]  ".green().bold(),
            tool.binary.bold()
          );
        }
        Ok(status) => {
          println!(
            "    {} Failed to install {} (exit code: {})",
            "[FAIL]".red().bold(),
            tool.binary.bold(),
            status.code().unwrap_or(1)
          );
          all_ok = false;
        }
        Err(e) => {
          println!(
            "    {} Error running {}: {}",
            "[ERR] ".red().bold(),
            program,
            e
          );
          all_ok = false;
        }
      }
    } else {
      println!(
        "\n  {} No automatic package manager found for {}.\n    Manual install: {}",
        "[MISS]".yellow().bold(),
        tool.binary.bold(),
        tool.install_hint
      );
      all_ok = false;
    }
  }

  all_ok
}

/// Collect the missing tools required by `surfaces` for the given action
/// (format or lint), then install them. Returns `false` if any tool
/// could not be installed.
pub fn preflight_install(
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
  for_fmt: bool,
) -> bool {
  use which::which;
  let mut seen: HashSet<&'static str> = HashSet::new();
  let mut missing: Vec<ToolInfo> = Vec::new();

  for surface in surfaces {
    let resolved = config.resolve_for_lang(surface.name());
    for tool in surface.tool_info(&resolved) {
      if seen.contains(tool.binary) {
        continue;
      }
      let needed = if for_fmt {
        tool.is_required_for_fmt
      } else {
        tool.is_required_for_lint
      };
      if needed && which(tool.binary).is_err() {
        seen.insert(tool.binary);
        missing.push(tool);
      }
    }
  }

  install_missing_tools(&missing)
}

pub struct ToolLookupResult {
  pub is_installed: bool,
  pub path: Option<String>,
  pub raw_version: Option<String>,
  pub parsed_version: Option<Version>,
  pub status: Option<ToolStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualEnvSource {
  EnvVar,
  Workspace(String),
  None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualEnvInfo {
  pub is_active: bool,
  pub venv_path: Option<PathBuf>,
  pub interpreter_path: Option<PathBuf>,
  pub source: VirtualEnvSource,
}

/// Look for Python interpreter binary inside a virtual environment directory.
pub fn find_venv_interpreter(venv_path: &Path) -> Option<PathBuf> {
  let candidates = [
    venv_path.join("Scripts").join("python.exe"),
    venv_path.join("Scripts").join("python"),
    venv_path.join("bin").join("python"),
    venv_path.join("bin").join("python3"),
    venv_path.join("bin").join("python.exe"),
    venv_path.join("python.exe"),
    venv_path.join("python"),
  ];
  for candidate in &candidates {
    if candidate.is_file() {
      return Some(candidate.clone());
    }
  }
  None
}

/// Detects active virtual environment (via `VIRTUAL_ENV`) or workspace virtualenv directory (`.venv`, `venv`, `env`, `.env`).
pub fn detect_virtualenv(root: &Path) -> VirtualEnvInfo {
  detect_virtualenv_with_env(
    root,
    std::env::var_os("VIRTUAL_ENV").map(PathBuf::from),
  )
}

pub fn detect_virtualenv_with_env(
  root: &Path,
  env_var: Option<PathBuf>,
) -> VirtualEnvInfo {
  if let Some(venv_dir) = env_var.filter(|p| !p.as_os_str().is_empty()) {
    let interpreter = find_venv_interpreter(&venv_dir).or_else(|| {
      which::which("python3")
        .or_else(|_| which::which("python"))
        .ok()
    });
    return VirtualEnvInfo {
      is_active: true,
      venv_path: Some(venv_dir),
      interpreter_path: interpreter,
      source: VirtualEnvSource::EnvVar,
    };
  }

  let candidates = [".venv", "venv", "env", ".env"];
  for dir_name in candidates {
    let dir = root.join(dir_name);
    if dir.is_dir() {
      let interpreter = find_venv_interpreter(&dir).or_else(|| {
        which::which("python3")
          .or_else(|_| which::which("python"))
          .ok()
      });
      return VirtualEnvInfo {
        is_active: false,
        venv_path: Some(dir),
        interpreter_path: interpreter,
        source: VirtualEnvSource::Workspace(dir_name.to_string()),
      };
    }
  }

  let sys_interpreter = which::which("python3")
    .or_else(|_| which::which("python"))
    .ok();
  VirtualEnvInfo {
    is_active: false,
    venv_path: None,
    interpreter_path: sys_interpreter,
    source: VirtualEnvSource::None,
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignoreHygieneIssue {
  pub category: &'static str,
  pub missing_patterns: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignoreHygieneReport {
  pub gitignore_exists: bool,
  pub issues: Vec<GitignoreHygieneIssue>,
}

/// Checks whether a specific pattern is ignored given `.gitignore` lines.
pub fn is_pattern_ignored(lines: &[&str], entry: &str) -> bool {
  let normalized_entry = entry.trim_matches('/');
  for raw_line in lines {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
      continue;
    }
    let trimmed = line
      .trim_start_matches("**/")
      .trim_start_matches('/')
      .trim_end_matches("/**")
      .trim_end_matches('/');
    if trimmed == normalized_entry {
      return true;
    }
    if normalized_entry == "__pycache__"
      && (line == "*.py[cod]" || line == "*.pyc" || line == "*$py.class")
    {
      return true;
    }
  }
  false
}

/// Validates that cache/artifact directories for active language toolchains are ignored in `.gitignore`.
pub fn check_gitignore_hygiene_content(
  gitignore_content: Option<&str>,
  has_python: bool,
  has_rust: bool,
  has_js: bool,
) -> GitignoreHygieneReport {
  let gitignore_exists = gitignore_content.is_some();
  let lines: Vec<&str> = gitignore_content.unwrap_or("").lines().collect();
  let mut issues = Vec::new();

  if has_python {
    let python_patterns: &[&'static str] =
      &[".ruff_cache", "__pycache__", ".pytest_cache"];
    let mut missing = Vec::new();
    for &pattern in python_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "Python",
        missing_patterns: missing,
      });
    }
  }

  if has_rust {
    let rust_patterns: &[&'static str] = &["target"];
    let mut missing = Vec::new();
    for &pattern in rust_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "Rust",
        missing_patterns: missing,
      });
    }
  }

  if has_js {
    let js_patterns: &[&'static str] = &["node_modules"];
    let mut missing = Vec::new();
    for &pattern in js_patterns {
      if !is_pattern_ignored(&lines, pattern) {
        missing.push(pattern);
      }
    }
    if !missing.is_empty() {
      issues.push(GitignoreHygieneIssue {
        category: "JavaScript / Node",
        missing_patterns: missing,
      });
    }
  }

  GitignoreHygieneReport {
    gitignore_exists,
    issues,
  }
}

pub fn check_gitignore_hygiene(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
) -> GitignoreHygieneReport {
  let gitignore_path = root.join(".gitignore");
  let gitignore_content = std::fs::read_to_string(&gitignore_path).ok();

  let has_python = surfaces
    .iter()
    .any(|s| s.name() == "python" || s.aliases().contains(&"py"))
    || root.join("pyproject.toml").is_file()
    || root.join("requirements.txt").is_file()
    || root.join("setup.py").is_file()
    || root.join("Pipfile").is_file()
    || root.join("ruff.toml").is_file()
    || root.join(".ruff.toml").is_file();

  let has_rust = surfaces
    .iter()
    .any(|s| s.name() == "rust" || s.aliases().contains(&"rs"))
    || root.join("Cargo.toml").is_file();

  let has_js = root.join("package.json").is_file()
    || root.join("node_modules").is_dir()
    || root.join("package-lock.json").is_file()
    || root.join("yarn.lock").is_file()
    || root.join("pnpm-lock.yaml").is_file()
    || root.join("bun.lockb").is_file()
    || root.join("bun.lock").is_file()
    || surfaces.iter().any(|s| {
      let n = s.name();
      n == "markdown" || n == "yaml" || n == "json"
    });

  check_gitignore_hygiene_content(
    gitignore_content.as_deref(),
    has_python,
    has_rust,
    has_js,
  )
}

pub fn run_doctor(
  root: &Path,
  show_all: bool,
  install: bool,
  config: &FormalityConfig,
) -> i32 {
  let surfaces: Vec<Box<dyn LanguageSurface>> = if show_all {
    all_surfaces()
  } else {
    let detected = detect_surfaces_smart(root, config);
    if detected.is_empty() {
      all_surfaces()
    } else {
      detected
    }
  };

  let mut cache: HashMap<&'static str, ToolLookupResult> = HashMap::new();
  let mut missing_unique_tools: Vec<ToolInfo> = Vec::new();
  let mut installed_unique_tools = HashSet::new();
  let mut outdated_unique_tools = HashSet::new();

  let mut doctor_table = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(8)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(20)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1));

  for surface in &surfaces {
    let resolved = config.resolve_for_lang(surface.name());
    let tools = surface.tool_info(&resolved);

    for tool in tools {
      let lookup = cache.entry(tool.binary).or_insert_with(|| {
        let is_installed = which::which(tool.binary).is_ok()
          || (tool.binary == "clippy"
            && (which::which("clippy-driver").is_ok()
              || which::which("cargo").is_ok()));

        if is_installed {
          let path = which::which(tool.binary)
            .or_else(|_| which::which("clippy-driver"))
            .or_else(|_| which::which("cargo"))
            .ok()
            .map(|p| p.display().to_string());
          let raw_version = get_raw_tool_version(tool.binary);
          let parsed_version = probe_tool_version(tool.binary);
          let status = minimum_supported_tool_version(tool.binary)
            .map(|mstv| check_tool_compatibility(tool.binary, &mstv));

          ToolLookupResult {
            is_installed: true,
            path,
            raw_version,
            parsed_version,
            status,
          }
        } else {
          ToolLookupResult {
            is_installed: false,
            path: None,
            raw_version: None,
            parsed_version: None,
            status: Some(ToolStatus::NotFound),
          }
        }
      });

      if lookup.is_installed {
        if installed_unique_tools.insert(tool.binary) {
          let path_str = lookup.path.as_deref().unwrap_or("");
          match &lookup.status {
            Some(ToolStatus::Outdated { current, minimum }) => {
              outdated_unique_tools.insert(tool.binary);
              let v_info = format!(" (v{} < MSTV v{})", current, minimum);
              let row = Row::new(vec![
                Cell::styled("[WARN] ", Style::Warn),
                Cell::styled(tool.binary, Style::Warn),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Warn),
                ]),
              ]);
              doctor_table.add_row(row);
            }
            Some(ToolStatus::Compatible { current, .. }) => {
              let v_info = format!(" (v{})", current);
              let row = Row::new(vec![
                Cell::styled("[READY]", Style::Ok),
                Cell::styled(tool.binary, Style::Tool),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Info),
                ]),
              ]);
              doctor_table.add_row(row);
            }
            _ => {
              let v_info = if let Some(ref v) = lookup.parsed_version {
                format!(" (v{})", v)
              } else if let Some(ref v) = lookup.raw_version {
                format!(" ({})", v.trim())
              } else {
                String::new()
              };
              let row = Row::new(vec![
                Cell::styled("[READY]", Style::Ok),
                Cell::styled(tool.binary, Style::Tool),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Info),
                ]),
              ]);
              doctor_table.add_row(row);
            }
          }
        }
      } else if !missing_unique_tools.iter().any(|t| t.binary == tool.binary) {
        missing_unique_tools.push(tool.clone());

        let row = Row::new(vec![
          Cell::styled("[MISS] ", Style::Warn),
          Cell::styled(tool.binary, Style::Warn),
          Cell::styled(surface.name(), Style::Dim),
          Cell::styled(tool.description, Style::Dim),
        ]);
        doctor_table.add_row(row);
      }
    }
  }

  let palette = Palette::detect();
  let rendered_table = render(&doctor_table, &palette);
  let separator = crate::table::separator_for_content(&rendered_table);

  println!(
    "{} {}",
    "fml doctor".bold().cyan(),
    if show_all {
      "(all surfaces)".dimmed()
    } else {
      "(active surfaces)".dimmed()
    }
  );
  println!("{}", separator.dimmed());
  if !rendered_table.is_empty() {
    println!("{}", rendered_table);
  }

  // Check for unconfigured surfaces if explicit `languages` is set
  if let Some(ref explicit_langs) = config.resolve_global().languages {
    let mut unconfigured = Vec::new();
    for surface in all_surfaces() {
      if !explicit_langs.iter().any(|l| {
        l.eq_ignore_ascii_case(surface.name())
          || surface.aliases().iter().any(|a| a.eq_ignore_ascii_case(l))
      }) && surface.detect(root)
      {
        unconfigured.push(surface.name());
      }
    }

    if !unconfigured.is_empty() {
      println!("\n{}", separator.dimmed());
      println!("{}", "Unconfigured Workspace Languages:".yellow().bold());
      for name in unconfigured {
        println!(
          "  • Files for '{}' exist in workspace, but '{}' is not in global.languages",
          name.bold(),
          name
        );
      }
      println!(
        "    {} Add them to {} if you want formality to manage them.",
        "Tip:".cyan().bold(),
        "languages = [...]".bold()
      );
    }
  }

  // Virtual Environment status
  let has_python = surfaces
    .iter()
    .any(|s| s.name() == "python" || s.aliases().contains(&"py"))
    || root.join("pyproject.toml").is_file()
    || root.join("requirements.txt").is_file()
    || root.join("setup.py").is_file()
    || root.join("Pipfile").is_file();
  let venv_info = detect_virtualenv(root);
  if has_python || venv_info.venv_path.is_some() || show_all {
    println!("\n{}", separator.dimmed());
    println!("{}", "Python Virtual Environment:".bold().cyan());
    match &venv_info.source {
      VirtualEnvSource::EnvVar => {
        let p = venv_info
          .venv_path
          .as_ref()
          .map(|p| p.display().to_string())
          .unwrap_or_default();
        println!(
          "  • {} Active virtualenv via VIRTUAL_ENV: {}",
          "[ACTIVE]".green().bold(),
          p.cyan()
        );
      }
      VirtualEnvSource::Workspace(dir_name) => {
        let p = venv_info
          .venv_path
          .as_ref()
          .map(|p| p.display().to_string())
          .unwrap_or_default();
        println!(
          "  • {} Detected workspace virtualenv ({}): {}",
          "[FOUND] ".cyan().bold(),
          dir_name.bold(),
          p.dimmed()
        );
      }
      VirtualEnvSource::None => {
        println!(
          "  • {} No virtual environment detected",
          "[NONE]  ".dimmed()
        );
      }
    }

    if let Some(ref interp) = venv_info.interpreter_path {
      println!(
        "  • Python interpreter: {}",
        interp.display().to_string().cyan()
      );
    } else {
      println!(
        "  • {} No Python interpreter found on PATH or in virtualenv",
        "[WARN] ".yellow().bold()
      );
    }
  }

  // .gitignore Cache Hygiene Check
  let hygiene_report = check_gitignore_hygiene(root, &surfaces);
  if !hygiene_report.issues.is_empty() {
    println!("\n{}", separator.dimmed());
    println!("{}", "Gitignore Cache Hygiene:".yellow().bold());
    if !hygiene_report.gitignore_exists {
      println!(
        "  • {} No {} file found in workspace root",
        "[WARN] ".yellow().bold(),
        ".gitignore".bold()
      );
    }
    for issue in &hygiene_report.issues {
      let missing_list = issue.missing_patterns.join(", ");
      println!(
        "  • {} {} cache/artifact entries missing from {}: {}",
        "[WARN] ".yellow().bold(),
        issue.category.bold(),
        ".gitignore".bold(),
        missing_list.yellow().bold()
      );
    }
    println!(
      "    {} Add missing patterns to {} to prevent committing artifacts.",
      "Tip:".cyan().bold(),
      ".gitignore".bold()
    );
  }

  // Auto-install mode
  if install && !missing_unique_tools.is_empty() {
    install_missing_tools(&missing_unique_tools);
  }

  println!("{}", separator.dimmed());
  let outdated_str = if !outdated_unique_tools.is_empty() {
    format!(" ({} outdated)", outdated_unique_tools.len())
      .yellow()
      .to_string()
  } else {
    String::new()
  };
  println!(
    "  {} installed{}, {} missing{}\n",
    installed_unique_tools.len().to_string().green().bold(),
    outdated_str,
    if missing_unique_tools.is_empty() {
      "0".green().bold().to_string()
    } else {
      missing_unique_tools
        .len()
        .to_string()
        .yellow()
        .bold()
        .to_string()
    },
    if !missing_unique_tools.is_empty() && !install {
      " (run 'fml install' to install missing tools)"
        .dimmed()
        .to_string()
    } else {
      String::new()
    }
  );

  if missing_unique_tools.is_empty() || install {
    0
  } else {
    2
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[test]
  fn test_detect_virtualenv_from_env_var() {
    let temp = tempdir().unwrap();
    let mock_venv = temp.path().join("custom_venv");
    std::fs::create_dir_all(&mock_venv).unwrap();

    let info = detect_virtualenv_with_env(temp.path(), Some(mock_venv.clone()));
    assert!(info.is_active);
    assert_eq!(info.venv_path, Some(mock_venv));
    assert_eq!(info.source, VirtualEnvSource::EnvVar);
  }

  #[test]
  fn test_detect_virtualenv_from_workspace_dirs() {
    for dir_name in &[".venv", "venv", "env", ".env"] {
      let temp = tempdir().unwrap();
      let venv_dir = temp.path().join(dir_name);
      std::fs::create_dir_all(&venv_dir).unwrap();

      let info = detect_virtualenv_with_env(temp.path(), None);
      assert!(!info.is_active);
      assert_eq!(info.venv_path, Some(venv_dir));
      assert_eq!(
        info.source,
        VirtualEnvSource::Workspace(dir_name.to_string())
      );
    }
  }

  #[test]
  fn test_detect_virtualenv_precedence() {
    let temp = tempdir().unwrap();
    let dot_venv = temp.path().join(".venv");
    let venv = temp.path().join("venv");
    std::fs::create_dir_all(&dot_venv).unwrap();
    std::fs::create_dir_all(&venv).unwrap();

    let info = detect_virtualenv_with_env(temp.path(), None);
    assert_eq!(info.venv_path, Some(dot_venv));
    assert_eq!(
      info.source,
      VirtualEnvSource::Workspace(".venv".to_string())
    );
  }

  #[test]
  fn test_detect_virtualenv_none() {
    let temp = tempdir().unwrap();
    let info = detect_virtualenv_with_env(temp.path(), None);
    assert!(!info.is_active);
    assert_eq!(info.venv_path, None);
    assert_eq!(info.source, VirtualEnvSource::None);
  }

  #[test]
  fn test_find_venv_interpreter() {
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let python_bin = bin_dir.join("python");
    std::fs::write(&python_bin, "#!/bin/sh\n").unwrap();

    let found = find_venv_interpreter(temp.path());
    assert_eq!(found, Some(python_bin));
  }

  #[test]
  fn test_is_pattern_ignored() {
    let lines = vec![
      "# Comments should be ignored",
      "",
      "target/",
      "/.ruff_cache/",
      "__pycache__",
      "**/node_modules/**",
      "!not_ignored",
    ];

    assert!(is_pattern_ignored(&lines, "target"));
    assert!(is_pattern_ignored(&lines, ".ruff_cache"));
    assert!(is_pattern_ignored(&lines, "__pycache__"));
    assert!(is_pattern_ignored(&lines, "node_modules"));
    assert!(!is_pattern_ignored(&lines, ".pytest_cache"));
    assert!(!is_pattern_ignored(&lines, "not_ignored"));
  }

  #[test]
  fn test_is_pattern_ignored_pyc_alias() {
    let lines = vec!["*.pyc"];
    assert!(is_pattern_ignored(&lines, "__pycache__"));
  }

  #[test]
  fn test_check_gitignore_hygiene_all_satisfied() {
    let gitignore = r#"
/target/
.ruff_cache/
__pycache__/
.pytest_cache/
node_modules/
"#;
    let report = check_gitignore_hygiene_content(
      Some(gitignore),
      true, // has_python
      true, // has_rust
      true, // has_js
    );
    assert!(report.gitignore_exists);
    assert!(report.issues.is_empty());
  }

  #[test]
  fn test_check_gitignore_hygiene_missing_entries() {
    let gitignore = r#"
target/
"#;
    let report = check_gitignore_hygiene_content(
      Some(gitignore),
      true, // has_python
      true, // has_rust
      true, // has_js
    );
    assert!(report.gitignore_exists);
    assert_eq!(report.issues.len(), 2);
    let py_issue = report
      .issues
      .iter()
      .find(|i| i.category == "Python")
      .unwrap();
    assert_eq!(
      py_issue.missing_patterns,
      vec![".ruff_cache", "__pycache__", ".pytest_cache"]
    );
    let js_issue = report
      .issues
      .iter()
      .find(|i| i.category == "JavaScript / Node")
      .unwrap();
    assert_eq!(js_issue.missing_patterns, vec!["node_modules"]);
  }

  #[test]
  fn test_check_gitignore_hygiene_no_file() {
    let report = check_gitignore_hygiene_content(
      None, true,  // has_python
      true,  // has_rust
      false, // has_js
    );
    assert!(!report.gitignore_exists);
    assert_eq!(report.issues.len(), 2);
    assert!(report.issues.iter().any(|i| i.category == "Python"));
    assert!(report.issues.iter().any(|i| i.category == "Rust"));
  }
}
