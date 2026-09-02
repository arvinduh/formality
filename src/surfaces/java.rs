//! Java language surface: formats via `google-java-format` and lints via
//! `checkstyle`, syncing the managed `checkstyle.xml` from
//! `formality.toml`.

use super::{
  DeclaresFacets, ExecutionContext, Facet, FacetSupport, LanguageSurface,
  NativeConfig, SurfaceResult, SurfaceStatus, ToolInfo,
  classify_all_nonzero_as_error, create_tool_command,
  diff_check_via_tempcopy_classified, find_files_with_ext,
  lint_fix_unsupported, run_tool_command, sync_native_config,
  tool_missing_guard,
};
use std::path::Path;
use std::time::Instant;

/// Detects the failure google-java-format produces when the `java` on
/// `PATH` is older than the JDK its bundled javac API was built against.
///
/// The raw output is a `NoClassDefFoundError` for an internal javac class
/// (`com.sun.tools.javac.tree.JCTree$JCAnyPattern`, which only exists in
/// newer JDKs) followed by twelve frames of JVM stack trace and a node
/// wrapper's own stack on top of that -- none of which says the one thing
/// the user needs to know, which is that their JDK is too old for the
/// formatter. `UnsupportedClassVersionError` is the same situation
/// detected earlier by the JVM, when the jar's class-file version alone is
/// already out of range.
#[must_use]
fn is_jvm_too_old_for_formatter(message: &str) -> bool {
  if message.contains("UnsupportedClassVersionError") {
    return true;
  }
  (message.contains("NoClassDefFoundError")
    || message.contains("ClassNotFoundException"))
    && (message.contains("com.sun.tools.javac")
      || message.contains("com/sun/tools/javac"))
}

/// Returns the JVM's own version line (`java -version` writes to stderr),
/// e.g. `openjdk version "17.0.13" 2024-10-15`, for naming the actual
/// culprit in the message below. Only called on the error path.
#[must_use]
fn java_version_line() -> Option<String> {
  let output = create_tool_command("java").arg("-version").output().ok()?;
  let stderr = String::from_utf8_lossy(&output.stderr);
  stderr.lines().next().map(str::trim).map(str::to_string)
}

/// Replaces a raw JVM stack trace with an explanation of what to do about
/// it, keeping the original text underneath so nothing is hidden.
///
/// Applied to the result of every `google-java-format` invocation:
/// google-java-format 1.28 and newer require JDK 21+, and the JDK a
/// machine happens to have on `PATH` is entirely outside this tool's
/// control -- a stock `ubuntu-latest` GitHub runner still defaults to JDK
/// 17, which is exactly where this fires.
#[must_use]
fn explain_jvm_incompatibility(result: SurfaceResult) -> SurfaceResult {
  let rewrite = |message: String| {
    if !is_jvm_too_old_for_formatter(&message) {
      return message;
    }
    let found = java_version_line()
      .map_or_else(String::new, |line| format!(" (found: {line})"));
    format!(
      "google-java-format could not run on the JVM on PATH{found}: it \
       failed to load a javac class that only exists in newer JDKs. \
       google-java-format 1.28 and newer require JDK 21 or later. Install \
       a newer JDK and make sure it is the `java` on PATH.\n\nOriginal \
       error:\n{message}"
    )
  };

  let status = match result.status {
    SurfaceStatus::ViolationsFound { message, diff } => {
      SurfaceStatus::ViolationsFound {
        message: rewrite(message),
        diff,
      }
    }
    SurfaceStatus::ExecutionError { message } => {
      SurfaceStatus::ExecutionError {
        message: rewrite(message),
      }
    }
    other => other,
  };

  SurfaceResult { status, ..result }
}

