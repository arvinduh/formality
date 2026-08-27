//! C/C++ language surface: formats via `clang-format` and lints via
//! `clang-tidy`, syncing the managed `.clang-format` / `.clang-tidy` from
//! `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo, create_tool_command,
  diff_check_via_tempcopy, find_files_with_ext, render_native_config,
  run_tool_command, sync_native_config, tool_missing_guard,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Native `.clang-format` configuration representation for C/C++ formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct ClangFormatConfig {
  /// Target language specification.
  pub language: String,
  /// Base style sheet (e.g. `"LLVM"`, `"Google"`).
  pub based_on_style: String,
  /// Indentation spaces width per level.
  pub indent_width: usize,
  /// Maximum line column limit.
  pub column_limit: usize,
  /// Tab usage policy (`"Always"`, `"Never"`).
  pub use_tab: String,
  /// Line ending style (`"LF"`, `"CRLF"`).
  pub line_ending: String,
  /// Pointer alignment style (`"Left"`, `"Right"`, `"Middle"`).
  pub pointer_alignment: String,
  /// Brace breaking style (`"Attach"`, `"Allman"`).
  pub break_before_braces: String,
  /// Whether to sort `#include` statements.
  pub sort_includes: bool,
  /// Language standard (e.g. `"c++17"`, `"c++20"`, `"Latest"`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub standard: Option<String>,
}

impl NativeConfig for ClangFormatConfig {
  const FILE_NAME: &'static str = ".clang-format";

