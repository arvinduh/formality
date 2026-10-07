//! C/C++ language surface: formats via `clang-format` and lints via `clang-tidy`.
//!
//! Implements `super::LanguageSurface` for C and C++, syncing `.clang-format`
//! and `.clang-tidy`. Fleet registration is owned by `super::registry`.

use crate::config;
use crate::config::facets;
use crate::config::facets::DeclaresFacets;
use crate::surfaces;
use crate::surfaces::LanguageSurface;
use crate::surfaces::sync;
use crate::surfaces::sync::native;
use crate::surfaces::tooling;
use std::collections;
use std::hash;
use std::path;
use std::time;

/// The `.clang-format` settings `ctx` resolves to. clang-format's `-style=`
/// takes the same keys as one flow map, so `fml fmt` passes
/// [`native::ToolConfig::flow`] instead of writing the file
/// (#157 [pre-recreation]).
fn clang_format_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let cpp = ctx.lang_config.cpp.as_ref();
  let line_ending =
    if ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf") {
      "CRLF"
    } else {
      "LF"
    };
  let cfg = native::ToolConfig::new(".clang-format")
    .set("Language", "Cpp")
    .set(
      "BasedOnStyle",
      cpp
        .and_then(|c| c.based_on_style.as_deref())
        .unwrap_or("LLVM"),
    )
    .set("IndentWidth", native::int(ctx.lang_config.indent_size))
    .set(
      "ColumnLimit",
      native::int(
        cpp
          .and_then(|c| c.column_limit)
          .unwrap_or(ctx.lang_config.line_length),
      ),
    )
    .set(
      "UseTab",
      if ctx.lang_config.use_tabs {
        "Always"
      } else {
        "Never"
      },
    )
    .set("LineEnding", line_ending)
    .set(
      "PointerAlignment",
      cpp
        .and_then(|c| c.pointer_alignment.as_deref())
        .unwrap_or("Left"),
    )
    .set(
      "BreakBeforeBraces",
      cpp
        .and_then(|c| c.break_before_braces.as_deref())
        .unwrap_or("Attach"),
    )
    .set(
      "SortIncludes",
      cpp.and_then(|c| c.sort_includes).unwrap_or(true),
    );
  match cpp.and_then(|c| c.standard.as_deref()) {
    Some(standard) => {
      cfg.set("Standard", standard.trim().trim_start_matches("-std="))
    }
    None => cfg,
  }
}

/// The `.clang-tidy` settings. clang-tidy's `--config=` takes them as one
/// flow map, so `fml lint` passes [`native::ToolConfig::flow`].
fn clang_tidy_config() -> native::ToolConfig {
  native::ToolConfig::new(".clang-tidy")
    .set(
      "Checks",
      "*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*",
    )
    .set("WarningsAsErrors", "")
    .set("HeaderFilterRegex", "")
    .set("FormatStyle", "none")
}

/// C/C++ language surface implementation.
#[derive(Debug, Default)]
pub struct CppSurface;

impl DeclaresFacets for CppSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs
      | facets::Facet::IndentWidth
      | facets::Facet::LineLength
      | facets::Facet::ImportSort
      | facets::Facet::Standard => facets::FacetSupport::Configurable,
      facets::Facet::QuoteStyle
      | facets::Facet::TrailingComma
      | facets::Facet::ProseWrap
      | facets::Facet::Edition => facets::FacetSupport::Unsupported,
    }
  }
}

/// Standard file extensions recognized for C/C++ source and header files.
const CPP_EXTENSIONS: &[&str] =
  &["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"];

/// Returns `true` if `ext` is a C++ source or header file extension.
#[must_use]
fn is_cpp_extension(ext: &str) -> bool {
  matches!(
    ext.to_ascii_lowercase().as_str(),
    "cpp" | "cc" | "cxx" | "hpp" | "hxx"
  )
}

/// Returns `true` if `ext` is a C source file extension.
#[must_use]
fn is_c_extension(ext: &str) -> bool {
  ext.eq_ignore_ascii_case("c")
}