/// Typed configuration for Checkstyle, rendered as a Checkstyle XML module
/// tree. `indent_size` is read from `ResolvedLangConfig::indent_size` — the
/// same value `fml sync` uses to generate `.editorconfig` — rather than
/// being recomputed locally, so `checkstyle.xml` and `.editorconfig` can
/// never disagree on indentation. `resolve_for_lang` is what actually
/// derives that value from the configured `style` (Google = 2, AOSP = 4)
/// when the user hasn't pinned `indent_size` themselves. `line_length` is
/// hardcoded to 100 (google-java-format's fixed column limit; there is no
/// knob to change it), so `fml fmt` output always passes `fml lint`
/// immediately afterward ("Smart Format").
///
/// ### XML Emission Special Case
/// Unlike other surfaces (which serialize to JSON, TOML, or YAML via [`super::render_native_config`]),
/// Checkstyle uses an XML DTD hierarchy with strict module nests and comment headers.
/// `CheckstyleConfig` implements [`NativeConfig`] by emitting this XML module structure directly
/// in [`NativeConfig::render`], integrating into the standard [`sync_native_config`] workflow
/// without requiring serde serialization overhead or an XML serializer crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckstyleConfig {
  /// Maximum line length rule limit.
  pub line_length: usize,
  /// Basic offset indentation size in spaces.
  pub indent_size: usize,
}

impl NativeConfig for CheckstyleConfig {
  const FILE_NAME: &'static str = "checkstyle.xml";

  fn from_context(ctx: &ExecutionContext) -> Self {
    Self {
      // google-java-format enforces a fixed 100-column limit; there is no
      // knob to change it, so the generated lint config mirrors that rather
      // than the user's generic `line_length` facet.
      line_length: 100,
      // Already resolved from the configured `style` (Google = 2, AOSP = 4)
      // by `FormalityConfig::resolve_for_lang` — reusing it here (instead of
      // recomputing from `style` locally) is what keeps this in agreement
      // with the generated `.editorconfig`.
      indent_size: ctx.lang_config.indent_size,
    }
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    Ok(format!(
      "<?xml version=\"1.0\"?>\n\
<!DOCTYPE module PUBLIC\n\
    \"-//Checkstyle//DTD Checkstyle Configuration 1.3//EN\"\n\
    \"https://checkstyle.org/dtds/configuration_1_3.dtd\">\n\
<!-- WARNING: DO NOT EDIT THIS FILE DIRECTLY! Automatically generated and managed by formality (fml). Canonical source of truth: formality.toml. Run 'fml sync' to update. -->\n\
<module name=\"Checker\">\n\
  <property name=\"charset\" value=\"UTF-8\"/>\n\
  <property name=\"severity\" value=\"warning\"/>\n\
  <module name=\"LineLength\">\n\
    <property name=\"max\" value=\"{line_length}\"/>\n\
  </module>\n\
  <module name=\"TreeWalker\">\n\
    <module name=\"Indentation\">\n\
      <property name=\"basicOffset\" value=\"{indent_size}\"/>\n\
    </module>\n\
    <module name=\"RedundantImport\"/>\n\
    <module name=\"UnusedImports\"/>\n\
    <module name=\"ImportOrder\">\n\
      <property name=\"ordered\" value=\"true\"/>\n\
      <property name=\"separated\" value=\"true\"/>\n\
    </module>\n\
  </module>\n\
</module>\n",
      line_length = self.line_length,
      indent_size = self.indent_size,
    ))
  }
}

/// Java language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct JavaSurface;

impl DeclaresFacets for JavaSurface {
  // IndentWidth and Standard both resolve to `Configurable`, but each carries
  // its own load-bearing rationale comment explaining *why* — merging the
  // arms via an or-pattern would bury one comment under the other's variant.
  #[allow(clippy::match_same_arms)]
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      // google-java-format never uses tabs.
      Facet::IndentTabs => FacetSupport::Fixed("spaces"),
      // Indentation width is configurable, but not freely: it tracks the
      // configured `style` (Google = 2, AOSP = 4) via
      // `FormalityConfig::resolve_for_lang`, or an explicit `indent_size`
      // override. There is no single value to report as Fixed, so this is
      // Configurable — `.editorconfig` and `checkstyle.xml` both read the
      // same resolved `ResolvedLangConfig::indent_size`, so they can never
      // disagree.
      Facet::IndentWidth => FacetSupport::Configurable,
      // google-java-format hardcodes a 100-column limit; there is no flag
      // to change it.
      Facet::LineLength => FacetSupport::Fixed("100"),
      // Import organization/sorting happens automatically as part of
      // `google-java-format --replace`, and is checked via Checkstyle's
      // ImportOrder/UnusedImports modules.
      Facet::ImportSort => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ProseWrap
      | Facet::Edition => FacetSupport::Unsupported,
      // Google vs AOSP style, surfaced via `[lang.java] style = "aosp"`.
      Facet::Standard => FacetSupport::Configurable,
    }
  }
}