  fn from_context(ctx: &ExecutionContext) -> Self {
    let use_tab = if ctx.lang_config.use_tabs {
      "Always"
    } else {
      "Never"
    };
    let line_ending =
      match ctx.global_config.end_of_line.to_lowercase().as_str() {
        "crlf" => "CRLF",
        _ => "LF",
      };

    let cpp_opts = ctx.lang_config.cpp.as_ref();
    let based_on_style = cpp_opts
      .and_then(|c| c.based_on_style.clone())
      .unwrap_or_else(|| "LLVM".to_string());
    let column_limit = cpp_opts
      .and_then(|c| c.column_limit)
      .unwrap_or(ctx.lang_config.line_length);
    let pointer_alignment = cpp_opts
      .and_then(|c| c.pointer_alignment.clone())
      .unwrap_or_else(|| "Left".to_string());
    let break_before_braces = cpp_opts
      .and_then(|c| c.break_before_braces.clone())
      .unwrap_or_else(|| "Attach".to_string());
    let sort_includes = cpp_opts.and_then(|c| c.sort_includes).unwrap_or(true);
    let standard = cpp_opts.and_then(|c| {
      c.standard
        .as_ref()
        .map(|s| s.trim().trim_start_matches("-std=").to_string())
    });

    Self {
      language: "Cpp".to_string(),
      based_on_style,
      indent_width: ctx.lang_config.indent_size,
      column_limit,
      use_tab: use_tab.to_string(),
      line_ending: line_ending.to_string(),
      pointer_alignment,
      break_before_braces,
      sort_includes,
      standard,
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Native `.clang-tidy` configuration representation for C/C++ linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct ClangTidyConfig {
  /// Enabled clang-tidy check patterns.
  pub checks: String,
  /// Warnings to treat as fatal errors.
  pub warnings_as_errors: String,
  /// Header file filter regex pattern.
  pub header_filter_regex: String,
  /// Code format style.
  pub format_style: String,
}

impl Default for ClangTidyConfig {
  fn default() -> Self {
    Self {
      checks:
        "*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*"
          .to_string(),
      warnings_as_errors: String::new(),
      header_filter_regex: String::new(),
      format_style: "none".to_string(),
    }
  }
}

impl NativeConfig for ClangTidyConfig {
  const FILE_NAME: &'static str = ".clang-tidy";

  fn from_context(_ctx: &ExecutionContext) -> Self {
    Self::default()
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// C/C++ language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct CppSurface;

impl DeclaresFacets for CppSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs
      | Facet::IndentWidth
      | Facet::LineLength
      | Facet::ImportSort
      | Facet::Standard => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ProseWrap
      | Facet::Edition => FacetSupport::Unsupported,
    }
  }
}

/// Renders a [`ClangFormatConfig`] as the inline `{Key: Value, ...}` YAML-flow
/// style string accepted by clang-format's `-style=` flag, so `fml fmt` can
/// apply the resolved formality.toml settings without writing
/// `.clang-format` to disk. Only `fml sync` writes that file now (see
/// [`CppSurface::sync_config`]). Verified byte-identical against the
/// file-based path for both the LLVM/2-space defaults and a custom
/// Google/4-space/Allman configuration (Fixes #157).
#[must_use]
pub fn build_clang_format_inline_style(cfg: &ClangFormatConfig) -> String {
  let mut style = format!(
    "{{Language: {}, BasedOnStyle: {}, IndentWidth: {}, ColumnLimit: {}, UseTab: {}, LineEnding: {}, PointerAlignment: {}, BreakBeforeBraces: {}, SortIncludes: {}",
    cfg.language,
    cfg.based_on_style,
    cfg.indent_width,
    cfg.column_limit,
    cfg.use_tab,
    cfg.line_ending,
    cfg.pointer_alignment,
    cfg.break_before_braces,
    cfg.sort_includes,
  );
  if let Some(ref standard) = cfg.standard {
    use std::fmt::Write as _;
    let _ = write!(style, ", Standard: {standard}");
  }
  style.push('}');
  style
}

/// Renders a [`ClangTidyConfig`] as the inline `{Key: Value, ...}` YAML-flow
/// string accepted by clang-tidy's `--config=` flag, so `fml lint` can apply
/// the resolved checks without writing `.clang-tidy` to disk. Only `fml
/// sync` writes that file now (see [`CppSurface::sync_config`]). Verified
/// byte-identical against the file-based path (Fixes #157).
#[must_use]
pub fn build_clang_tidy_inline_config(cfg: &ClangTidyConfig) -> String {
  format!(
    "{{Checks: '{}', WarningsAsErrors: '{}', HeaderFilterRegex: '{}', FormatStyle: {}}}",
    cfg.checks,
    cfg.warnings_as_errors,
    cfg.header_filter_regex,
    cfg.format_style,
  )
}

/// Standard file extensions recognized for C/C++ source and header files.
pub const CPP_EXTENSIONS: &[&str] =
  &["c", "cpp", "cc", "cxx", "h", "hpp", "hxx"];

/// Default template text for `.clang-tidy` config files.
pub const CLANG_TIDY_TEMPLATE: &str = "# WARNING: DO NOT EDIT THIS FILE DIRECTLY! Automatically generated and managed by formality (fml). Canonical source of truth: formality.toml. Run 'fml sync' to update.
---
Checks: '*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*'
WarningsAsErrors: ''
HeaderFilterRegex: ''
FormatStyle: none
";

/// Returns `true` if `ext` is a C++ source or header file extension.
#[must_use]
pub fn is_cpp_extension(ext: &str) -> bool {
  matches!(
    ext.to_ascii_lowercase().as_str(),
    "cpp" | "cc" | "cxx" | "hpp" | "hxx"
  )
}

/// Returns `true` if `ext` is a C source file extension.
#[must_use]
pub fn is_c_extension(ext: &str) -> bool {
  ext.eq_ignore_ascii_case("c")
}

/// Scans the provided file list and directories on disk once upfront for C++ files.
/// Returns the set of directory paths that contain at least one C++ file.
#[must_use]
pub fn scan_cpp_dirs(all_files: &[PathBuf]) -> HashSet<PathBuf> {
  let mut cpp_dirs = HashSet::new();

  // 1. Mark directories of known C++ files in `all_files`.
  for f in all_files {
    if f
      .extension()
      .and_then(|e| e.to_str())
      .is_some_and(is_cpp_extension)
      && let Some(parent) = f.parent()
    {
      cpp_dirs.insert(parent.to_path_buf());
    }
  }

  // 2. For headers in `all_files` whose parent directory isn't already known
  // to contain C++ files, scan the directory on disk once.
  let mut scanned_dirs = HashSet::new();
  for f in all_files {
    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("h")
      && let Some(parent) = f.parent()
      && !cpp_dirs.contains(parent)
      && scanned_dirs.insert(parent.to_path_buf())
      && (parent != Path::new("") || f.is_absolute())
    {
      let dir_to_read = if parent == Path::new("") {
        Path::new(".")
      } else {
        parent
      };
      if let Ok(entries) = std::fs::read_dir(dir_to_read) {
        let has_cpp_on_disk = entries.filter_map(Result::ok).any(|e| {
          let ep = e.path();
          ep.as_path() != f.as_path()
            && ep
              .extension()
              .and_then(|ext| ext.to_str())
              .is_some_and(is_cpp_extension)
        });
        if has_cpp_on_disk {
          cpp_dirs.insert(parent.to_path_buf());
        }
      }
    }
  }

  cpp_dirs
}

/// Determines the appropriate `-std=` compiler flag (`-std=c++17` or `-std=c17`) for a target file,
/// using a precomputed set of directories known to contain C++ files.
#[must_use]
pub fn std_flag_for_file_with_dirs(
  file: &Path,
  cpp_dirs: &HashSet<PathBuf>,
) -> &'static str {
  let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
  if is_cpp_extension(ext) {
    "-std=c++17"
  } else if is_c_extension(ext) {
    "-std=c17"
  } else if ext.eq_ignore_ascii_case("h") {
    let parent = file.parent().unwrap_or(Path::new(""));
    if cpp_dirs.contains(parent) {
      "-std=c++17"
    } else {
      "-std=c17"
    }
  } else {
    "-std=c++17"
  }
}

/// Determines the appropriate `-std=` compiler flag (`-std=c++17` or `-std=c17`) for a target file.
#[must_use]
pub fn std_flag_for_file(file: &Path, all_files: &[PathBuf]) -> &'static str {
  let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
  if is_cpp_extension(ext) {
    "-std=c++17"
  } else if is_c_extension(ext) {
    "-std=c17"
  } else if ext.eq_ignore_ascii_case("h") {
    let parent = file.parent().unwrap_or(Path::new(""));
    let cpp_dirs = scan_cpp_dirs(all_files);
    if cpp_dirs.contains(parent) {
      "-std=c++17"
    } else if (parent != Path::new("") || file.is_absolute())
      && let Ok(entries) = std::fs::read_dir(if parent == Path::new("") {
        Path::new(".")
      } else {
        parent
      })
    {
      let has_cpp_on_disk = entries.filter_map(Result::ok).any(|e| {
        let ep = e.path();
        ep.as_path() != file
          && ep
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(is_cpp_extension)
      });
      if has_cpp_on_disk {
        "-std=c++17"
      } else {
        "-std=c17"
      }
    } else {
      "-std=c17"
    }
  } else {
    "-std=c++17"
  }
}