/// Scans the provided file list and directories on disk once upfront for C++ files.
/// Returns the set of directory paths that contain at least one C++ file.
#[must_use]
fn scan_cpp_dirs(
  all_files: &[path::PathBuf],
) -> collections::HashSet<path::PathBuf> {
  let mut cpp_dirs = collections::HashSet::new();

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
  let mut scanned_dirs = collections::HashSet::new();
  for f in all_files {
    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("h")
      && let Some(parent) = f.parent()
      && !cpp_dirs.contains(parent)
      && scanned_dirs.insert(parent.to_path_buf())
      && (parent != path::Path::new("") || f.is_absolute())
    {
      let dir_to_read = if parent == path::Path::new("") {
        path::Path::new(".")
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
fn std_flag_for_file_with_dirs<S: hash::BuildHasher>(
  file: &path::Path,
  cpp_dirs: &collections::HashSet<path::PathBuf, S>,
) -> &'static str {
  let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
  if is_cpp_extension(ext) {
    "-std=c++17"
  } else if is_c_extension(ext) {
    "-std=c17"
  } else if ext.eq_ignore_ascii_case("h") {
    let parent = file.parent().unwrap_or(path::Path::new(""));
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
pub fn std_flag_for_file(
  file: &path::Path,
  all_files: &[path::PathBuf],
) -> &'static str {
  let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
  if is_cpp_extension(ext) {
    "-std=c++17"
  } else if is_c_extension(ext) {
    "-std=c17"
  } else if ext.eq_ignore_ascii_case("h") {
    let parent = file.parent().unwrap_or(path::Path::new(""));
    let cpp_dirs = scan_cpp_dirs(all_files);
    if cpp_dirs.contains(parent) {
      "-std=c++17"
    } else if (parent != path::Path::new("") || file.is_absolute())
      && let Ok(entries) = std::fs::read_dir(if parent == path::Path::new("") {
        path::Path::new(".")
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
  files: &[path::PathBuf],
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

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["clang-format", "clang-tidy"]
  }

  fn aliases(&self) -> &[&'static str] {
    &["c", "c++", "cxx"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    CPP_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(Self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn marker_files(&self) -> &[&'static str] {
    &[
      "CMakeLists.txt",
      "Makefile",
      "meson.build",
      ".clang-format",
      ".clang-tidy",
    ]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![
      surfaces::ToolInfo {
        binary: "clang-format",
        description: "C/C++ code formatter",
        install_hint: None,
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      surfaces::ToolInfo {
        binary: "clang-tidy",
        description: "C/C++ linter and static analyzer",
        install_hint: None,
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    let files =
      match ctx.files_for(self.name(), "clang-format", CPP_EXTENSIONS, start) {
        Ok(files) => files,
        Err(res) => return res,
      };

    // Inline `-style='{...}'` instead of writing `.clang-format` to disk —
    // see `clang_format_config` (Fixes #157 [pre-recreation]). `fml sync` remains
    // the only path that materializes the file.
    let inline_style = clang_format_config(ctx).flow(native::Quote::None);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = ctx.command("clang-format");
          cmd.arg(format!("-style={inline_style}"));
          cmd.arg("-i").arg(scratch);
          cmd.args(ctx.lang_config.tool_args("clang-format"));
          cmd.output()
        },
        self.name(),
        start,
        // `clang-format -i` rewrites the scratch copy and exits 0 whether or
        // not it changed anything; it has no "differences found" exit code
        // (that is `--dry-run --Werror`, which this path never passes). A
        // non-zero exit means clang-format could not do its job — an
        // unreadable file, an invalid `-style`, an unknown flag from
        // `extra_args` — so every non-zero exit is an `ExecutionError`
        // (Fixes #151). Same reasoning applies verbatim to the non-`--check`
        // write branch below (Fixes #155).
        tooling::classify_all_nonzero_as_error,
      );
    }

    let mut cmd = ctx.command("clang-format");
    cmd.arg(format!("-style={inline_style}"));
    cmd.arg("-i");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(ctx.lang_config.tool_args("clang-format"));

    tooling::run_tool_command_classified(
      self.name(),
      &mut cmd,
      tooling::classify_all_nonzero_as_error,
    )
  }

  #[expect(
    clippy::too_many_lines,
    reason = "orchestrates clang-tidy execution across C and C++ files with std detection"
  )]
  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    let files =
      match ctx.files_for(self.name(), "clang-tidy", CPP_EXTENSIONS, start) {
        Ok(files) => files,
        Err(res) => return res,
      };

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
        } else if lower.starts_with('c') || lower.starts_with("gnu") {
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

    let groups: Vec<(Vec<path::PathBuf>, String)> =
      [(c_files, c_std_flag), (cpp_files, cpp_std_flag)]
        .into_iter()
        .filter(|(flist, _)| !flist.is_empty())
        .collect();

    // Inline `--config='{...}'` instead of reading `.clang-tidy` off disk —
    // see `clang_tidy_config` (Fixes #157 [pre-recreation]). `fml sync` remains
    // the only path that materializes the file.
    let inline_config = clang_tidy_config().flow(native::Quote::Single);

    let mut failed_outputs = Vec::new();

    for (flist, std_flag) in groups {
      let mut cmd = ctx.command("clang-tidy");
      cmd.arg(format!("--config={inline_config}"));
      let args = build_clang_tidy_args(
        &flist,
        fix,
        &std_flag,
        ctx.lang_config.tool_args("clang-tidy"),
      );
      cmd.args(&args);

      match cmd.output() {
        Ok(output) => {
          if !output.status.success() {
            failed_outputs.push(output);
          }
        }
        Err(e) => {
          return surfaces::SurfaceResult::error(
            self.name(),
            start,
            format!("Failed to execute clang-tidy: {e}"),
          );
        }
      }
    }

    if failed_outputs.is_empty() {
      surfaces::SurfaceResult::new(
        self.name(),
        start,
        surfaces::SurfaceStatus::Passed,
      )
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

      surfaces::SurfaceResult::new(
        self.name(),
        start,
        surfaces::SurfaceStatus::ViolationsFound {
          message: final_msg,
          diff: None,
        },
      )
    }
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #157 [pre-recreation]): they
  // pass the resolved config to clang-format/clang-tidy inline via
  // `-style='{...}'`/`--config='{...}'` (see `clang_format_config`
  // and `clang_tidy_config`, used in `format()`/`lint()`
  // above). This was left as a documented exception in #151 [pre-recreation] because neither
  // tool was installed in that pass's environment to verify byte-identical
  // output — verified here with LLVM 22.1.8 (clang-format/clang-tidy)
  // actually installed and invoked: both the LLVM/2-space default style and
  // a custom Google/4-space/Right-pointer/Allman style produce
  // byte-identical formatted output via `-style=` vs a `.clang-format` file,
  // and clang-tidy's diagnostic output is identical via `--config=` vs a
  // `.clang-tidy` file. This method is now reached only by `fml sync`, for
  // users who explicitly want the native files materialized on disk (e.g.
  // for editor/clangd integration outside of `fml`).
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    // Each file is timed from its own `Instant` because
    // `merge_sync_results` sums the durations it is given; sharing one start
    // would double-count the first file's time.
    let format_res = clang_format_config(ctx).sync(
      ctx,
      check,
      time::Instant::now(),
      self.name(),
    );

    if !format_res.is_success() {
      return format_res;
    }

    let tidy_res =
      sync_clang_tidy_config(ctx, check, time::Instant::now(), self.name());

    // Both filenames are reported, not just whichever happened to be
    // written (#130): `.clang-format` used to be dropped whenever
    // `.clang-tidy` was also synced.
    sync::merge_sync_results(&[format_res, tidy_res])
  }
}

/// Synchronizes `.clang-tidy` native configuration file.
#[must_use]
fn sync_clang_tidy_config(
  ctx: &surfaces::ExecutionContext,
  check: bool,
  start: time::Instant,
  surface_name: &'static str,
) -> surfaces::SurfaceResult {
  clang_tidy_config().sync(ctx, check, start, surface_name)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::surfaces;
  use std::path;
  use std::sync;

  #[test]
  fn std_flag_for_c_files() {
    let all_files = vec![
      path::PathBuf::from("src/main.c"),
      path::PathBuf::from("src/utils.c"),
    ];
    assert_eq!(
      std_flag_for_file(path::Path::new("src/main.c"), &all_files),
      "-std=c17"
    );
    assert_eq!(
      std_flag_for_file(path::Path::new("src/utils.c"), &all_files),
      "-std=c17"
    );
  }

  #[test]
  fn std_flag_for_cpp_files() {
    let all_files = vec![
      path::PathBuf::from("src/main.cpp"),
      path::PathBuf::from("src/app.cc"),
      path::PathBuf::from("src/engine.cxx"),
      path::PathBuf::from("src/math.hpp"),
      path::PathBuf::from("src/types.hxx"),
    ];
    assert_eq!(
      std_flag_for_file(path::Path::new("src/main.cpp"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(path::Path::new("src/app.cc"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(path::Path::new("src/engine.cxx"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(path::Path::new("src/math.hpp"), &all_files),
      "-std=c++17"
    );
    assert_eq!(
      std_flag_for_file(path::Path::new("src/types.hxx"), &all_files),
      "-std=c++17"
    );
  }

  #[test]
  fn std_flag_for_header_without_cpp_siblings() {
    let all_files = vec![
      path::PathBuf::from("src/main.c"),
      path::PathBuf::from("src/utils.h"),
    ];
    assert_eq!(
      std_flag_for_file(path::Path::new("src/utils.h"), &all_files),
      "-std=c17"
    );
  }

  #[test]
  fn std_flag_for_header_with_cpp_siblings() {
    let all_files = vec![
      path::PathBuf::from("src/main.cpp"),
      path::PathBuf::from("src/utils.h"),
    ];
    assert_eq!(
      std_flag_for_file(path::Path::new("src/utils.h"), &all_files),
      "-std=c++17"
    );
  }

  #[test]
  fn std_flag_for_header_on_disk_detection() {
    let dir = tempfile::tempdir().unwrap();
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
  fn scan_cpp_dirs_and_std_flag_for_file_with_dirs() {
    let dir = tempfile::tempdir().unwrap();
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
  fn build_clang_tidy_args_without_fix() {
    let files = vec![
      path::PathBuf::from("src/main.c"),
      path::PathBuf::from("src/utils.c"),
    ];
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
  fn build_clang_tidy_args_with_fix() {
    let files = vec![path::PathBuf::from("src/app.cpp")];
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
  fn sync_config_generates_clang_tidy_and_clang_format() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let cfg = config::FormalityConfig::default();
    let mut ctx = surfaces::test_ctx(&root, cfg.resolve_for_lang("cpp"));
    ctx.global_config = sync::Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let res = surface.sync_config(&ctx, false);
    assert!(res.is_success());
    // Fixes #130: both files are named. `.clang-format` used to be dropped
    // whenever `.clang-tidy` was also written.
    assert_eq!(
      res.status.created_file_names(),
      [".clang-format", ".clang-tidy"]
    );

    let format_path = root.join(".clang-format");
    let tidy_path = root.join(".clang-tidy");

    assert!(format_path.is_file());
    assert!(tidy_path.is_file());

    let format_content = std::fs::read_to_string(&format_path).unwrap();
    let tidy_content = std::fs::read_to_string(&tidy_path).unwrap();

    assert!(format_content.contains("Language: Cpp"));
    assert!(tidy_content.contains("Checks: '*,-fuchsia-*,-google-readability-todo,-llvm-header-guard,-llvmlibc-*'"));

    let check_res = surface.sync_config(&ctx, true);
    assert!(matches!(check_res.status, surfaces::SurfaceStatus::Passed));
  }
  /// `-style=` carries exactly the file's keys in file order, `Standard` only
  /// when set (with any `-std=` prefix dropped), and CR falls back to LF.
  #[test]
  fn clang_format_inline_style_table() {
    let base = "{Language: Cpp, BasedOnStyle: LLVM, IndentWidth: 2, \
                ColumnLimit: 80, UseTab: Never, LineEnding: LF, \
                PointerAlignment: Left, BreakBeforeBraces: Attach, \
                SortIncludes: true";
    // (standard option, end_of_line, expected -style=)
    let cases = [
      (None, "lf", format!("{base}}}")),
      (None, "cr", format!("{base}}}")),
      (
        Some("-std=c++20"),
        "lf",
        format!("{base}, Standard: c++20}}"),
      ),
      (
        None,
        "crlf",
        format!("{base}}}").replace("LineEnding: LF", "LineEnding: CRLF"),
      ),
    ];
    for (standard, eol, expected) in cases {
      let mut lang = config::ResolvedLangConfig::new("cpp");
      lang.indent_size = 2;
      lang.line_length = 80;
      lang.cpp = standard.map(|s| config::options::CppOptions {
        standard: Some(s.to_string()),
        ..Default::default()
      });
      let mut ctx = surfaces::test_ctx(path::Path::new("."), lang);
      ctx.global_config = sync::Arc::new(config::ResolvedGlobalConfig {
        end_of_line: eol.to_string(),
        ..Default::default()
      });
      assert_eq!(
        clang_format_config(&ctx).flow(native::Quote::None),
        expected
      );
    }
    let tidy = clang_tidy_config();
    assert!(
      tidy
        .flow(native::Quote::Single)
        .starts_with("{Checks: '*,-fuchsia-*")
    );
    assert!(tidy.render().contains("FormatStyle: none"));
  }

  #[test]
  fn build_clang_tidy_args_with_fix_and_extra_args() {
    let files = vec![path::PathBuf::from("src/app.cpp")];
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
  fn sync_config_with_custom_style_knobs() {
    let dir = tempfile::tempdir().unwrap();
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
    let cfg = config::FormalityConfig::parse_str(
      toml_str,
      path::Path::new("formality.toml"),
    )
    .unwrap();
    let mut ctx = surfaces::test_ctx(&root, cfg.resolve_for_lang("cpp"));
    ctx.global_config = sync::Arc::new(cfg.resolve_global());

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
    assert!(matches!(check_res.status, surfaces::SurfaceStatus::Passed));
  }

  #[test]
  fn cpp_format_does_not_write_clang_format() {
    // Fixes #157 [pre-recreation]: `fml fmt` must not write `.clang-format` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("clang-format") {
      return;
    }
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("main.cpp"), "int main(){return 0;}\n")
      .unwrap();

    let cfg = config::FormalityConfig::default();
    let mut ctx = surfaces::test_ctx(temp.path(), cfg.resolve_for_lang("cpp"));
    ctx.global_config = sync::Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let _ = surface.format(&ctx);

    assert!(
      !temp.path().join(".clang-format").exists(),
      "fml fmt must not write .clang-format"
    );
  }

  #[test]
  fn cpp_lint_does_not_write_clang_tidy() {
    // Fixes #157 [pre-recreation]: `fml lint` must not write `.clang-tidy` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("clang-tidy") {
      return;
    }
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("main.cpp"), "int main() { return 0; }\n")
      .unwrap();

    let cfg = config::FormalityConfig::default();
    let mut ctx = surfaces::test_ctx(temp.path(), cfg.resolve_for_lang("cpp"));
    ctx.global_config = sync::Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let _ = surface.lint(&ctx, false);

    assert!(
      !temp.path().join(".clang-tidy").exists(),
      "fml lint must not write .clang-tidy"
    );
  }

  #[test]
  fn cpp_check_reports_execution_error_on_formatter_failure() {
    // Fixes #151: when clang-format cannot run on the `fml fmt --check` path
    // (here: an unknown flag forced in via `extra_args`), the surface must
    // classify that as `ExecutionError` (`[ERR]`), not a lint-style
    // `ViolationsFound` (`[FAIL]`). `clang-format -i` has no
    // "differences found" exit code, so any non-zero exit is operational.
    if !tooling::check_binary_exists("clang-format") {
      return;
    }
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.cpp"), "int main(){return 0;}\n")
      .unwrap();

    let cfg = config::FormalityConfig::default();
    let mut lang = cfg.resolve_for_lang("cpp");
    lang.extra_args = [(
      "clang-format".to_string(),
      vec!["--this-flag-does-not-exist-fml151".to_string()],
    )]
    .into();
    let mut ctx = surfaces::test_ctx(temp.path(), lang);
    ctx.global_config = sync::Arc::new(cfg.resolve_global());
    ctx.check_only = true;

    let surface = CppSurface;
    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on --check must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn cpp_write_reports_execution_error_on_formatter_failure() {
    // Fixes #155: the non-`--check` write path must classify the same
    // operational clang-format failure as `ExecutionError`, not
    // `ViolationsFound` — mirroring
    // `cpp_check_reports_execution_error_on_formatter_failure` above.
    if !tooling::check_binary_exists("clang-format") {
      return;
    }
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.cpp"), "int main(){return 0;}\n")
      .unwrap();

    let cfg = config::FormalityConfig::default();
    let mut lang = cfg.resolve_for_lang("cpp");
    lang.extra_args = [(
      "clang-format".to_string(),
      vec!["--this-flag-does-not-exist-fml151".to_string()],
    )]
    .into();
    let mut ctx = surfaces::test_ctx(temp.path(), lang);
    ctx.global_config = sync::Arc::new(cfg.resolve_global());

    let surface = CppSurface;
    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }
}