/// Standard file extensions recognized for Java source files.
pub const JAVA_EXTENSIONS: &[&str] = &["java"];

/// Builds argument vector for a `checkstyle -f plain` invocation whose
/// output is safe to parse for the LSP server (`fml lsp`, Fixes #159,
/// #165). Checkstyle has both an `-f xml` and `-f plain` machine-readable
/// mode; `plain` is used here since it needs no XML-parsing crate and its
/// shape — `[LEVEL] path:line[:col]: message [RuleName]` (verified against
/// a real checkstyle 10.20.2 run) — is a single line per violation, like
/// every other surface's structured-diagnostics parser in this module.
/// Does not include the `-c <config>` flag — the caller supplies that
/// separately, matching [`JavaSurface::lint`]'s own self-healing
/// `checkstyle.xml` generation.
#[must_use]
pub fn build_checkstyle_plain_args(
  files: &[std::path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["-f".to_string(), "plain".to_string()];
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

#[must_use]
fn is_aosp_style(ctx: &ExecutionContext) -> bool {
  ctx
    .lang_config
    .java
    .as_ref()
    .and_then(|j| j.style.as_deref())
    == Some("aosp")
}

impl LanguageSurface for JavaSurface {
  fn name(&self) -> &'static str {
    "java"
  }

  fn aliases(&self) -> &[&'static str] {
    &["jav"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    JAVA_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    false
  }

  fn detect(&self, root: &Path) -> bool {
    root.join("pom.xml").is_file()
      || root.join("build.gradle").is_file()
      || root.join("build.gradle.kts").is_file()
      || root.join("checkstyle.xml").is_file()
      || !find_files_with_ext(root, JAVA_EXTENSIONS, &[], &[], &[]).is_empty()
  }

  fn tool_info(
    &self,
    _config: &crate::config::ResolvedLangConfig,
  ) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "google-java-format",
        description: "Java code formatter with built-in import organizing",
        install_hint: "Install via: brew install google-java-format (or download the all-deps jar from https://github.com/google/google-java-format/releases and place a 'google-java-format' wrapper on PATH)",
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "checkstyle",
        description: "Java static analysis / style linter",
        install_hint: "Install via: brew install checkstyle (or download from https://checkstyle.org and place a 'checkstyle' wrapper on PATH)",
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(
      self.name(),
      "google-java-format",
      start,
      Some(
        "brew install google-java-format / download the all-deps jar from https://github.com/google/google-java-format/releases",
      ),
    ) {
      return res;
    }

    let files = ctx.matched_files(JAVA_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let aosp = is_aosp_style(ctx);

    // A single `--replace` invocation both formats and organizes/sorts
    // imports (Smart Format): google-java-format's pretty-printer always
    // reorders and de-duplicates imports as part of normal formatting, so
    // there is no separate import-sort pass needed like Python's ruff.
    if ctx.check_only {
      return explain_jvm_incompatibility(diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = create_tool_command("google-java-format");
          if aosp {
            cmd.arg("--aosp");
          }
          cmd.arg("--replace").arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        // `google-java-format --replace` rewrites the scratch copy and exits
        // 0 whether or not it changed anything; it has no "would reformat"
        // exit code (that is `--dry-run --set-exit-if-changed`, never passed
        // here). A non-zero exit means it could not format — a file that does
        // not parse, or the `NoClassDefFoundError` a too-old JVM raises
        // (which `explain_jvm_incompatibility` then annotates). Every
        // non-zero exit is therefore an `ExecutionError` (Fixes #151). NOTE:
        // this classifies the `--check` path only. The non-`--check` write
        // branch below still runs through the unclassified `run_tool_command`
        // (wrapped in `explain_jvm_incompatibility`), which maps the same
        // operational failure to `[FAIL] Violations found`; closing that
        // asymmetry is tracked in #155.
        classify_all_nonzero_as_error,
      ));
    }

    let mut cmd = create_tool_command("google-java-format");
    if aosp {
      cmd.arg("--aosp");
    }
    cmd.arg("--replace");

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    explain_jvm_incompatibility(run_tool_command(self.name(), &mut cmd))
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    if fix {
      return lint_fix_unsupported(self.name(), start);
    }

    if let Some(res) = tool_missing_guard(
      self.name(),
      "checkstyle",
      start,
      Some("brew install checkstyle / download from https://checkstyle.org"),
    ) {
      return res;
    }

    let files = ctx.matched_files(JAVA_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // Checkstyle requires an explicit `-c` config. If `fml sync` hasn't been
    // run yet and `checkstyle.xml` is missing from `ctx.root`, render a temporary
    // config to the system temp directory so `fml lint` remains a read-only
    // pass without writing files into `ctx.root`.
    let root_config = ctx.root.join(CheckstyleConfig::FILE_NAME);
    let (config_path, _temp_config) = if root_config.is_file() {
      (root_config, None)
    } else {
      let cfg = CheckstyleConfig::from_context(ctx);
      match cfg.render() {
        Ok(rendered) => {
          let mut temp_file = match tempfile::Builder::new()
            .prefix("checkstyle-")
            .suffix(".xml")
            .tempfile()
          {
            Ok(tf) => tf,
            Err(e) => {
              return SurfaceResult {
                surface_name: self.name(),
                status: SurfaceStatus::ExecutionError {
                  message: format!(
                    "Failed to create temporary checkstyle config: {e}"
                  ),
                },
                duration: start.elapsed(),
              };
            }
          };
          use std::io::Write;
          if let Err(e) = temp_file.write_all(rendered.as_bytes()) {
            return SurfaceResult {
              surface_name: self.name(),
              status: SurfaceStatus::ExecutionError {
                message: format!(
                  "Failed to write temporary checkstyle config: {e}"
                ),
              },
              duration: start.elapsed(),
            };
          }
          let path = temp_file.path().to_path_buf();
          (path, Some(temp_file))
        }
        Err(e) => {
          return SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ExecutionError {
              message: format!("Failed to render checkstyle config: {e}"),
            },
            duration: start.elapsed(),
          };
        }
      }
    };

    let mut cmd = create_tool_command("checkstyle");
    cmd.arg("-c").arg(&config_path);
    for f in &files {
      cmd.arg(f);
    }
    cmd.args(&ctx.lang_config.extra_args);
    cmd.current_dir(ctx.root.as_path());

    match cmd.output() {
      Ok(output) => {
        // Checkstyle exits 0 even when it reports "warning" severity
        // findings (it only fails the process on "error" severity or
        // higher), so treat any diagnostic-bearing output as a violation.
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let has_findings = stdout.contains("WARN") || stdout.contains("ERROR");

        if output.status.success() && !has_findings {
          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::Passed,
            duration: start.elapsed(),
          }
        } else {
          let msg = if !stdout.trim().is_empty() {
            stdout
          } else if !stderr.trim().is_empty() {
            stderr
          } else {
            "Checkstyle violations found in Java files".to_string()
          };

          SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ViolationsFound {
              message: msg,
              diff: None,
            },
            duration: start.elapsed(),
          }
        }
      }
      Err(e) => SurfaceResult {
        surface_name: self.name(),
        status: SurfaceStatus::ExecutionError {
          message: format!("Failed to execute checkstyle: {e}"),
        },
        duration: start.elapsed(),
      },
    }
  }

  // Left as a documented exception, verified not feasible for #157:
  // checkstyle's config is an XML *module tree* (see
  // `CheckstyleConfig::render` above), not a flat key=value map. Confirmed
  // against checkstyle 10.20.2's own `-h` output (actually installed and
  // run, not just docs): `-c=<configurationFile>` is described as
  // "Specifies the location of the file that defines the configuration
  // modules. The location can either be a filesystem location, or a name
  // passed to the ClassLoader.getResource() method" — i.e. a file path or
  // one of its built-in bundled resource names (`/google_checks.xml`,
  // `/sun_checks.xml`), never inline module-tree text. No other flag in
  // `checkstyle -h` (`-p` properties file, `-b` xpath query, etc.) accepts a
  // literal module tree either, and there is no stdin-config mechanism.
  // `-c` cannot take a `-` (stdin) sentinel the way `golangci-lint --config`
  // or `taplo -o` do, because the value is passed straight to
  // `ClassLoader.getResource()` — piping XML through stdin isn't a resource
  // location, and there's no separate flag to switch that interpretation.
  // When missing from disk, `JavaSurface::lint` renders a temporary config
  // file in the system temp directory so `fml lint` remains a read-only pass
  // without mutating `ctx.root`. google-java-format (the actual formatter tool)
  // has a fixed, unconfigurable style already, so it was never reading this file
  // in the first place — this config is specific to checkstyle, the linter.
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    let start = Instant::now();
    sync_native_config::<CheckstyleConfig>(ctx, check, start, self.name())
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;
  use crate::config::ResolvedLangConfig;
  use crate::surfaces::{check_binary_exists, test_ctx};
  use std::sync::Arc;
  use tempfile::TempDir;

  #[test]
  fn test_build_checkstyle_plain_args() {
    let files = vec![std::path::PathBuf::from("Main.java")];
    let extra = vec!["--exclude".to_string(), "generated".to_string()];
    let args = build_checkstyle_plain_args(&files, &extra);
    assert_eq!(
      args,
      vec![
        "-f".to_string(),
        "plain".to_string(),
        "Main.java".to_string(),
        "--exclude".to_string(),
        "generated".to_string(),
      ]
    );
  }

  #[test]
  fn test_java_facet_support() {
    let surface = JavaSurface;
    assert_eq!(
      surface.facet_support(Facet::IndentTabs),
      FacetSupport::Fixed("spaces")
    );
    assert_eq!(
      surface.facet_support(Facet::IndentWidth),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::LineLength),
      FacetSupport::Fixed("100")
    );
    assert_eq!(
      surface.facet_support(Facet::ImportSort),
      FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(Facet::QuoteStyle),
      FacetSupport::Unsupported
    );
  }

  #[test]
  fn test_java_surface_aliases_and_extensions() {
    let surface = JavaSurface;
    assert_eq!(surface.name(), "java");
    assert_eq!(surface.aliases(), &["jav"]);
    assert_eq!(surface.file_extensions(), &["java"]);
    assert!(!surface.supports_lint_fix());
  }

  #[test]
  fn test_java_detect_by_build_files() {
    let temp = TempDir::new().unwrap();
    let surface = JavaSurface;
    assert!(!surface.detect(temp.path()));

    std::fs::write(temp.path().join("pom.xml"), "<project></project>").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_java_detect_by_source_file() {
    let temp = TempDir::new().unwrap();
    let surface = JavaSurface;
    std::fs::write(temp.path().join("Main.java"), "class Main {}\n").unwrap();
    assert!(surface.detect(temp.path()));
  }

  #[test]
  fn test_checkstyle_config_render_google_style() {
    let cfg = CheckstyleConfig {
      line_length: 100,
      indent_size: 2,
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("DO NOT EDIT THIS FILE DIRECTLY!"));
    assert!(rendered.contains("<property name=\"max\" value=\"100\"/>"));
    assert!(rendered.contains("<property name=\"basicOffset\" value=\"2\"/>"));
    assert!(rendered.contains("<module name=\"UnusedImports\"/>"));
    assert!(rendered.contains("<module name=\"ImportOrder\">"));
    assert!(rendered.contains("<module name=\"RedundantImport\"/>"));
  }

  #[test]
  fn test_checkstyle_config_from_context_aosp_style() {
    let temp = TempDir::new().unwrap();
    // Go through real config parsing + resolve_for_lang (not a hand-built
    // ResolvedLangConfig) so this exercises the same indent_size derivation
    // `fml sync` actually uses.
    let toml_str = r#"
      [lang.java]
      style = "aosp"
    "#;
    let cfg = crate::config::FormalityConfig::parse_str(
      toml_str,
      Path::new("formality.toml"),
    )
    .unwrap();
    let lang_cfg = cfg.resolve_for_lang("java");
    assert_eq!(lang_cfg.indent_size, 4);

    let ctx = test_ctx(temp.path(), lang_cfg);

    let checkstyle_cfg = CheckstyleConfig::from_context(&ctx);
    assert_eq!(checkstyle_cfg.indent_size, 4);
    assert_eq!(checkstyle_cfg.line_length, 100);
  }

  /// Regression test for the AOSP indent-width contradiction: `fml sync`
  /// must generate `checkstyle.xml` (basicOffset) and `.editorconfig`
  /// (`indent_size`) with the *same* indent width for `[*.java]`, whatever
  /// `style` is configured. Previously `checkstyle.xml` derived 4 from
  /// `style = "aosp"` while `.editorconfig` always emitted a hardcoded 2.
  #[test]
  fn test_aosp_style_editorconfig_and_checkstyle_agree() {
    let toml_str = r#"
      [lang.java]
      style = "aosp"
    "#;
    let cfg = crate::config::FormalityConfig::parse_str(
      toml_str,
      Path::new("formality.toml"),
    )
    .unwrap();
    let lang_cfg = cfg.resolve_for_lang("java");

    let temp = TempDir::new().unwrap();
    let mut ctx = test_ctx(temp.path(), lang_cfg);
    ctx.global_config = Arc::new(cfg.resolve_global());

    let checkstyle_indent = CheckstyleConfig::from_context(&ctx).indent_size;

    let surfaces: Vec<Box<dyn LanguageSurface>> = vec![Box::new(JavaSurface)];
    let editorconfig_content =
      crate::surfaces::editorconfig::generate_editorconfig_from_config(
        &cfg, &surfaces,
      );
    // Extract the `[*.java]` section's indent_size line.
    let java_section = editorconfig_content
      .split("[*.java]")
      .nth(1)
      .expect("expected a [*.java] section since indent diverges from global");
    let editorconfig_indent: usize = java_section
      .lines()
      .find_map(|l| l.strip_prefix("indent_size = "))
      .expect("expected an indent_size line in the [*.java] section")
      .trim()
      .parse()
      .unwrap();

    assert_eq!(checkstyle_indent, 4);
    assert_eq!(editorconfig_indent, 4);
    assert_eq!(checkstyle_indent, editorconfig_indent);
  }

  #[test]
  fn test_java_sync_config_generates_checkstyle_xml() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let ctx = test_ctx(&root, ResolvedLangConfig::new("java"));

    let surface = JavaSurface;
    let res = surface.sync_config(&ctx, false);
    assert!(res.is_success());

    let config_path = root.join("checkstyle.xml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("Checker"));
    assert!(content.contains("basicOffset"));

    let check_res = surface.sync_config(&ctx, true);
    assert!(matches!(check_res.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_is_aosp_style_default_false() {
    let temp = TempDir::new().unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("java"));
    assert!(!is_aosp_style(&ctx));
  }

  #[test]
  fn test_java_lint_missing_tool() {
    if check_binary_exists("checkstyle") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("Main.java"), "class Main {}\n").unwrap();

    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("java"));

    let surface = JavaSurface;
    let res = surface.lint(&ctx, false);
    assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
  }

  #[test]
  fn test_java_format_missing_tool() {
    if check_binary_exists("google-java-format") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("Main.java"), "class Main {}\n").unwrap();

    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("java"));

    let surface = JavaSurface;
    let res = surface.format(&ctx);
    assert!(matches!(res.status, SurfaceStatus::ToolMissing { .. }));
  }

  #[test]
  fn test_java_lint_does_not_write_to_ctx_root() {
    let temp = TempDir::new().unwrap();
    let main_java = temp.path().join("Main.java");
    std::fs::write(&main_java, "class Main {}\n").unwrap();

    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("java"));

    let surface = JavaSurface;
    let _ = surface.lint(&ctx, false);

    assert!(
      !temp.path().join("checkstyle.xml").exists(),
      "fml lint must not write checkstyle.xml to ctx.root"
    );
    assert_eq!(
      std::fs::read_dir(temp.path()).unwrap().count(),
      1,
      "ctx.root must contain only Main.java after lint pass"
    );
  }

  // The real stack trace an ubuntu-latest runner (JDK 17 by default)
  // produces for google-java-format 1.35, trimmed to the frames that carry
  // the signature. Without the rewrite this is the entire diagnostic the
  // user sees for "your JDK is too old".
  const JDK_TOO_OLD_STACK: &str = "error: com/sun/tools/javac/tree/JCTree$JCAnyPattern\njava.lang.NoClassDefFoundError: com/sun/tools/javac/tree/JCTree$JCAnyPattern\n\tat com.google.googlejavaformat.java.JavaInputAstVisitor.scan(JavaInputAstVisitor.java:369)\nCaused by: java.lang.ClassNotFoundException: com.sun.tools.javac.tree.JCTree$JCAnyPattern\n";

  #[test]
  fn test_detects_jvm_too_old_signatures() {
    assert!(is_jvm_too_old_for_formatter(JDK_TOO_OLD_STACK));
    assert!(is_jvm_too_old_for_formatter(
      "java.lang.UnsupportedClassVersionError: com/google/googlejavaformat/java/Main has been compiled by a more recent version of the Java Runtime"
    ));
  }

  #[test]
  fn test_does_not_claim_jvm_too_old_for_ordinary_failures() {
    assert!(!is_jvm_too_old_for_formatter(
      "Sample.java:1:1: error: class, interface, enum, or record expected"
    ));
    assert!(
      !is_jvm_too_old_for_formatter(
        "java.lang.NoClassDefFoundError: org/example/Missing"
      ),
      "a NoClassDefFoundError for some unrelated class is not evidence \
       that the JDK is too old for the formatter"
    );
  }

  #[test]
  fn test_explain_jvm_incompatibility_rewrites_only_the_matching_message() {
    let result = SurfaceResult {
      surface_name: "java",
      status: SurfaceStatus::ExecutionError {
        message: JDK_TOO_OLD_STACK.to_string(),
      },
      duration: std::time::Duration::from_millis(1),
    };
    let SurfaceStatus::ExecutionError { message } =
      explain_jvm_incompatibility(result).status
    else {
      panic!("status variant must be preserved");
    };
    assert!(
      message.contains("require JDK 21 or later"),
      "the rewrite must state the actual requirement: {message}"
    );
    assert!(
      message.contains("JCAnyPattern"),
      "the original error must be kept underneath, not discarded: {message}"
    );

    let untouched = SurfaceResult {
      surface_name: "java",
      status: SurfaceStatus::ViolationsFound {
        message: "Sample.java:1:1: needs formatting".to_string(),
        diff: None,
      },
      duration: std::time::Duration::from_millis(1),
    };
    let SurfaceStatus::ViolationsFound { message, .. } =
      explain_jvm_incompatibility(untouched).status
    else {
      panic!("status variant must be preserved");
    };
    assert_eq!(
      message, "Sample.java:1:1: needs formatting",
      "an ordinary violation message must pass through verbatim"
    );
  }

  #[test]
  fn test_java_check_reports_execution_error_on_formatter_failure() {
    // Fixes #151: an unparseable `.java` file on `fml fmt --check` makes
    // google-java-format exit non-zero. That must classify as
    // `ExecutionError` (`[ERR]`), not a lint-style `ViolationsFound`
    // (`[FAIL]`) — `--replace` has no "would reformat" exit code, so a
    // non-zero exit is always operational.
    if !check_binary_exists("google-java-format") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("Broken.java"),
      "class Broken { void m( { int x = ; } }\n",
    )
    .unwrap();

    let surface = JavaSurface;
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("java"));
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on --check must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }
}