/// Builds argument vector for clang-tidy invocation.
#[must_use]
pub fn build_clang_tidy_args(
  files: &[PathBuf],
  fix: bool,
  std_flag: &str,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = Vec::new();
  if fix {
    args.push("-fix".to_string());
    args.push("-fix-errors".to_string());
  }
  args.extend(extra_args.iter().cloned());
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.push("--".to_string());
  args.push(std_flag.to_string());
  args
}

impl LanguageSurface for CppSurface {
  fn name(&self) -> &'static str {
    "cpp"
  }

  fn aliases(&self) -> &[&'static str] {
    &["c", "c++", "cxx"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    CPP_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("CMakeLists.txt").is_file()
      || root.join("Makefile").is_file()
      || root.join("meson.build").is_file()
      || root.join(".clang-format").is_file()
      || root.join(".clang-tidy").is_file()
      || !find_files_with_ext(root, CPP_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "clang-format",
        description: "C/C++ code formatter",
        install_hint: "Install via: sudo apt install clang-format (or brew install clang-format / pip install clang-format / winget install LLVM.LLVM)",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "clang-tidy",
        description: "C/C++ linter and static analyzer",
        install_hint: "Install via: sudo apt install clang-tidy (or brew install llvm / winget install LLVM.LLVM)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "clang-format",
      start,
      Some(
        "sudo apt install clang-format / brew install clang-format / pip install clang-format / winget install LLVM.LLVM",
      ),
    ) {
      return res;
    }

    let files = ctx.matched_files(CPP_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `-style='{...}'` instead of writing `.clang-format` to disk —
    // see `build_clang_format_inline_style` (Fixes #157). `fml sync` remains
    // the only path that materializes the file.
    let inline_style =
      build_clang_format_inline_style(&ClangFormatConfig::from_context(ctx));

    if ctx.check_only {
      return diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("clang-format");
          cmd.arg(format!("-style={inline_style}"));
          cmd.arg("-i").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = create_tool_command("clang-format");
    cmd.arg(format!("-style={inline_style}"));
    cmd.arg("-i");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    run_tool_command(self.name(), &mut cmd)
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "clang-tidy",
      start,
      Some(
        "sudo apt install clang-tidy / brew install llvm / winget install LLVM.LLVM",
      ),
    ) {
      return res;
    }

    let files = ctx.matched_files(CPP_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let cpp_opts = ctx.lang_config.cpp.as_ref();
    let custom_std = cpp_opts.and_then(|c| c.standard.as_deref());

    let (c_std_flag, cpp_std_flag) = match custom_std {
      Some(raw) => {
        let trimmed = raw.trim();
        let flag = if trimmed.starts_with("-std=") {
          trimmed.to_string()
        } else {
          format!("-std={trimmed}")
        };
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("++") {
          ("-std=c17".to_string(), flag)
        } else if lower.starts_with("c") || lower.starts_with("gnu") {
          (flag, "-std=c++17".to_string())
        } else {
          ("-std=c17".to_string(), flag)
        }
      }
      None => ("-std=c17".to_string(), "-std=c++17".to_string()),
    };

    let cpp_dirs = scan_cpp_dirs(&files);
    let mut c_files = Vec::new();
    let mut cpp_files = Vec::new();

    for f in &files {
      let flag = std_flag_for_file_with_dirs(f, &cpp_dirs);
      if flag == "-std=c17" {
        c_files.push(f.clone());
      } else {
        cpp_files.push(f.clone());
      }
    }

    let groups: Vec<(Vec<PathBuf>, String)> =
      [(c_files, c_std_flag), (cpp_files, cpp_std_flag)]
        .into_iter()
        .filter(|(flist, _)| !flist.is_empty())
        .collect();

    // Inline `--config='{...}'` instead of reading `.clang-tidy` off disk —
    // see `build_clang_tidy_inline_config` (Fixes #157). `fml sync` remains
    // the only path that materializes the file.
    let inline_config =
      build_clang_tidy_inline_config(&ClangTidyConfig::from_context(ctx));

    let mut failed_outputs = Vec::new();

    for (flist, std_flag) in groups {
      let mut cmd = create_tool_command("clang-tidy");
      cmd.arg(format!("--config={inline_config}"));
      let args = build_clang_tidy_args(
        &flist,
        fix,
        &std_flag,
        &ctx.lang_config.extra_args,
      );
      cmd.args(&args);
      cmd.current_dir(ctx.root.as_path());

      match cmd.output() {
        Ok(output) => {
          if !output.status.success() {
            failed_outputs.push(output);
          }
        }
        Err(e) => {
          return SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ExecutionError {
              message: format!("Failed to execute clang-tidy: {e}"),
            },
            duration: start.elapsed(),
          };
        }
      }
    }

    if failed_outputs.is_empty() {
      SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::Passed,
        duration: start.elapsed(),
      }
    } else {
      let mut msgs = Vec::new();
      for output in failed_outputs {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let msg = if stderr.trim().is_empty() {
          stdout
        } else {
          stderr
        };
        if !msg.trim().is_empty() {
          msgs.push(msg);
        }
      }
      let final_msg = if msgs.is_empty() {
        "clang-tidy violations found".to_string()
      } else {
        msgs.join("\n")
      };

      SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ViolationsFound {
          message: final_msg,
          diff: None,
        },
        duration: start.elapsed(),
      }
    }
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #157): they
  // pass the resolved config to clang-format/clang-tidy inline via
  // `-style='{...}'`/`--config='{...}'` (see `build_clang_format_inline_style`
  // and `build_clang_tidy_inline_config`, used in `format()`/`lint()`
  // above). This was left as a documented exception in #151 because neither
  // tool was installed in that pass's environment to verify byte-identical
  // output — verified here with LLVM 22.1.8 (clang-format/clang-tidy)
  // actually installed and invoked: both the LLVM/2-space default style and
  // a custom Google/4-space/Right-pointer/Allman style produce
  // byte-identical formatted output via `-style=` vs a `.clang-format` file,
  // and clang-tidy's diagnostic output is identical via `--config=` vs a
  // `.clang-tidy` file. This method is now reached only by `fml sync`, for
  // users who explicitly want the native files materialized on disk (e.g.
  // for editor/clangd integration outside of `fml`).
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    let format_res =
      sync_native_config::<ClangFormatConfig>(ctx, check, start, self.name());

    if !format_res.is_success() {
      return format_res;
    }

    let tidy_res = sync_clang_tidy_config(ctx, check, start, self.name());
    if !tidy_res.is_success() {
      return tidy_res;
    }

    if matches!(tidy_res.status, SurfaceStatus::ConfigSynced { .. }) {
      tidy_res
    } else {
      format_res
    }
  }
}

/// Synchronizes `.clang-tidy` native configuration file.
#[must_use]
pub fn sync_clang_tidy_config(
  ctx: &ExecutionContext,
  check: bool,
  start: Instant,
  surface_name: &'static str,
) -> SurfaceResult {
  sync_native_config::<ClangTidyConfig>(ctx, check, start, surface_name)
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::FormalityConfig;
  use crate::surfaces::{check_binary_exists, test_ctx};
  use std::sync::Arc;
  use tempfile::tempdir;

  #[test]
  fn test_std_flag_for_c_files() {
    let all_files =
      vec![PathBuf::from("src/main.c"), PathBuf::from("src/utils.c")];
    assert_eq!(
      std_flag_for_file(Path::new("src/main.c"), &all_files),
      "-std=c17"
    );
    assert_eq!(
      std_flag_for_file(Path::new("src/utils.c"), &all_files),
      "-std=c17"
    );
  }

  #[test]
  fn test_std_flag_for_cpp_files() {
    let all_files = vec![
      PathBuf::from("src/main.cpp"),
      PathBuf::from("src/app.cc"),
      PathBuf::from("src/engine.cxx"),
      PathBuf::from("src/math.hpp"),
      PathBuf::from("src/types.hxx"),
    ];
    assert_eq!(
      std_flag_for_file(Path::new("src/main.cpp"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(Path::new("src/app.cc"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(Path::new("src/engine.cxx"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(Path::new("src/math.hpp"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(Path::new("src/types.hxx"), &all_files),
      "-std=c++17"
    );
  }

  #[test]
  fn test_std_flag_for_header_without_cpp_siblings() {
    let all_files =
      vec![PathBuf::from("src/main.c"), PathBuf::from("src/utils.h")];
    assert_eq!(
      std_flag_for_file(Path::new("src/utils.h"), &all_files),
      "-std=c17"
    );
  }

  #[test]
  fn test_std_flag_for_header_with_cpp_siblings() {
    let all_files =
      vec![PathBuf::from("src/main.cpp"), PathBuf::from("src/utils.h")];
    assert_eq!(
      std_flag_for_file(Path::new("src/utils.h"), &all_files),
      "-std=c++17"
    );
  }

  #[test]
  fn test_std_flag_for_header_on_disk_detection() {
    let dir = tempdir().unwrap();
    let c_dir = dir.path().join("c_code");
    std::fs::create_dir_all(&c_dir).unwrap();
    let c_header = c_dir.join("header.h");
    let c_source = c_dir.join("source.c");
    std::fs::write(&c_header, "").unwrap();
    std::fs::write(&c_source, "").unwrap();

    let cpp_dir = dir.path().join("cpp_code");
    std::fs::create_dir_all(&cpp_dir).unwrap();
    let cpp_header = cpp_dir.join("header.h");
    let cpp_source = cpp_dir.join("source.cpp");
    std::fs::write(&cpp_header, "").unwrap();
    std::fs::write(&cpp_source, "").unwrap();

    assert_eq!(std_flag_for_file(&c_header, &[]), "-std=c17");
    assert_eq!(std_flag_for_file(&cpp_header, &[]), "-std=c++17");
  }

  #[test]
  fn test_scan_cpp_dirs_and_std_flag_for_file_with_dirs() {
    let dir = tempdir().unwrap();
    let c_dir = dir.path().join("c_pkg");
    std::fs::create_dir_all(&c_dir).unwrap();
    let c_header = c_dir.join("c_header.h");
    let c_source = c_dir.join("c_source.c");
    std::fs::write(&c_header, "").unwrap();
    std::fs::write(&c_source, "").unwrap();

    let cpp_dir = dir.path().join("cpp_pkg");
    std::fs::create_dir_all(&cpp_dir).unwrap();
    let cpp_header = cpp_dir.join("cpp_header.h");
    let cpp_source = cpp_dir.join("cpp_source.cpp");
    std::fs::write(&cpp_header, "").unwrap();
    std::fs::write(&cpp_source, "").unwrap();

    let files = vec![
      c_source.clone(),
      c_header.clone(),
      cpp_source.clone(),
      cpp_header.clone(),
    ];

    let cpp_dirs = scan_cpp_dirs(&files);
    assert!(cpp_dirs.contains(&cpp_dir));
    assert!(!cpp_dirs.contains(&c_dir));

    assert_eq!(
      std_flag_for_file_with_dirs(&c_source, &cpp_dirs),
      "-std=c17"
    );
    assert_eq!(
      std_flag_for_file_with_dirs(&c_header, &cpp_dirs),
      "-std=c17"
    );
    assert_eq!(
      std_flag_for_file_with_dirs(&cpp_source, &cpp_dirs),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file_with_dirs(&cpp_header, &cpp_dirs),
      "-std=c++17"
    );
  }

  #[test]
  fn test_build_clang_tidy_args_without_fix() {
    let files = vec![PathBuf::from("src/main.c"), PathBuf::from("src/utils.c")];
    let extra_args = vec!["--checks=*".to_string()];
    let args = build_clang_tidy_args(&files, false, "-std=c17", &extra_args);
    assert_eq!(
      args,
      vec![
        "--checks=*".to_string(),
        "src/main.c".to_string(),
        "src/utils.c".to_string(),
        "--".to_string(),
        "-std=c17".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_clang_tidy_args_with_fix() {
    let files = vec![PathBuf::from("src/app.cpp")];
    let args = build_clang_tidy_args(&files, true, "-std=c++17", &[]);
    assert_eq!(
      args,
      vec![
        "-fix".to_string(),
        "-fix-errors".to_string(),
        "src/app.cpp".to_string(),
        "--".to_string(),
        "-std=c++17".to_string(),
      ]
    );
  }

  #[test]
  fn test_clang_tidy_template_content() {
    assert!(
      CLANG_TIDY_TEMPLATE.contains("WARNING: DO NOT EDIT THIS FILE DIRECTLY!")
    );
    assert!(CLANG_TIDY_TEMPLATE.contains("Checks: '*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*'"));
    assert!(CLANG_TIDY_TEMPLATE.contains("WarningsAsErrors: ''"));
    assert!(CLANG_TIDY_TEMPLATE.contains("HeaderFilterRegex: ''"));
    assert!(CLANG_TIDY_TEMPLATE.contains("FormatStyle: none"));
  }

  #[test]
  fn test_sync_config_generates_clang_tidy_and_clang_format() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let cfg = FormalityConfig::default();
    let mut ctx = test_ctx(&root, cfg.resolve_for_lang("cpp"));
    ctx.global_config = Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let res = surface.sync_config(&ctx, false);
    assert!(res.is_success());

    let format_path = root.join(".clang-format");
    let tidy_path = root.join(".clang-tidy");

    assert!(format_path.is_file());
    assert!(tidy_path.is_file());

    let format_content = std::fs::read_to_string(&format_path).unwrap();
    let tidy_content = std::fs::read_to_string(&tidy_path).unwrap();

    assert!(format_content.contains("Language: Cpp"));
    assert!(tidy_content.contains("Checks: '*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*'"));

    let check_res = surface.sync_config(&ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }
  #[test]
  fn test_clang_format_config_typed_serialization() {
    let cfg = ClangFormatConfig {
      language: "Cpp".to_string(),
      based_on_style: "LLVM".to_string(),
      indent_width: 4,
      column_limit: 100,
      use_tab: "Never".to_string(),
      line_ending: "LF".to_string(),
      pointer_alignment: "Left".to_string(),
      break_before_braces: "Attach".to_string(),
      sort_includes: true,
      standard: Some("c++17".to_string()),
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("Language: Cpp"));
    assert!(rendered.contains("BasedOnStyle: LLVM"));
    assert!(rendered.contains("IndentWidth: 4"));
    assert!(rendered.contains("ColumnLimit: 100"));
    assert!(rendered.contains("UseTab: Never"));
    assert!(rendered.contains("LineEnding: LF"));
    assert!(rendered.contains("PointerAlignment: Left"));
    assert!(rendered.contains("BreakBeforeBraces: Attach"));
    assert!(rendered.contains("SortIncludes: true"));
    assert!(rendered.contains("Standard: c++17"));
  }

  #[test]
  fn test_clang_tidy_config_typed_serialization() {
    let cfg = ClangTidyConfig::default();
    let rendered = cfg.render().unwrap();
    assert!(rendered.starts_with(crate::surfaces::AUTO_GENERATED_HEADER));
    assert!(rendered.contains("Checks:"));
    assert!(rendered.contains("FormatStyle: none"));
  }
  #[test]
  fn test_build_clang_tidy_args_with_fix_and_extra_args() {
    let files = vec![PathBuf::from("src/app.cpp")];
    let extra_args = vec![
      "--checks=-*,llvm-*".to_string(),
      "--warnings-as-errors=*".to_string(),
    ];
    let args = build_clang_tidy_args(&files, true, "-std=c++17", &extra_args);
    assert_eq!(
      args,
      vec![
        "-fix".to_string(),
        "-fix-errors".to_string(),
        "--checks=-*,llvm-*".to_string(),
        "--warnings-as-errors=*".to_string(),
        "src/app.cpp".to_string(),
        "--".to_string(),
        "-std=c++17".to_string(),
      ]
    );
  }
  #[test]
  fn test_sync_config_with_custom_style_knobs() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let toml_str = r#"
      [lang.cpp]
      indent_size = 4
      line_length = 80
      column_limit = 120
      standard = "c++20"
      use_tabs = false
      based_on_style = "Google"
      pointer_alignment = "Right"
      break_before_braces = "Allman"
      sort_includes = false
    "#;
    let cfg = FormalityConfig::parse_str(toml_str, Path::new("formality.toml"))
      .unwrap();
    let mut ctx = test_ctx(&root, cfg.resolve_for_lang("cpp"));
    ctx.global_config = Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let res = surface.sync_config(&ctx, false);
    assert!(res.is_success());

    let format_path = root.join(".clang-format");
    assert!(format_path.is_file());

    let format_content = std::fs::read_to_string(&format_path).unwrap();
    assert!(format_content.contains("Language: Cpp"));
    assert!(format_content.contains("BasedOnStyle: Google"));
    assert!(format_content.contains("IndentWidth: 4"));
    assert!(format_content.contains("ColumnLimit: 120"));
    assert!(format_content.contains("Standard: c++20"));
    assert!(format_content.contains("UseTab: Never"));
    assert!(format_content.contains("PointerAlignment: Right"));
    assert!(format_content.contains("BreakBeforeBraces: Allman"));
    assert!(format_content.contains("SortIncludes: false"));

    let check_res = surface.sync_config(&ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_build_clang_format_inline_style_shape() {
    let cfg = ClangFormatConfig {
      language: "Cpp".to_string(),
      based_on_style: "Google".to_string(),
      indent_width: 4,
      column_limit: 100,
      use_tab: "Never".to_string(),
      line_ending: "LF".to_string(),
      pointer_alignment: "Right".to_string(),
      break_before_braces: "Allman".to_string(),
      sort_includes: false,
      standard: None,
    };
    let inline = build_clang_format_inline_style(&cfg);
    assert_eq!(
      inline,
      "{Language: Cpp, BasedOnStyle: Google, IndentWidth: 4, ColumnLimit: 100, UseTab: Never, LineEnding: LF, PointerAlignment: Right, BreakBeforeBraces: Allman, SortIncludes: false}"
    );

    let cfg_with_std = ClangFormatConfig {
      standard: Some("c++20".to_string()),
      ..cfg
    };
    let inline_with_std = build_clang_format_inline_style(&cfg_with_std);
    assert_eq!(
      inline_with_std,
      "{Language: Cpp, BasedOnStyle: Google, IndentWidth: 4, ColumnLimit: 100, UseTab: Never, LineEnding: LF, PointerAlignment: Right, BreakBeforeBraces: Allman, SortIncludes: false, Standard: c++20}"
    );
  }

  #[test]
  fn test_clang_format_config_line_ending_cr_fallback() {
    let global = crate::config::ResolvedGlobalConfig {
      end_of_line: "cr".to_string(),
      ..Default::default()
    };
    let mut ctx = test_ctx(
      Path::new("."),
      crate::config::ResolvedLangConfig::new("cpp"),
    );
    ctx.global_config = Arc::new(global);
    let cfg = ClangFormatConfig::from_context(&ctx);
    assert_eq!(cfg.line_ending, "LF");
  }

  #[test]
  fn test_build_clang_tidy_inline_config_shape() {
    let cfg = ClangTidyConfig::default();
    let inline = build_clang_tidy_inline_config(&cfg);
    assert!(inline.starts_with("{Checks: '*,-fuchsia-*"));
    assert!(inline.contains("FormatStyle: none"));
  }

  #[test]
  fn test_cpp_format_does_not_write_clang_format() {
    // Fixes #157: `fml fmt` must not write `.clang-format` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !check_binary_exists("clang-format") {
      return;
    }
    let temp = tempdir().unwrap();
    std::fs::write(temp.path().join("main.cpp"), "int main(){return 0;}\n")
      .unwrap();

    let cfg = FormalityConfig::default();
    let mut ctx = test_ctx(temp.path(), cfg.resolve_for_lang("cpp"));
    ctx.global_config = Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let _ = surface.format(&ctx);

    assert!(
      !temp.path().join(".clang-format").exists(),
      "fml fmt must not write .clang-format"
    );
  }

  #[test]
  fn test_cpp_lint_does_not_write_clang_tidy() {
    // Fixes #157: `fml lint` must not write `.clang-tidy` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !check_binary_exists("clang-tidy") {
      return;
    }
    let temp = tempdir().unwrap();
    std::fs::write(temp.path().join("main.cpp"), "int main() { return 0; }\n")
      .unwrap();

    let cfg = FormalityConfig::default();
    let mut ctx = test_ctx(temp.path(), cfg.resolve_for_lang("cpp"));
    ctx.global_config = Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let _ = surface.lint(&ctx, false);

    assert!(
      !temp.path().join(".clang-tidy").exists(),
      "fml lint must not write .clang-tidy"
    );
  }
}
