//! Markdown language surface: formats via `prettier` and lints via
//! `markdownlint` (falling back to `prettier --check` if `markdownlint` is
//! unavailable), syncing the managed `.prettierrc.json` /
//! `.markdownlint.json` from `formality.toml`.

use super::{
  AUTO_GENERATED_JSON_COMMENT, DeclaresFacets, ExecutionContext, Facet,
  FacetSupport, LanguageSurface, NativeConfig, PrettierConfig, SurfaceResult,
  SurfaceStatus, ToolInfo, build_prettier_inline_args, check_binary_exists,
  classify_all_nonzero_as_error, classify_exit_one_as_violation,
  create_tool_command, diff_check_via_tempcopy_classified, install_hint_for,
  render_native_config, run_tool_command, run_tool_command_classified,
  sync_native_config, tool_missing_guard, tool_missing_result,
};
use crate::config::ResolvedLangConfig;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

/// Comment field container for markdownlint config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintComment {
  /// Comment description string.
  pub description: String,
}

/// MD007 (ul-indent) rule options for markdownlint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintMd007 {
  /// Number of spaces for list indentation.
  pub indent: usize,
}

/// MD013 (line length) rule options for markdownlint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintMd013 {
  /// Maximum line length allowed.
  pub line_length: usize,
  /// Whether to check code blocks.
  pub code_blocks: bool,
  /// Whether to check tables.
  pub tables: bool,
}

/// Native `.markdownlint.json` configuration representation for Markdown linting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownlintConfig {
  /// Warning comment header block.
  #[serde(rename = "$comment")]
  pub comment: MarkdownlintComment,
  /// Default rule enablement setting.
  pub default: bool,
  /// MD007 (ul-indent) list indentation rule settings. Synced with
  /// prettier's `tabWidth` (`indent_size`) so nested lists agree across both
  /// tools (#394).
  #[serde(rename = "MD007")]
  pub md007: MarkdownlintMd007,
  /// MD013 line length rule settings.
  #[serde(rename = "MD013")]
  pub md013: MarkdownlintMd013,
  /// MD033 (no-inline-html) rule enablement. Shipped default is `false` —
  /// see [`crate::config::MarkdownOptions::no_inline_html`] for why, and
  /// how to opt back in from `formality.toml`.
  #[serde(rename = "MD033")]
  pub md033: bool,
}

impl NativeConfig for MarkdownlintConfig {
  const FILE_NAME: &'static str = ".markdownlint.json";

  fn from_context(ctx: &ExecutionContext) -> Self {
    markdownlint_config_for_lang(&ctx.lang_config)
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

/// Builds the resolved [`MarkdownlintConfig`] from a [`ResolvedLangConfig`]
/// alone — the shared logic behind both [`NativeConfig::from_context`]
/// (used by `fml sync`/`fml fmt`/`fml lint`, which all have a full
/// [`ExecutionContext`] on hand) and [`write_markdownlint_temp_config`]
/// (also called from `fml lsp`'s `markdownlint_diagnostics`, which only
/// ever resolves a per-language config, not a full `ExecutionContext`).
fn markdownlint_config_for_lang(
  lang_config: &ResolvedLangConfig,
) -> MarkdownlintConfig {
  // MD033/no-inline-html is a house-style rule, not a correctness one —
  // there is no markdown equivalent for centered badge blocks
  // (`<p align="center">` + `<img>`) or `<details>`/`<summary>` disclosure
  // widgets, so it ships disabled by default. Opt back in via
  // `[lang.markdown] no_inline_html = true` in `formality.toml` (see
  // `MarkdownOptions::no_inline_html`, issue #120).
  let no_inline_html = lang_config
    .markdown
    .as_ref()
    .and_then(|m| m.no_inline_html)
    .unwrap_or(false);

  MarkdownlintConfig {
    comment: MarkdownlintComment {
      description: AUTO_GENERATED_JSON_COMMENT.to_string(),
    },
    default: true,
    md007: MarkdownlintMd007 {
      indent: lang_config.indent_size,
    },
    md013: MarkdownlintMd013 {
      line_length: lang_config.line_length,
      code_blocks: false,
      tables: false,
    },
    md033: no_inline_html,
  }
}

/// Builds argument vector for markdownlint-cli2 invocation. `config_path`,
/// when given, is passed as `--config <path>` ahead of the file list so it
/// takes effect for both the `--fix` pass and the plain lint pass — the only
/// way to hand markdownlint-cli2 formality.toml's resolved settings, since
/// unlike prettier/rustfmt it has no per-flag inline config mechanism (see
/// [`write_markdownlint_temp_config`]).
#[must_use]
pub fn build_markdownlint_args(
  files: &[PathBuf],
  fix: bool,
  config_path: Option<&Path>,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = Vec::new();
  if fix {
    args.push("--fix".to_string());
  }
  if let Some(path) = config_path {
    args.push("--config".to_string());
    args.push(path.to_string_lossy().to_string());
  }
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds the `markdownlint-cli2 --fix` argv for both of
/// [`MarkdownSurface::format`]'s branches — the `--check` temp-copy pass and
/// the in-place write pass, which differ only in *which* paths they hand the
/// tool.
///
/// It exists so the argv is derived from the [`ExecutionContext`] in exactly
/// one place. Issue #150 was caused by the opposite arrangement: `lint()`
/// went through [`build_markdownlint_args`] and forwarded
/// `[lang.markdown] extra_args` for free, while *both* `format()` branches
/// hand-assembled a byte-identical argv that happened to omit `extra_args`
/// entirely — so the same list applied under `fml lint` and silently vanished
/// under `fml fmt`. Patching the two hand-rolled copies would have fixed that
/// instance and left in place the divergence that produced it.
///
/// `ResolvedLangConfig::extra_args` is one flat per-surface list with no
/// per-tool split, so the same list also reaches the `prettier --write` pass —
/// the convention `PythonSurface::format()` already sets for its own two-pass
/// pipeline (`ruff check --select I --fix`, then `ruff format`). Markdown is
/// where that convention bites hardest, because its two tools are separate
/// binaries with disjoint flag vocabularies. Reproduced against
/// `markdownlint-cli2 v0.23.2 (markdownlint v0.41.1)`, what a one-tool-only
/// flag actually does here is **not** a loud failure:
///
/// - A flag markdownlint-cli2 doesn't know is **consumed as a glob**, not
///   rejected — `--fix --config c.json a.md --prose-wrap always` prints
///   `Finding: a.md --prose-wrap always` and lints normally. A prettier-only
///   `extra_args` entry is therefore silently swallowed here, which is also
///   why routing `extra_args` into this pass does not regress projects that
///   already carry prettier-only flags.
/// - `--config` is a flag **both** tools accept, and `extra_args` lands after
///   the injected temp config (markdownlint honours the *last* `--config`;
///   see `test_build_markdownlint_args_extra_args_config_wins_last`). A
///   prettier `--config` therefore silently *overrides* the resolved
///   `formality.toml` markdownlint settings on the `fml fmt` path — MD013
///   falls back to markdownlint's own default 80 rather than the configured
///   value, exit 1, which `classify_exit_one_as_violation` treats as
///   "violations remain, prettier still runs", so `fml fmt` still reports
///   `[PASS]`. Silent misconfiguration, not an attributable error. `lint()`
///   has always behaved this way; this makes `fmt` consistent with it rather
///   than inventing the behaviour. Documented in
///   `docs/language-surfaces.md`; the per-tool `extra_args` split that would
///   actually fix it is new config surface, tracked in #210.
/// - The loud `ExecutionError` case is only a flag markdownlint *recognises
///   and rejects* — e.g. `--config` naming a path that does not exist, which
///   exits 2 and is surfaced by the #113 guard.
fn build_markdownlint_fix_argv(
  files: &[PathBuf],
  config_path: Option<&Path>,
  ctx: &ExecutionContext,
) -> Vec<String> {
  build_markdownlint_args(files, true, config_path, &ctx.lang_config.extra_args)
}

/// Renders the resolved [`MarkdownlintConfig`] to a throwaway temp file and
/// returns the guard holding it, so `fml fmt`/`fml lint`/`fml lsp` can pass
/// `--config <temp-path>` to markdownlint-cli2 without ever writing
/// `.markdownlint.json` into the project tree. markdownlint-cli2 only
/// accepts a config *path* (no per-flag inline settings like prettier or
/// rustfmt get), so a temp file is the only way to hand it formality.toml's
/// resolved settings inline. **A discovered `.markdownlint.json` in the
/// linted file's own directory still takes precedence over this
/// `--config`** — markdownlint-cli2 treats `--config` as a default, not an
/// override, so formality.toml only wins here when no such file exists on
/// disk (true for this repo post-#1, and the common case for any repo that
/// dropped its native config files). The returned
/// [`tempfile::NamedTempFile`] is named with a `.markdownlint-` prefix
/// (never the bare `.markdownlint.json` name) deliberately — that keeps it
/// from being auto-discovered by markdownlint-cli2 itself for the scratch
/// copies `diff_check_via_tempcopy` places in the same tmpdir; don't
/// "simplify" the prefix away. It must be kept alive for the duration of
/// the command it's passed to — it is deleted from disk when dropped,
/// which also guarantees cleanup on an early-return/error path. Only `fml
/// sync` writes the persistent `.markdownlint.json` now (see
/// [`MarkdownSurface::sync_config`]).
pub(crate) fn write_markdownlint_temp_config(
  lang_config: &ResolvedLangConfig,
) -> std::io::Result<tempfile::NamedTempFile> {
  use std::io::Write;

  let cfg = markdownlint_config_for_lang(lang_config);
  let content = cfg
    .render()
    .map_err(|e| std::io::Error::other(e.to_string()))?;

  let mut file = tempfile::Builder::new()
    .prefix(".markdownlint-")
    .suffix(".json")
    .tempfile()?;
  file.write_all(content.as_bytes())?;
  file.flush()?;
  Ok(file)
}

/// Builds argument vector for prettier format invocation.
#[must_use]
pub fn build_prettier_fmt_args(
  files: &[PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["--write".to_string()];
  for f in files {
    args.push(f.to_string_lossy().to_string());
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Whether a finished markdownlint-cli2 `--fix` invocation *failed to run*, as
/// opposed to running fine and merely finding violations it has no autofix
/// for. markdownlint-cli2 exits `0` when the file is clean or `--fix`
/// resolved everything, `1` when unfixable violations remain (MD001
/// heading-increment, MD041 first-line-heading, and the rest with no
/// autofixer), and any other non-zero code — plus an outright failure to
/// spawn the process at all — on an operational error (an unresolvable
/// `--config` path exits `2`, an internal crash likewise). Only that last
/// group is a problem `format()` must surface: exit `1` is expected and
/// prettier still takes the next pass.
///
/// This is the classification issue #113 turns on. The `--fix` pass result
/// used to be `let _ = …output()`-discarded in both `format()` branches, so a
/// markdownlint half that silently did nothing was masked by prettier's own
/// success and `fml fmt` reported `[PASS] Clean / Formatted`. The write
/// branch reaches the same verdict through [`run_tool_command_classified`] +
/// [`classify_exit_one_as_violation`]; this predicate is the form the
/// `check_only` closure needs, where the raw [`std::process::Output`] is what
/// flows on to [`diff_check_via_tempcopy_classified`].
#[must_use]
fn markdownlint_fix_pass_failed(
  outcome: &std::io::Result<std::process::Output>,
) -> bool {
  match outcome {
    Err(_) => true,
    Ok(output) => !output.status.success() && output.status.code() != Some(1),
  }
}

/// markdownlint config enabling only the two rules whose fixers turn a line
/// starting with an unspaced `#` into an ATX heading: MD018
/// (no-missing-space-atx, `#x`, `##x`) and MD020 (no-missing-space-closed-atx,
/// `#x#`, e.g. prose ending in `C#`).
const HASH_RULES_CONFIG: &str =
  "{\"default\": false, \"MD018\": true, \"MD020\": true}\n";

/// Writes [`HASH_RULES_CONFIG`] to a temp file for
/// [`escape_continuation_hashes`]'s `--config`.
///
/// The `.markdownlint-` prefix keeps markdownlint-cli2 from discovering it,
/// as in [`write_markdownlint_temp_config`]. The file is deleted on drop.
///
/// # Errors
///
/// Returns an error when the temp file cannot be created or written.
fn write_hash_rules_temp_config() -> std::io::Result<tempfile::NamedTempFile> {
  use std::io::Write;

  let mut file = tempfile::Builder::new()
    .prefix(".markdownlint-")
    .suffix(".json")
    .tempfile()?;
  file.write_all(HASH_RULES_CONFIG.as_bytes())?;
  file.flush()?;
  Ok(file)
}

/// Whether `line` starts with a run of `#` followed by a character other
/// than space or tab: markdownlint's own MD018 test (`/^#+[^# \t]/`), which
/// also covers every MD020 finding that is not already a heading.
///
/// A real heading such as `# Title#` (an MD020 finding) fails this test, so
/// it is never escaped.
#[must_use]
fn is_unspaced_hash_line(line: &str) -> bool {
  line.starts_with('#')
    && line
      .trim_start_matches('#')
      .chars()
      .next()
      .is_some_and(|c| c != ' ' && c != '\t' && c != '\r' && c != '\n')
}

/// Whether any line outside a fenced code block passes
/// [`is_unspaced_hash_line`].
///
/// A cheap filter in front of [`escape_continuation_hashes`]'s markdownlint
/// spawn: a file with no such line cannot hold an escapable finding, so the
/// common case costs a read and no process. A shebang or `#include` inside a
/// code block does not count.
fn has_unspaced_hash_line(content: &str) -> bool {
  let mut fence = None;
  content
    .lines()
    .any(|line| !in_fence(&mut fence, line) && is_unspaced_hash_line(line))
}

/// Splits `line` into its leading indent width in columns, advancing from
/// start column `width` (a tab advances to the next multiple of 4,
/// `CommonMark`'s tab stop), and the text after it.
fn split_indent_at(mut width: usize, line: &str) -> (usize, &str) {
  for (i, c) in line.char_indices() {
    match c {
      ' ' => width += 1,
      '\t' => width += 4 - width % 4,
      _ => return (width, &line[i..]),
    }
  }
  (width, "")
}

/// Splits `line` into its leading indent width in columns (a tab advances to
/// the next multiple of 4, `CommonMark`'s tab stop) and the text after it.
fn split_indent(line: &str) -> (usize, &str) {
  split_indent_at(0, line)
}

/// Advances the fenced-code state `fence` (marker character and run length
/// of the open fence) past `line`, returning whether `line` is a fence
/// marker or code inside a fence rather than Markdown content.
///
/// `CommonMark` rules: a fence is three or more backticks or tildes behind at
/// most three columns of indent (four make it indented code, or paragraph
/// text), a backtick opener's info string holds no backtick (else it is an
/// inline code span), and the closer uses the opener's character, is at
/// least as long, and carries nothing else.
fn in_fence(fence: &mut Option<(char, usize)>, line: &str) -> bool {
  let (indent, rest) = split_indent(line);
  let marker = rest.chars().next().filter(|c| matches!(c, '`' | '~'));
  let run = marker.map_or(0, |c| rest.chars().take_while(|&x| x == c).count());
  let tail = &rest[run..];
  match (*fence, marker) {
    (None, Some(c))
      if indent <= 3 && run >= 3 && (c == '~' || !tail.contains('`')) =>
    {
      *fence = Some((c, run));
      true
    }
    (None, _) => false,
    (Some((open, len)), Some(c))
      if indent <= 3 && c == open && run >= len && tail.trim().is_empty() =>
    {
      *fence = None;
      true
    }
    (Some(_), _) => true,
  }
}

/// The 1-based line numbers of every MD018 or MD020 finding in markdownlint
/// output for content read from stdin.
///
/// Checked against markdownlint-cli2 v0.23.2 (markdownlint v0.41.1) and
/// markdownlint-cli v0.49.1, which both print a stdin finding as
/// `stdin:4:1 error MD018/no-missing-space-atx …` (the column is optional
/// for some rules). Because the input came from stdin, the name before the
/// line number is always `stdin`, never a path that could hold spaces or a
/// drive-letter colon.
fn hash_rule_lines(output: &str) -> Vec<usize> {
  output
    .lines()
    .filter(|l| l.contains(" MD018/") || l.contains(" MD020/"))
    .filter_map(|l| {
      let rest = l.trim_start().strip_prefix("stdin:")?;
      let end = rest.find(|c: char| !c.is_ascii_digit())?;
      rest[..end].parse().ok()
    })
    .collect()
}

/// The number of leading lines markdownlint strips as front matter: its
/// default `frontMatterRe` (markdownlint v0.41.1 `helpers/helpers.cjs`),
/// which opens on line 1 with `---`, `+++` or `{` and closes with `---`,
/// `+++` or `...`, or `}` respectively. Zero when there is none.
fn front_matter_lines(content: &str) -> usize {
  let mut lines = content.lines();
  let closers: &[&str] =
    match lines.next().map(|l| l.trim_end_matches([' ', '\t'])) {
      Some("---") => &["---"],
      Some("+++") => &["+++", "..."],
      Some("{") => &["}"],
      _ => return 0,
    };
  lines
    .position(|l| closers.contains(&l.trim_end()))
    .map_or(0, |close| close + 2)
}

/// The end marker of the `CommonMark` HTML block (types 1-5) that `rest`, a
/// line with its indent stripped, opens: `<script>`/`<pre>`/`<style>`/
/// `<textarea>`, a comment, a processing instruction, a declaration or
/// CDATA.
///
/// Types 6 and 7 (`<div>`, any lone tag) end at a blank line instead; no
/// MD018/MD020 finding can sit inside one, so they are not tracked.
fn html_block_end(rest: &str) -> Option<&'static str> {
  const RAW: [(&str, &str); 4] = [
    ("script", "</script>"),
    ("pre", "</pre>"),
    ("style", "</style>"),
    ("textarea", "</textarea>"),
  ];
  let tail = rest.strip_prefix('<')?;
  for (open, end) in [("!--", "-->"), ("![CDATA[", "]]>"), ("?", "?>")] {
    if tail.starts_with(open) {
      return Some(end);
    }
  }
  if tail
    .strip_prefix('!')
    .is_some_and(|t| t.starts_with(|c: char| c.is_ascii_alphabetic()))
  {
    return Some(">");
  }
  RAW.iter().find_map(|&(tag, end)| {
    let head = tail.get(..tag.len())?;
    let next = tail[tag.len()..].chars().next();
    (head.eq_ignore_ascii_case(tag)
      && next.is_none_or(|c| c.is_ascii_whitespace() || c == '>'))
    .then_some(end)
  })
}

/// Whether `hay` contains `needle`, ignoring ASCII case, as `CommonMark`'s
/// HTML block end conditions do.
fn contains_ignore_case(hay: &str, needle: &str) -> bool {
  hay
    .as_bytes()
    .windows(needle.len())
    .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// `CommonMark` block state across lines, just enough to tell whether a line
/// opens a block of its own or continues an open paragraph.
#[derive(Default)]
struct BlockScan {
  /// The previous line left a paragraph open, so a following line that
  /// starts no other block is a continuation of it.
  in_para: bool,
  /// The content column of each open list item, innermost last: a line
  /// indented that far belongs to the item, four more make indented code.
  items: Vec<usize>,
  /// The open paragraph sits in a blockquote, so a line without `>` can
  /// only continue it lazily.
  quoted: bool,
  /// The open code fence, as [`in_fence`] tracks it.
  fence: Option<(char, usize)>,
  /// The end marker of an open HTML block, from [`html_block_end`].
  html_end: Option<&'static str>,
}

impl BlockScan {
  /// Advances past `line`, returning whether an unspaced `#` line there
  /// would open a block of its own: a heading the author forgot to space,
  /// which markdownlint's MD018/MD020 fixers may space into `# Title`.
  ///
  /// It does so at the start of the content or after a blank line, an ATX
  /// heading, a setext underline, a thematic break, a closing fence,
  /// indented code, or the end of an HTML block. Everything else that is not
  /// blank leaves a paragraph open, so a `#` line after it is prose: plain
  /// paragraph text, or a lazy continuation of a list item, blockquote or
  /// GFM table row (prettier 3.9.6 turns `| 1 |⏎#2` into `| 1 |⏎| #2 |`).
  /// Inside a list item, indented code starts four columns past the item's
  /// content column; less is a paragraph of the item (`- i⏎⏎    more`).
  /// A line inside a fence or HTML block also returns `false`: markdownlint
  /// never reports one there, so a finding means this scan lost track, and
  /// escaping is the outcome that renders the same either way.
  fn opens_block(&mut self, line: &str) -> bool {
    let line = line.trim_end_matches(['\n', '\r']);
    let opens = !self.in_para;
    self.in_para = false;
    // An open HTML block goes first: a fence marker inside one is HTML.
    if let Some(end) = self.html_end {
      if contains_ignore_case(line, end) {
        self.html_end = None;
      }
      return false;
    }
    if in_fence(&mut self.fence, line) {
      return false;
    }
    let (indent, rest) = split_indent(line);
    if rest.is_empty() {
      return false;
    }
    if opens {
      // Not a lazy continuation, so it closes the items it is not inside.
      self.items.retain(|&col| col <= indent);
    }
    if indent >= self.items.last().map_or(0, |&col| col) + 4 {
      // Indented code unless it continues a paragraph; never a `#` line.
      self.in_para = !opens;
      return false;
    }
    if let Some(end) = html_block_end(rest) {
      if !contains_ignore_case(&rest[1..], end) {
        self.html_end = Some(end);
      }
      return false;
    }
    // A setext underline is never lazy: outside the paragraph's blockquote
    // or list item, `===` is paragraph text (`> q⏎===` is one paragraph).
    let lazy = (self.quoted && !rest.starts_with('>'))
      || self.items.last().is_some_and(|&col| indent < col);
    let setext = !opens
      && !lazy
      && (rest.trim_end().bytes().all(|b| b == b'=')
        || rest.trim_end().bytes().all(|b| b == b'-'));
    let breaks = setext || is_thematic_break(rest);
    let marker = list_marker(indent, rest).filter(|_| !breaks);
    if let Some(width) = marker {
      self.items.retain(|&col| col <= indent);
      self.items.push(indent + width);
    }
    let content = marker.map_or(rest, |_| {
      rest.trim_start_matches(|c: char| {
        c.is_ascii_digit() || "-*+.) \t".contains(c)
      })
    });
    self.quoted = content.starts_with('>') || (!opens && self.quoted);
    self.in_para = !(is_atx_heading(rest) || breaks);
    opens
  }
}

/// The width of the list item marker that starts `rest`, a line with its
/// indent stripped, plus the one to four spaces after it: how far the item's
/// content is indented. A bullet is `-`, `*` or `+`; an ordered marker is one
/// to nine digits and `.` or `)`. Either needs a space, a tab or the end of
/// the line after it.
fn list_marker(indent: usize, rest: &str) -> Option<usize> {
  let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
  let mark = match rest.as_bytes().get(digits) {
    Some(b'-' | b'*' | b'+') if digits == 0 => 1,
    Some(b'.' | b')') if (1..=9).contains(&digits) => digits + 1,
    _ => return None,
  };
  let (end, text) = split_indent_at(indent + mark, &rest[mark..]);
  let gap = end - indent - mark;
  match gap {
    0 if !text.is_empty() => None,
    1..=4 if !text.is_empty() => Some(mark + gap),
    _ => Some(mark + 1),
  }
}

/// Whether `rest`, a line with its indent stripped, is an ATX heading: one
/// to six `#` followed by a space, a tab or the end of the line.
#[must_use]
fn is_atx_heading(rest: &str) -> bool {
  let hashes = rest.bytes().take_while(|&b| b == b'#').count();
  (1..=6).contains(&hashes)
    && matches!(rest.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// Whether `rest`, a line with its indent stripped, is a thematic break:
/// three or more of one of `*`, `-`, `_`, with only spaces or tabs between.
#[must_use]
fn is_thematic_break(rest: &str) -> bool {
  let Some(mark) = rest.chars().next().filter(|c| matches!(c, '*' | '-' | '_'))
  else {
    return false;
  };
  let marks = rest.chars().filter(|&c| c == mark).count();
  marks >= 3 && rest.chars().all(|c| c == mark || c == ' ' || c == '\t')
}

/// Prefixes the leading `#` of each listed 1-based line with a backslash,
/// when that line passes [`is_unspaced_hash_line`] and continues a paragraph
/// ([`BlockScan::opens_block`] is `false`).
///
/// `\#` renders exactly as the bare `#` of such a line, so escaping never
/// changes the rendered document. A finding that opens a block of its own
/// (`#Title` after a blank line) also renders as text, but is far more
/// likely a heading the author forgot to space, so it is left for
/// markdownlint's MD018/MD020 fixers to make `# Title` (#413). Prettier
/// 3.9.6 keeps the escape: `prettier --parser markdown --prose-wrap always`
/// leaves `\#299) ok.` at a line start unchanged, and `short \#tag mid and
/// \# alone.` keeps both escapes mid-line, so the result is stable across
/// runs.
fn escape_line_hashes(content: &str, lines: &[usize]) -> String {
  let mut out = String::with_capacity(content.len() + lines.len());
  let skip = front_matter_lines(content);
  let mut scan = BlockScan::default();
  for (idx, line) in content.split_inclusive('\n').enumerate() {
    let opens_block = idx >= skip && scan.opens_block(line);
    if !opens_block && is_unspaced_hash_line(line) && lines.contains(&(idx + 1))
    {
      out.push('\\');
    }
    out.push_str(line);
  }
  out
}

/// Escapes every `#` that starts a paragraph continuation line in `file`,
/// before markdownlint's MD018/MD020 fixers can see it (#314, #413).
///
/// A line inside a paragraph can start with a token such as `#299)`, or
/// `#299 for C#`: prettier's prose wrap breaks right before one (#314), or
/// the author wrapped the line by hand (#413). It renders as text, but
/// MD018 or MD020 reports it, and `markdownlint --fix` inserts a space,
/// turning prose into a heading (#309). `fml fmt` therefore runs this step
/// both before its own `markdownlint --fix` pass and after `prettier
/// --write`, and `fml fix` runs it before its lint fix pass. Reproduced with
/// markdownlint-cli2 v0.23.2 (markdownlint v0.41.1) and prettier 3.9.6 at
/// `line_length = 80`: without the first run, one `fml fmt` turns
/// `see⏎#299) for details.` into the H1 `# 299) for details`.
///
/// Only continuation lines are escaped; see [`escape_line_hashes`] for why
/// a `#Title` that opens its own block is left to the fixers.
///
/// markdownlint, limited to [`HASH_RULES_CONFIG`], picks the lines: MD018
/// and MD020 skip fenced and indented code, HTML blocks and front matter, so
/// those are classified by the same parser that would have "fixed" them.
/// The content goes in on stdin, which markdownlint-cli2 never rewrites,
/// even under a discovered `.markdownlint-cli2.jsonc` with `"fix": true`.
///
/// # Errors
///
/// Returns an error when `file` cannot be read or rewritten, or markdownlint
/// fails to run (any exit other than 0 or 1).
///
/// # Side Effects
///
/// Rewrites `file` in place when it holds a finding, and spawns `bin` only
/// for a file that passes [`has_unspaced_hash_line`].
fn escape_continuation_hashes(
  bin: &str,
  config: &Path,
  file: &Path,
  root: &Path,
) -> std::io::Result<()> {
  use std::io::Write;

  let content = std::fs::read_to_string(file)?;
  if !has_unspaced_hash_line(&content) {
    return Ok(());
  }

  // markdownlint-cli2 reads stdin from a `-` argument; the legacy
  // markdownlint CLI needs `--stdin`.
  let stdin_arg = if bin == "markdownlint" {
    "--stdin"
  } else {
    "-"
  };
  let mut child = create_tool_command(bin)
    .arg("--config")
    .arg(config)
    .arg(stdin_arg)
    .current_dir(root)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
  // Both CLIs print at most a one-line banner before reading all of stdin,
  // so writing it in full before collecting the output cannot deadlock.
  if let Some(mut stdin) = child.stdin.take() {
    stdin.write_all(content.as_bytes())?;
  }
  let outcome = child.wait_with_output();
  if markdownlint_fix_pass_failed(&outcome) {
    return Err(outcome.err().unwrap_or_else(|| {
      std::io::Error::other(format!("{bin} failed while checking MD018/MD020"))
    }));
  }
  let output = outcome?;

  let mut findings = String::from_utf8_lossy(&output.stdout).into_owned();
  findings.push_str(&String::from_utf8_lossy(&output.stderr));
  let lines = hash_rule_lines(&findings);
  if lines.is_empty() {
    return Ok(());
  }
  std::fs::write(file, escape_line_hashes(&content, &lines))
}

/// Runs [`escape_continuation_hashes`] over `files` in parallel: each
/// candidate file costs one markdownlint spawn.
///
/// # Errors
///
/// Returns the first error [`escape_continuation_hashes`] hits.
fn escape_all(
  bin: &str,
  config: &Path,
  files: &[PathBuf],
  root: &Path,
) -> std::io::Result<()> {
  files
    .par_iter()
    .try_for_each(|f| escape_continuation_hashes(bin, config, f, root))
}

/// The `ExecutionError` result for a failed [`escape_all`].
fn escape_failed(
  surface_name: &'static str,
  start: Instant,
  e: &std::io::Error,
) -> SurfaceResult {
  SurfaceResult {
    surface_name,
    status: SurfaceStatus::ExecutionError {
      message: format!("Failed to escape a paragraph-continuation `#`: {e}"),
    },
    duration: start.elapsed(),
  }
}

/// markdownlint-cli2 line prefixes that carry only progress chatter, never a
/// per-violation finding. It has no `--quiet` flag and `noBanner: true`
/// suppresses only the first of these (verified against v0.23.2), so `fml`
/// filters them out itself. Anchored to the exact known prefixes — an
/// unrecognized stdout line from a future markdownlint-cli2 must still reach
/// the user rather than being swallowed. `Summary:` is deliberately *not*
/// here: [`filter_markdownlint_noise`] keeps it as a tail count.
const MARKDOWNLINT_NOISE_PREFIXES: &[&str] =
  &["markdownlint-cli2 v", "Finding: ", "Linting: "];

/// Strips markdownlint-cli2's banner/progress lines from a captured
/// diagnostic `message`, leaving the surviving violation lines untouched
/// (including whatever absolute paths markdownlint-cli2 chose to print).
///
/// Path relativization is deliberately **not** done here any more (#157):
/// it used to be, via a local shim, but the runner already relativizes every
/// surface's diagnostics once, centrally, via
/// [`crate::ui::paths::relativize_text`] before printing them — doing it here
/// too meant markdown's diagnostics were relativized twice on their way to
/// the screen. `relativize_text` recognizes a line whose leading token is an
/// absolute path under the run root (the exact shape a markdownlint-cli2
/// violation line has when it falls back to one), so the single downstream
/// pass covers this surface's output correctly without a second, surface-
/// local pass.
///
/// The real per-violation lines land on stderr (see #107's shared capture)
/// and reach `message` after a blank line and a bare `stderr:` label courtesy
/// of `merge_tool_streams`; the version banner, the `Finding:` echo of the
/// absolute input paths `fml` itself just passed in, and the `Linting: N
/// files` count all land on stdout. This drops those three (anchored to
/// [`MARKDOWNLINT_NOISE_PREFIXES`]) plus the now-content-free `stderr:`
/// label, moves the `Summary:` line to the tail as a bare count, and leaves
/// any other line — including an unrecognized one from a newer
/// markdownlint-cli2 — untouched.
fn filter_markdownlint_noise(message: &str) -> String {
  let mut kept: Vec<String> = Vec::new();
  let mut summary: Option<String> = None;

  for line in message.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "stderr:" {
      continue;
    }
    if MARKDOWNLINT_NOISE_PREFIXES
      .iter()
      .any(|p| trimmed.starts_with(p))
    {
      continue;
    }
    if let Some(rest) = trimmed.strip_prefix("Summary: ") {
      summary = Some(rest.to_string());
      continue;
    }
    kept.push(trimmed.to_string());
  }

  if let Some(s) = summary {
    kept.push(s);
  }
  kept.join("\n")
}

// --- Block-level embedded HTML formatting (Fixes #253) ---
//
// #120 shipped MD033/no-inline-html disabled by default because idioms like
// a centered badge block (`<p align="center">` + `<img>`) or a `<details>`/
// `<summary>` disclosure widget have no markdown equivalent. The owner's
// condition for being comfortable with that: embedded HTML should still get
// *formatted*, not left as a permanent escape hatch from `fml fmt`.
//
// Verified against prettier 3.9.6 (see the issue's decision comment and this
// PR's body): prettier's own `--parser markdown` treats a block-level HTML
// node as an opaque string — confirmed by feeding it
// `<img src="a.png"     alt="badge">` inside a `<p align="center">` wrapper
// and seeing the quadruple space survive byte-for-byte. There is no prettier
// option that changes this (`--embedded-language-formatting` only concerns
// code embedded in fenced blocks / template literals; `--html-whitespace-
// sensitivity` only changes how the **html** parser itself treats
// whitespace, it does not make the markdown parser reach for that parser in
// the first place). So this is the extract-and-splice pass the issue asks
// for, not a one-line config change.
//
// Deliberately **not** reached for inline HTML spans mid-paragraph (a
// `<strong>`/`<a>`/`<code>` sitting inside a sentence) — the owner's
// decision draws that line because whitespace around an inline element is
// rendering-significant and nobody would notice a silent change until they
// looked at the rendered page. `pulldown-cmark` already draws exactly this
// distinction in its own event stream: a block-level HTML node arrives as
// `Event::Start(Tag::HtmlBlock)` / `Event::End(TagEnd::HtmlBlock)`, while an
// inline span arrives as `Event::InlineHtml` — the two are never conflated,
// so only the former is ever collected below.

/// HTML void elements (per the WHATWG HTML spec) that never need a matching
/// closing tag. Used by [`scan_html_tags`] so `<img>`, `<br>`, etc. don't
/// throw off tag-balance tracking across block-level HTML nodes.
const VOID_ELEMENTS: &[&str] = &[
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
  "param", "source", "track", "wbr",
];

/// One HTML tag found by [`scan_html_tags`], reduced to just what
/// [`group_html_blocks`] needs to track: its (lowercased) name, whether it is
/// a closing tag, and whether it self-closes (`<br/>`, or any void element).
struct HtmlTagToken {
  name: String,
  closing: bool,
  self_closing: bool,
}

/// Tokenizes the tags in a block-level HTML fragment for tag-balance
/// tracking, returning `None` when the fragment can't be tokenized with
/// confidence (an unterminated tag or comment) rather than guessing.
///
/// This is a small hand-rolled scanner, not a full HTML parser: it tracks
/// quoted attribute values (so a stray `>` inside `alt="a > b"` doesn't end
/// the tag early) and skips comments and `<!DOCTYPE>`/`<?...?>` declarations,
/// but doesn't understand raw-text elements (`<script>`, `<style>`) or
/// anything past what ordinary README idioms need. Given how narrow the
/// scope is (badge wrappers, `<details>`/`<summary>`, `<div>` wrappers), that
/// trade-off buys a lot of simplicity for very little real coverage lost —
/// and [`group_html_blocks`] bails out (leaves the whole document's block
/// HTML untouched) rather than guess when this returns `None`.
fn scan_html_tags(s: &str) -> Option<Vec<HtmlTagToken>> {
  let bytes = s.as_bytes();
  let n = bytes.len();
  let mut i = 0usize;
  let mut tokens = Vec::new();
  while i < n {
    if bytes[i] != b'<' {
      i += 1;
      continue;
    }
    if s[i..].starts_with("<!--") {
      let end_rel = s[i..].find("-->")?;
      i += end_rel + 3;
      continue;
    }
    if i + 1 < n && (bytes[i + 1] == b'!' || bytes[i + 1] == b'?') {
      let end_rel = s[i..].find('>')?;
      i += end_rel + 1;
      continue;
    }
    let closing = i + 1 < n && bytes[i + 1] == b'/';
    let name_start = if closing { i + 2 } else { i + 1 };
    if name_start >= n || !bytes[name_start].is_ascii_alphabetic() {
      // A bare `<` that isn't actually a tag start (stray comparison text,
      // malformed input) -- not a tag, keep scanning.
      i += 1;
      continue;
    }
    let mut j = name_start;
    while j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
      j += 1;
    }
    let name = s[name_start..j].to_ascii_lowercase();
    let mut k = j;
    let mut quote: Option<u8> = None;
    let self_closing;
    loop {
      if k >= n {
        return None;
      }
      let c = bytes[k];
      if let Some(q) = quote {
        if c == q {
          quote = None;
        }
      } else if c == b'"' || c == b'\'' {
        quote = Some(c);
      } else if c == b'>' {
        let mut p = k;
        while p > j && (bytes[p - 1] as char).is_whitespace() {
          p -= 1;
        }
        self_closing = p > j && bytes[p - 1] == b'/';
        break;
      }
      k += 1;
    }
    tokens.push(HtmlTagToken {
      name,
      closing,
      self_closing,
    });
    i = k + 1;
  }
  Some(tokens)
}

/// Parses `src` with the GFM extensions prettier's markdown parser honours.
///
/// These extensions don't change how HTML blocks are delimited (that's pure
/// `CommonMark`), but they do change how *other* content parses, which
/// matters for not mis-detecting a block boundary inside, say, a GFM table
/// living next to embedded HTML.
fn parse_markdown(src: &str) -> Parser<'_> {
  let opts = Options::ENABLE_TABLES
    | Options::ENABLE_STRIKETHROUGH
    | Options::ENABLE_FOOTNOTES
    | Options::ENABLE_TASKLISTS;
  Parser::new_ext(src, opts)
}

/// Byte ranges of every block-level HTML node in `src`, in document order —
/// exactly the [`Event::Start(Tag::HtmlBlock)`] spans `pulldown-cmark`
/// emits, which already cover the *whole* node (start of the opening `<` to
/// past the node's last byte), never an inline HTML span embedded in a
/// paragraph.
///
/// Only top-level blocks count. A block nested in any container (blockquote,
/// list item, footnote) carries that container's `> ` prefixes or indent on
/// every line; prettier's html parser would read the prefixes as text and
/// re-indent the lines out of the container, so such blocks stay as written.
fn extract_html_block_ranges(src: &str) -> Vec<(usize, usize)> {
  let mut spans = Vec::new();
  let mut depth = 0usize;
  // Where the current top-level block's next `Html` chunk must start;
  // `None` once a chunk is not a verbatim slice of `src`. Defence in depth
  // for the container rule: prettier must see exactly the block's HTML.
  let mut next: Option<usize> = None;
  // `<!-- prettier-ignore -->` shields the next top-level block, whatever
  // its kind; `-start`/`-end` shield every block between them.
  let (mut ignore_next, mut ignore_range) = (false, false);
  for (event, range) in parse_markdown(src).into_offset_iter() {
    match event {
      Event::Start(tag) => {
        if depth == 0 {
          let block = &src[range.clone()];
          // Comment-only blocks have nothing to tidy; skipping them saves a
          // Node spawn per `markdownlint-disable` comment.
          let mut skip = std::mem::take(&mut ignore_next)
            || ignore_range
            || (tag == Tag::HtmlBlock && is_comment_only(block));
          if tag == Tag::HtmlBlock
            && let Some((directive, rest)) = leading_comment(block)
            && directive.starts_with("prettier-ignore")
          {
            match directive {
              "prettier-ignore" => ignore_next = rest.trim().is_empty(),
              "prettier-ignore-start" => ignore_range = true,
              "prettier-ignore-end" => ignore_range = false,
              _ => {}
            }
            skip = true;
          }
          if tag == Tag::HtmlBlock && !skip {
            next = Some(range.start);
          }
        }
        depth += 1;
      }
      Event::Rule if depth == 0 => ignore_next = false,
      Event::Html(text) if depth == 1 => {
        next = next
          .filter(|&at| src[at..].starts_with(&*text))
          .map(|at| at + text.len());
      }
      Event::End(TagEnd::HtmlBlock) if depth == 1 => {
        depth -= 1;
        if next.take() == Some(range.end) {
          spans.push((range.start, range.end));
        }
      }
      Event::End(_) => depth -= 1,
      _ => {}
    }
  }
  spans
}

/// Splits `block` into its leading HTML comment's trimmed text and the
/// text after that comment, or `None` when `block` does not open with one.
fn leading_comment(block: &str) -> Option<(&str, &str)> {
  let rest = block.trim_start().strip_prefix("<!--")?;
  let end = rest.find("-->")?;
  Some((rest[..end].trim(), &rest[end + 3..]))
}

/// Reports whether `block` holds nothing but HTML comments and whitespace.
#[must_use]
fn is_comment_only(mut block: &str) -> bool {
  while let Some((_, rest)) = leading_comment(block) {
    block = rest;
  }
  block.trim().is_empty()
}

/// Groups `spans` (as returned by [`extract_html_block_ranges`]) into
/// maximal runs of *tag-balanced* HTML.
///
/// `CommonMark`'s HTML-block rule ends a block at the first blank line, so a
/// `<details>`/`<summary>…</summary>` opener and its matching `</details>`
/// closer — with ordinary markdown content in between (already normalized by
/// the earlier `markdownlint --fix` + `prettier --parser markdown` passes) —
/// arrive as two, non-adjacent spans. Formatting either span in isolation
/// through prettier's **html** parser is unsafe: prettier repairs invalid
/// fragments rather than rejecting them (verified: handed just the opener,
/// it silently *inserts* a closing `</details>` that doesn't belong there),
/// and handed just the closer alone it errors out entirely.
///
/// So spans are grouped by simulating a single tag stack across all of them,
/// in document order: an opening tag not in [`VOID_ELEMENTS`] pushes, a
/// closing tag pops (and must match the stack's top), and each time the
/// stack returns to empty marks the end of one group. Returns `None` —
/// meaning "leave every block HTML node in this document untouched" — the
/// moment anything looks inconclusive: a closing tag with no matching open,
/// an unterminated tag ([`scan_html_tags`] returning `None`), or unbalanced
/// tags left open at end of document. Round-trip safety matters more here
/// than coverage: this never guesses.
fn group_html_blocks(
  src: &str,
  spans: &[(usize, usize)],
) -> Option<Vec<Vec<usize>>> {
  let mut stack: Vec<String> = Vec::new();
  let mut groups: Vec<Vec<usize>> = Vec::new();
  let mut current_group: Vec<usize> = Vec::new();

  for (idx, &(start, end)) in spans.iter().enumerate() {
    let tokens = scan_html_tags(&src[start..end])?;
    current_group.push(idx);
    for tok in tokens {
      if tok.self_closing
        || (!tok.closing && VOID_ELEMENTS.contains(&tok.name.as_str()))
      {
        continue;
      }
      if tok.closing {
        match stack.last() {
          Some(top) if *top == tok.name => {
            stack.pop();
          }
          // A closing tag with nothing matching on the stack: either
          // genuinely malformed HTML, or a shape this scanner doesn't
          // understand (a raw-text element, mismatched case, etc). Bail
          // for the whole document rather than mis-splice it.
          _ => return None,
        }
      } else {
        stack.push(tok.name);
      }
    }
    if stack.is_empty() {
      groups.push(std::mem::take(&mut current_group));
    }
  }

  // Tags left open (or a group with no closing spans at all) at EOF: same
  // "don't guess" bailout.
  if !stack.is_empty() || !current_group.is_empty() {
    return None;
  }
  Some(groups)
}

/// Runs prettier's **html** parser over a single, self-contained HTML
/// fragment via `crate::surfaces::create_tool_command` — never a bare
/// `std::process::Command::new` (#266, #103's whole class of Windows
/// `.cmd`-shim bugs came from call sites that bypassed that helper).
///
/// `--html-whitespace-sensitivity=css` is set explicitly rather than left to
/// prettier's own default (even though `css` happens to be that default) so
/// the choice is visible and doesn't silently drift if that default ever
/// changes. `css` treats whitespace around an element as significant or not
/// based on that element's *actual* CSS `display` value — insignificant for
/// a block-level `<div>`/`<details>`/`<p>` wrapper, still significant for an
/// inline element (`<a>`, `<strong>`, `<em>`) that might live inside one.
/// Verified against `<p align="center"><strong>Bold</strong><em>Italic</em></p>`:
/// `css` (and `strict`) leave it untouched; `--html-whitespace-sensitivity=
/// ignore` inserts a newline between the two, which browsers collapse
/// adjacent-tag whitespace into a rendered space — turning `BoldItalic` into
/// "Bold Italic" on the rendered page. `ignore` is exactly the setting this
/// pass must not use.
///
/// `extra_args` also reach this call (after the fixed flags) — same
/// convention `build_markdownlint_fix_argv` documents for the other two
/// passes in this surface's pipeline, so a project's own `extra_args` don't
/// silently apply to two of this surface's three tool invocations and not
/// the third.
fn run_prettier_html(
  html: &str,
  inline_config: &[String],
  extra_args: &[String],
) -> Option<String> {
  let mut cmd = create_tool_command("prettier");
  cmd
    .arg("--parser")
    .arg("html")
    .arg("--html-whitespace-sensitivity=css")
    .args(inline_config)
    .args(extra_args)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  let mut child = cmd.spawn().ok()?;
  child.stdin.take()?.write_all(html.as_bytes()).ok()?;
  let output = child.wait_with_output().ok()?;
  if !output.status.success() {
    return None;
  }
  String::from_utf8(output.stdout).ok()
}

/// Splits `formatted` (prettier html output for a multi-span group's
/// bridged virtual document — see [`format_block_html`]) back into one
/// string per original span, using the `<!--fml-html-gap-N-->` placeholder
/// lines [`format_block_html`] inserted between them as the cut points.
///
/// Returns `None` if a placeholder didn't survive formatting intact (would
/// only happen if prettier's html parser mangled an HTML comment, which it
/// doesn't in practice) — the caller falls back to leaving that group
/// untouched rather than splicing a corrupted result.
fn split_on_gap_placeholders(
  formatted: &str,
  gap_count: usize,
) -> Option<Vec<&str>> {
  let mut parts = Vec::with_capacity(gap_count + 1);
  let mut remaining = formatted;
  for gi in 0..gap_count {
    let marker = format!("<!--fml-html-gap-{gi}-->");
    let marker_pos = remaining.find(&marker)?;
    let line_start = remaining[..marker_pos].rfind('\n').map_or(0, |p| p + 1);
    let after_marker = marker_pos + marker.len();
    let line_end = remaining[after_marker..]
      .find('\n')
      .map_or(remaining.len(), |p| after_marker + p + 1);
    parts.push(&remaining[..line_start]);
    remaining = &remaining[line_end..];
  }
  parts.push(remaining);
  Some(parts)
}

/// Formats every block-level HTML node in `src`, leaving everything else —
/// every other byte, including inline HTML spans mid-paragraph — untouched.
///
/// Runs after the existing `markdownlint-cli2 --fix` + `prettier --parser
/// markdown` passes, on their output, so the markdown content nested inside
/// an HTML wrapper (e.g. a list inside `<details>`) is already in its final,
/// normalized form by the time this runs — this pass only ever touches the
/// HTML tags themselves. Idempotent by construction: re-running it against
/// already-formatted output re-detects the same block spans and re-formats
/// them to the same result, since prettier's own html formatting is
/// idempotent and this pass never touches anything else.
///
/// A group of one span (the common case — a self-contained
/// `<p align="center">…</p>` or `<div>…</div>` with no blank line inside)
/// is hand ed to [`run_prettier_html`] directly. A multi-span group (an
/// opener/closer pair split by `CommonMark`'s blank-line rule, e.g.
/// `<details>` … blank-line-separated markdown … `</details>`) is bridged
/// into one virtual document with a numbered
/// `<!--fml-html-gap-N-->` placeholder standing in for each gap, formatted
/// once, then split back apart on those placeholders — the original gap
/// text (already-formatted markdown) is re-spliced in verbatim, never
/// touched by the html parser. See [`group_html_blocks`]'s doc comment for
/// why a single combined html-parser pass over the *whole* group (interior
/// markdown included) isn't safe: it collapses a list's line breaks into
/// plain HTML text content.
///
/// Falls back to leaving a group's original text untouched whenever
/// anything is inconclusive: [`group_html_blocks`] declining to group the
/// document at all, prettier failing to format a fragment, or a
/// placeholder not surviving the round trip. A hard failure here would make
/// `fml fmt` fail on real-world HTML this pass doesn't understand yet;
/// leaving it exactly as written is always a safe, silent no-op instead.
fn format_block_html(
  src: &str,
  inline_config: &[String],
  extra_args: &[String],
) -> String {
  let spans = extract_html_block_ranges(src);
  if spans.is_empty() {
    return src.to_string();
  }
  let Some(groups) = group_html_blocks(src, &spans) else {
    return src.to_string();
  };

  let mut out = String::with_capacity(src.len());
  let mut cursor = 0usize;

  for group in groups {
    let group_start = spans[group[0]].0;
    let group_end = spans[*group.last().expect("group is never empty")].1;
    out.push_str(&src[cursor..group_start]);

    let spliced = if group.len() == 1 {
      let block_text = &src[spans[group[0]].0..spans[group[0]].1];
      run_prettier_html(block_text, inline_config, extra_args)
    } else {
      let mut virtual_doc = String::new();
      for (gi, &block_idx) in group.iter().enumerate() {
        let (bs, be) = spans[block_idx];
        virtual_doc.push_str(&src[bs..be]);
        if gi + 1 < group.len() {
          if !virtual_doc.ends_with('\n') {
            virtual_doc.push('\n');
          }
          let _ = writeln!(virtual_doc, "<!--fml-html-gap-{gi}-->");
        }
      }
      run_prettier_html(&virtual_doc, inline_config, extra_args).and_then(
        |formatted| {
          let parts = split_on_gap_placeholders(&formatted, group.len() - 1)?;
          let mut spliced = String::new();
          for (gi, &block_idx) in group.iter().enumerate() {
            spliced.push_str(parts[gi].trim_end_matches('\n'));
            spliced.push('\n');
            if gi + 1 < group.len() {
              let (_, be) = spans[block_idx];
              let (next_start, _) = spans[group[gi + 1]];
              spliced.push_str(&src[be..next_start]);
            }
          }
          Some(spliced)
        },
      )
    };

    match spliced {
      Some(text) => {
        out.push_str(text.trim_end_matches('\n'));
        out.push('\n');
      }
      // Formatting failed for this group specifically (a shape prettier's
      // html parser rejects, or a spawn failure) -- leave it exactly as
      // written rather than failing the whole `fml fmt` run over it.
      None => out.push_str(&src[group_start..group_end]),
    }
    cursor = group_end;
  }
  out.push_str(&src[cursor..]);
  // Prettier indents nested wrappers; after a blank line, four spaces turn
  // a wrapper into an indented code block. Any splice that changes how the
  // document parses is rejected wholesale.
  if block_structure(&out) == block_structure(src) {
    out
  } else {
    src.to_string()
  }
}

/// The `Start`/`End` tag sequence `pulldown-cmark` sees in `src`: equal for
/// two documents exactly when they nest the same blocks and inline spans
/// the same way, whatever the text inside an HTML block says.
fn block_structure(src: &str) -> Vec<(bool, TagEnd)> {
  parse_markdown(src)
    .filter_map(|event| match event {
      Event::Start(tag) => Some((true, tag.to_end())),
      Event::End(tag) => Some((false, tag)),
      _ => None,
    })
    .collect()
}

/// Runs [`format_block_html`] against the file at `path` and writes the
/// result back — but only when it actually changed, so a file with no block
/// HTML at all (the overwhelming majority) never gets rewritten with an
/// identical byte stream. Shared by both of [`MarkdownSurface::format`]'s
/// branches (the `--check` temp-copy pass and the real in-place write pass),
/// which differ only in *which* path they hand this.
fn apply_block_html_pass(
  path: &Path,
  inline_config: &[String],
  extra_args: &[String],
) -> std::io::Result<()> {
  let content = std::fs::read_to_string(path)?;
  let updated = format_block_html(&content, inline_config, extra_args);
  if updated != content {
    std::fs::write(path, updated)?;
  }
  Ok(())
}

/// Markdown language surface implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownSurface;

impl DeclaresFacets for MarkdownSurface {
  fn facet_support(&self, facet: Facet) -> FacetSupport {
    match facet {
      Facet::IndentTabs
      | Facet::IndentWidth
      | Facet::LineLength
      | Facet::ProseWrap => FacetSupport::Configurable,
      Facet::QuoteStyle
      | Facet::TrailingComma
      | Facet::ImportSort
      | Facet::Edition
      | Facet::Standard => FacetSupport::Unsupported,
    }
  }
}

const MD_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkdn"];

impl LanguageSurface for MarkdownSurface {
  fn name(&self) -> &'static str {
    "markdown"
  }

  fn aliases(&self) -> &[&'static str] {
    &["md"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    MD_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(*self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn marker_files(&self) -> &[&'static str] {
    &[".markdownlint.json", ".markdownlint.yaml"]
  }

  fn tool_info(&self, _config: &ResolvedLangConfig) -> Vec<ToolInfo> {
    vec![
      ToolInfo {
        binary: "prettier",
        description: "Opinionated code/markdown formatter",
        install_hint: None,
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      ToolInfo {
        binary: "markdownlint-cli2",
        description: "Fast markdown linter",
        install_hint: None,
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  // Orchestrates prettier markdown formatting across check and write modes with fallback tempcopy handling.
  #[allow(clippy::too_many_lines)]
  fn format(&self, ctx: &ExecutionContext) -> SurfaceResult {
    let start = Instant::now();

    if let Some(res) = tool_missing_guard(self.name(), "prettier", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(MD_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    let md_binary = if check_binary_exists("markdownlint-cli2") {
      Some("markdownlint-cli2")
    } else if check_binary_exists("markdownlint") {
      Some("markdownlint")
    } else {
      None
    };

    // Inline `--tab-width`/`--print-width`/etc. instead of writing
    // `.prettierrc.json` to disk — see `build_prettier_inline_args` (Fixes
    // #151 [pre-recreation]). `fml sync` remains the only path that materializes the file.
    let inline_config =
      build_prettier_inline_args(&PrettierConfig::from_context(ctx));

    // markdownlint-cli2's own `--fix` pass has no per-flag inline config
    // (it only accepts `--config <path>`), so the resolved settings are
    // rendered to a throwaway temp file and passed via `--config` instead —
    // see `write_markdownlint_temp_config`. Only created when a markdownlint
    // binary was actually found; kept alive across both the check-only and
    // write branches below so the file exists for the duration of every
    // invocation that references its path. The second file holds the
    // MD018/MD020-only config `escape_continuation_hashes` checks with (#314).
    let md_temp_cfgs = if md_binary.is_some() {
      match write_markdownlint_temp_config(&ctx.lang_config)
        .and_then(|f| Ok((f, write_hash_rules_temp_config()?)))
      {
        Ok(f) => Some(f),
        Err(e) => {
          return SurfaceResult {
            surface_name: self.name(),
            status: SurfaceStatus::ExecutionError {
              message: format!(
                "Failed to write temporary markdownlint config: {e}"
              ),
            },
            duration: start.elapsed(),
          };
        }
      }
    } else {
      None
    };
    let md_temp_cfg_path = md_temp_cfgs.as_ref().map(|(f, _)| f.path());
    let hash_cfg_path = md_temp_cfgs.as_ref().map(|(_, h)| h.path());

    if ctx.check_only {
      return diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          if let Some(bin) = md_binary {
            // #413: the same pre-fixer step as the write branch below.
            if let Some(cfg) = hash_cfg_path {
              escape_continuation_hashes(
                bin,
                cfg,
                scratch,
                ctx.root.as_path(),
              )?;
            }
            let mut md_cmd = create_tool_command(bin);
            // Fixes #150: this argv used to be hand-assembled here, and the
            // hand-assembled copy omitted `extra_args`. See
            // `build_markdownlint_fix_argv` — one builder shared with the
            // write branch below, so the two cannot drift apart again.
            md_cmd.args(build_markdownlint_fix_argv(
              &[scratch.to_path_buf()],
              md_temp_cfg_path,
              ctx,
            ));
            md_cmd.current_dir(ctx.root.as_path());

            // Issue #113: a markdownlint-cli2 `--fix` pass that could not run
            // must surface here, not be discarded — otherwise `fml fmt
            // --check` reports a clean diff that a real `fml fmt` would not
            // reproduce. Exit 1 just means unfixable violations remain
            // (MD001, MD041, …); prettier still takes the next pass. Any
            // other non-zero exit, or a spawn failure, is handed straight on
            // to `diff_check_via_tempcopy_classified`, which renders it as an
            // `ExecutionError` (`classify_all_nonzero_as_error` below —
            // nothing that reaches that classifier is a formatting result:
            // markdownlint's exit-1 outcomes are filtered out right here, and
            // `prettier --write` only exits non-zero on an operational
            // failure).
            let md_outcome = md_cmd.output();
            if markdownlint_fix_pass_failed(&md_outcome) {
              return md_outcome;
            }
          }

          let mut cmd = create_tool_command("prettier");
          cmd
            .arg("--write")
            .arg("--parser")
            .arg("markdown")
            .args(&inline_config)
            .arg(scratch);
          cmd.args(&ctx.lang_config.extra_args);
          cmd.current_dir(ctx.root.as_path());
          let output = cmd.output()?;
          if !output.status.success() {
            return Ok(output);
          }

          // Fixes #253: format block-level HTML (badge wrappers,
          // `<details>`/`<summary>` sections, `<div>` wrappers) that
          // prettier's markdown parser leaves untouched. Runs after
          // prettier's own markdown pass above so it only ever sees
          // already-normalized nested markdown content. Any failure here
          // (an IO error reading/writing the scratch copy — the html
          // sub-pass itself never fails the run, see
          // `format_block_html`'s doc comment) surfaces as an
          // `ExecutionError` via the early return on `?`, exactly like a
          // prettier spawn failure would.
          apply_block_html_pass(
            scratch,
            &inline_config,
            &ctx.lang_config.extra_args,
          )?;

          // #314: the same post-prettier step as the write branch below, so
          // `--check` reports exactly the diff a real `fml fmt` writes.
          if let (Some(bin), Some(cfg)) = (md_binary, hash_cfg_path) {
            escape_continuation_hashes(bin, cfg, scratch, ctx.root.as_path())?;
          }
          Ok(output)
        },
        self.name(),
        start,
        classify_all_nonzero_as_error,
      );
    }

    if let Some(bin) = md_binary {
      // #413: a hand-wrapped `#299)` must be escaped before the fixer below
      // spaces it into a heading.
      if let Some(cfg) = hash_cfg_path
        && let Err(e) = escape_all(bin, cfg, &files, ctx.root.as_path())
      {
        return escape_failed(self.name(), start, &e);
      }
      let mut md_cmd = create_tool_command(bin);
      // Fixes #150: same builder as the `check_only` branch above, differing
      // only in the paths handed to the tool. See
      // `build_markdownlint_fix_argv`.
      md_cmd.args(build_markdownlint_fix_argv(&files, md_temp_cfg_path, ctx));
      md_cmd.current_dir(ctx.root.as_path());

      // Issue #113: this pass used to be `let _ = md_cmd.output()`-discarded,
      // so a markdownlint-cli2 that could not run (bad `--config`, internal
      // crash, unresolvable binary) left prettier's later success as the
      // surface's whole result and `fml fmt` reported `[PASS]` on a half-run
      // pipeline. markdownlint-cli2 exits 1 when it merely found violations it
      // can't autofix — prettier must still run and the format must not fail
      // on that basis — so `classify_exit_one_as_violation` maps only a non-1
      // non-zero exit (or a spawn failure) to `ExecutionError`, the one
      // outcome surfaced here.
      let md_res = run_tool_command_classified(
        self.name(),
        &mut md_cmd,
        classify_exit_one_as_violation,
      );
      if matches!(md_res.status, SurfaceStatus::ExecutionError { .. }) {
        return md_res;
      }
    }

    let mut cmd = create_tool_command("prettier");
    cmd.args(build_prettier_fmt_args(&files, &ctx.lang_config.extra_args));
    cmd.args(&inline_config);
    cmd.current_dir(ctx.root.as_path());

    // `prettier --write` exits 0 whether or not it reformatted anything and
    // only exits non-zero on an operational failure (parse error, bad
    // `--config`, unreadable file) — no "found drift" exit code, same as the
    // `--check` path above. Every non-zero exit here is therefore an
    // `ExecutionError`, not a lint-style `ViolationsFound` (Fixes #155).
    let res = run_tool_command_classified(
      self.name(),
      &mut cmd,
      classify_all_nonzero_as_error,
    );
    if !res.is_success() {
      return res;
    }

    // Fixes #253: same block-level HTML pass as the `--check` branch above,
    // applied in place to the real files once prettier's own markdown pass
    // has succeeded. See `apply_block_html_pass` and `format_block_html`.
    for file in &files {
      if let Err(e) =
        apply_block_html_pass(file, &inline_config, &ctx.lang_config.extra_args)
      {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to format embedded HTML in {}: {e}",
              file.display()
            ),
          },
          duration: start.elapsed(),
        };
      }
    }

    // #314: prettier's wrap can leave a `#` at a line start, which the next
    // `markdownlint --fix` would turn into a heading.
    if let (Some(bin), Some(cfg)) = (md_binary, hash_cfg_path)
      && let Err(e) = escape_all(bin, cfg, &files, ctx.root.as_path())
    {
      return escape_failed(self.name(), start, &e);
    }
    res
  }

  fn lint(&self, ctx: &ExecutionContext, fix: bool) -> SurfaceResult {
    let start = Instant::now();

    let binary = if check_binary_exists("markdownlint-cli2") {
      "markdownlint-cli2"
    } else if check_binary_exists("markdownlint") {
      "markdownlint"
    } else {
      return tool_missing_result(
        self.name(),
        start,
        "markdownlint-cli2",
        &install_hint_for("markdownlint-cli2"),
      );
    };

    let files = ctx.matched_files(MD_EXTENSIONS);
    if let Some(res) = ctx.early_out_if_empty(&files, self.name(), start) {
      return res;
    }

    // See `write_markdownlint_temp_config`: markdownlint-cli2 only accepts
    // config via `--config <path>`, so the resolved formality.toml settings
    // are rendered to a throwaway temp file rather than depending on
    // `.markdownlint.json` being present on disk. `md_temp_cfg` must stay
    // alive until `cmd.output()` below returns.
    let md_temp_cfg = match write_markdownlint_temp_config(&ctx.lang_config) {
      Ok(f) => f,
      Err(e) => {
        return SurfaceResult {
          surface_name: self.name(),
          status: SurfaceStatus::ExecutionError {
            message: format!(
              "Failed to write temporary markdownlint config: {e}"
            ),
          },
          duration: start.elapsed(),
        };
      }
    };

    // #413: `fml fix` runs this pass before `format()`, so it needs the same
    // pre-fixer escape, or `--fix` spaces a hand-wrapped `#299)` first.
    if fix
      && let Err(e) = write_hash_rules_temp_config().and_then(|cfg| {
        escape_all(binary, cfg.path(), &files, ctx.root.as_path())
      })
    {
      return escape_failed(self.name(), start, &e);
    }

    let mut cmd = create_tool_command(binary);
    cmd.args(build_markdownlint_args(
      &files,
      fix,
      Some(md_temp_cfg.path()),
      &ctx.lang_config.extra_args,
    ));
    cmd.current_dir(ctx.root.as_path());

    let mut res = run_tool_command(self.name(), &mut cmd);
    match &mut res.status {
      SurfaceStatus::ViolationsFound { message, .. }
      | SurfaceStatus::ExecutionError { message } => {
        *message = filter_markdownlint_noise(message);
      }
      _ => {}
    }
    res
  }

  fn uses_prettier(&self) -> bool {
    true
  }

  // `.markdownlint.json` is written here because `fml fmt`'s
  // markdownlint-cli2 pass and `fml lint` both still consume it — see the
  // comment in `format()` for why that tool can't take its settings inline.
  //
  // `.prettierrc.json` is deliberately *not* written here even though this
  // surface formats via prettier: it is shared with the JSON and YAML
  // surfaces, and syncing it from all three under `surfaces.par_iter()` put
  // three threads on one path (#130). The shared pass
  // (`sync_shared_prettier_config`) owns it; `uses_prettier` above is this
  // surface's declaration that it consumes it. `fml fmt` is unaffected — it
  // passes prettier's settings inline (Fixes #151 [pre-recreation]).
  fn sync_config(&self, ctx: &ExecutionContext, check: bool) -> SurfaceResult {
    sync_native_config::<MarkdownlintConfig>(
      ctx,
      check,
      Instant::now(),
      self.name(),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::ResolvedLangConfig;
  use crate::surfaces::test_ctx;
  use tempfile::TempDir;

  #[test]
  fn test_prettier_config_typed_serialization() {
    let cfg = PrettierConfig {
      comment: "warning".to_string(),
      tab_width: 4,
      print_width: 100,
      use_tabs: true,
      end_of_line: "crlf".to_string(),
      prose_wrap: "preserve".to_string(),
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$comment\": \"warning\""));
    assert!(rendered.contains("\"tabWidth\": 4"));
    assert!(rendered.contains("\"printWidth\": 100"));
    assert!(rendered.contains("\"useTabs\": true"));
    assert!(rendered.contains("\"endOfLine\": \"crlf\""));
    assert!(rendered.contains("\"proseWrap\": \"preserve\""));
  }

  #[test]
  fn test_markdownlint_config_typed_serialization() {
    let cfg = MarkdownlintConfig {
      comment: MarkdownlintComment {
        description: "desc".to_string(),
      },
      default: true,
      md007: MarkdownlintMd007 { indent: 2 },
      md013: MarkdownlintMd013 {
        line_length: 120,
        code_blocks: false,
        tables: false,
      },
      md033: false,
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$comment\":"));
    assert!(rendered.contains("\"description\": \"desc\""));
    assert!(rendered.contains("\"default\": true"));
    assert!(rendered.contains("\"MD007\":"));
    assert!(rendered.contains("\"indent\": 2"));
    assert!(rendered.contains("\"MD013\":"));
    assert!(rendered.contains("\"line_length\": 120"));
    assert!(rendered.contains("\"MD033\": false"));
  }

  #[test]
  fn test_markdown_supports_lint_fix() {
    assert!(MarkdownSurface.supports_lint_fix());
  }

  #[test]
  fn test_build_markdownlint_args_with_and_without_fix() {
    let no_fix = build_markdownlint_args(&[], false, None, &[]);
    assert_eq!(no_fix, Vec::<String>::new());

    let files = vec![PathBuf::from("a.md"), PathBuf::from("b.md")];
    let extra = vec!["--loglevel".to_string(), "warn".to_string()];
    let with_fix = build_markdownlint_args(&files, true, None, &extra);
    assert_eq!(
      with_fix,
      vec![
        "--fix".to_string(),
        "a.md".to_string(),
        "b.md".to_string(),
        "--loglevel".to_string(),
        "warn".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_markdownlint_args_with_config_path() {
    let files = vec![PathBuf::from("a.md")];
    let cfg_path = PathBuf::from("/tmp/some-config.json");
    let args =
      build_markdownlint_args(&files, true, Some(cfg_path.as_path()), &[]);
    assert_eq!(
      args,
      vec![
        "--fix".to_string(),
        "--config".to_string(),
        cfg_path.to_string_lossy().to_string(),
        "a.md".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_markdownlint_args_extra_args_config_wins_last() {
    // markdownlint-cli2 honours the *last* `--config` flag it sees, so a
    // project-supplied `extra_args = ["--config", "mine.json"]` must land
    // after the injected temp-config path to actually override it (see
    // `write_markdownlint_temp_config`'s doc comment).
    let files = vec![PathBuf::from("a.md")];
    let injected = PathBuf::from("/tmp/.markdownlint-abc123.json");
    let extra = vec!["--config".to_string(), "mine.json".to_string()];
    let args =
      build_markdownlint_args(&files, false, Some(injected.as_path()), &extra);
    assert_eq!(
      args,
      vec![
        "--config".to_string(),
        injected.to_string_lossy().to_string(),
        "a.md".to_string(),
        "--config".to_string(),
        "mine.json".to_string(),
      ]
    );
    // The user-supplied override is the last "--config" in the arg list.
    let last_config_idx = args.iter().rposition(|a| a == "--config").unwrap();
    assert_eq!(args[last_config_idx + 1], "mine.json");
  }

  #[test]
  fn test_write_markdownlint_temp_config_reflects_formality_toml() {
    // Regression test for the gap CI caught on issue #1: markdownlint-cli2
    // has no per-flag inline config, so `fml lint`/`fml fmt` used to run it
    // with whatever `.markdownlint.json` happened to be on disk (or its own
    // stricter built-in defaults — MD013 `code_blocks`/`tables: true` — if
    // that file was absent). The temp-file config must carry
    // formality.toml's actual resolved MD013 settings.
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.line_length = 100;

    let temp_cfg = write_markdownlint_temp_config(&lang_cfg).unwrap();
    let content = std::fs::read_to_string(temp_cfg.path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["MD013"]["line_length"], 100);
    assert_eq!(parsed["MD013"]["code_blocks"], false);
    assert_eq!(parsed["MD013"]["tables"], false);
  }

  #[test]
  fn test_markdownlint_config_for_lang_disables_md033_by_default() {
    // Issue #120: MD033/no-inline-html fires unfixably on ordinary README
    // idioms (centered badge blocks, `<details>` disclosure widgets), so the
    // shipped default must disable it.
    let lang_cfg = ResolvedLangConfig::new("markdown");
    let cfg = markdownlint_config_for_lang(&lang_cfg);
    assert!(!cfg.md033, "MD033 must be disabled by default");
  }

  #[test]
  fn test_markdownlint_config_for_lang_reenables_md033_from_formality_toml() {
    // Issue #120 acceptance criterion: MD033 must be re-enablable, not just
    // removed — `[lang.markdown] no_inline_html = true`.
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.markdown = Some(crate::config::MarkdownOptions {
      prose_wrap: None,
      no_inline_html: Some(true),
    });
    let cfg = markdownlint_config_for_lang(&lang_cfg);
    assert!(cfg.md033, "MD033 must be re-enabled when opted back in");
  }

  #[test]
  fn test_markdownlint_config_for_lang_syncs_md007_indent_with_indent_size() {
    // Issue #394: prettier indents nested lists using tabWidth (indent_size),
    // so markdownlint's MD007 indent must match indent_size to avoid oscillation.
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.indent_size = 4;
    let cfg = markdownlint_config_for_lang(&lang_cfg);
    assert_eq!(cfg.md007.indent, 4);

    let rendered = cfg.render().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed["MD007"]["indent"], 4);
  }

  #[test]
  fn test_markdown_nested_list_indent_agrees_at_configured_indent_size() {
    // Issue #394 acceptance criterion: fml fix followed by fml lint passes
    // on a nested list fixture at indent_size 2 and 4 without oscillation.
    if !check_binary_exists("prettier")
      || (!check_binary_exists("markdownlint-cli2")
        && !check_binary_exists("markdownlint"))
    {
      return;
    }

    for indent_size in [2, 4] {
      let temp = TempDir::new().unwrap();
      let file = temp.path().join("list.md");
      std::fs::write(&file, "# Title\n\n* item\n    * nested\n").unwrap();

      let mut lang_cfg = ResolvedLangConfig::new("markdown");
      lang_cfg.indent_size = indent_size;
      let ctx = test_ctx(temp.path(), lang_cfg);
      let surface = MarkdownSurface;

      let fmt_res = surface.format(&ctx);
      assert!(
        fmt_res.is_success(),
        "format failed for indent_size = {indent_size}: {:?}",
        fmt_res.status
      );

      let lint_res = surface.lint(&ctx, false);
      assert!(
        lint_res.is_success(),
        "lint failed for indent_size = {indent_size}: {:?}",
        lint_res.status
      );
    }
  }

  /// Fixture README containing a centered badge block and a `<details>`
  /// disclosure widget — the exact ordinary README idioms that trip
  /// MD033/no-inline-html unfixably. This repo's own README has zero inline
  /// HTML, which is exactly why CI dogfooding never caught issue #120; this
  /// fixture deliberately breaks that coupling.
  const README_WITH_INLINE_HTML: &str = "# Project\n\n\
<p align=\"center\">\n\
  <img src=\"badge.png\" alt=\"badge\">\n\
</p>\n\n\
Some ordinary prose.\n\n\
<details>\n\
<summary>More info</summary>\n\n\
Extra detail text.\n\n\
</details>\n";

  #[test]
  fn test_lint_md033_does_not_fire_by_default_on_readme_with_inline_html() {
    // Issue #120 acceptance criterion: a README fixture with real inline
    // HTML must lint clean by default now that MD033 ships disabled.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("README.md"), README_WITH_INLINE_HTML)
      .unwrap();
    assert!(!temp.path().join(".markdownlint.json").exists());

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = surface.lint(&ctx, false);
    assert!(
      res.is_success(),
      "expected MD033 not to fire by default, got: {:?}",
      res.status
    );
  }

  #[test]
  fn test_lint_md033_fires_when_reenabled_on_readme_with_inline_html() {
    // Issue #120 acceptance criterion: opting back in via
    // `[lang.markdown] no_inline_html = true` must restore MD033
    // enforcement on the very same fixture that lints clean by default.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("README.md"), README_WITH_INLINE_HTML)
      .unwrap();

    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.markdown = Some(crate::config::MarkdownOptions {
      prose_wrap: None,
      no_inline_html: Some(true),
    });
    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.lint(&ctx, false);
    assert!(
      !res.is_success(),
      "expected MD033 to fire once re-enabled, got: {:?}",
      res.status
    );
    let combined = format!("{:?}", res.status);
    assert!(
      combined.contains("MD033"),
      "expected an MD033 violation in the lint output, got: {combined}"
    );
  }

  #[test]
  fn test_lint_respects_formality_toml_md013_with_no_config_on_disk() {
    // End-to-end regression test for the same gap: with no
    // `.markdownlint.json` anywhere on disk (the actual state of this repo's
    // own root after issue #1), `lint()` must still enforce formality.toml's
    // MD013 settings (via the temp-config `--config` pass), not
    // markdownlint-cli2's stricter built-in defaults.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let temp = TempDir::new().unwrap();
    // A code fence line over 80 chars, made of real words with spaces
    // (NOT a single unbroken token like "x".repeat(90) — MD013's default
    // `strict: false` exempts any line with no spaces past the limit, so an
    // unbroken-token line is never flagged by *any* config, config-present
    // or config-absent, fixed or broken; that made an earlier version of
    // this test pass even with the fix reverted). With real spaces,
    // markdownlint-cli2's built-in MD013 default (`code_blocks: true`)
    // flags this; formality.toml's default (`code_blocks: false`, set in
    // `MarkdownlintConfig::from_context`) must not.
    let long_line = "lorem ipsum dolor sit amet ".repeat(5);
    std::fs::write(
      temp.path().join("a.md"),
      format!("# Title\n\n```text\n{long_line}\n```\n"),
    )
    .unwrap();
    assert!(!temp.path().join(".markdownlint.json").exists());

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = surface.lint(&ctx, false);
    assert!(
      res.is_success(),
      "expected clean lint with no .markdownlint.json on disk, got: {:?}",
      res.status
    );
    // Still no native config file was written as a side effect.
    assert!(!temp.path().join(".markdownlint.json").exists());
  }

  #[test]
  fn test_filter_markdownlint_noise_drops_banner_keeps_violations() {
    // The exact stream shape `merge_tool_streams` produces: banner + Finding
    // + Linting + Summary from stdout, then a bare `stderr:` label, then the
    // real per-violation lines.
    let raw = "\
markdownlint-cli2 v0.23.2 (markdownlint v0.41.1)
Finding: /home/u/proj/README.md /home/u/proj/docs/x.md
Linting: 2 files
Summary: 2 issues in 1 file

stderr:
/home/u/proj/README.md:7:3 error MD019/no-multiple-space-atx Multiple spaces after hash [Context: \"#  Bad\"]
README.md:7 error MD025/single-title/single-h1 Multiple top-level headings";

    let out = filter_markdownlint_noise(raw);

    assert!(!out.contains("markdownlint-cli2 v"), "banner kept: {out}");
    assert!(!out.contains("Finding:"), "Finding line kept: {out}");
    assert!(!out.contains("Linting:"), "Linting line kept: {out}");
    assert!(!out.contains("stderr:"), "stderr label kept: {out}");
    assert!(out.contains("MD019/no-multiple-space-atx"));
    assert!(out.contains("MD025/single-title/single-h1"));
    // filter_markdownlint_noise no longer relativizes paths itself (#157) —
    // that absolute leading path survives this function untouched...
    assert!(
      out.contains("/home/u/proj/README.md:7:3 error MD019"),
      "leading path should still be absolute here, got: {out}"
    );
    // ...and is relativized exactly once downstream, by the same shared
    // helper the runner calls on every surface's diagnostics.
    let relativized =
      crate::ui::paths::relativize_text(Path::new("/home/u/proj"), &out);
    assert!(
      relativized.contains("README.md:7:3 error MD019"),
      "downstream relativize_text should strip the leading root path, got: \
       {relativized}"
    );
    assert!(
      !relativized.contains("/home/u/proj"),
      "no run-root-absolute path should survive the full pipeline, got: \
       {relativized}"
    );
    // Summary is kept, but as a bare tail count at the end.
    assert_eq!(out.lines().last().unwrap(), "2 issues in 1 file");
  }

  #[test]
  fn test_filter_markdownlint_noise_passes_unknown_lines_through() {
    // Acceptance criterion: filtering is anchored to known prefixes, so an
    // unrecognized stdout line from a future markdownlint-cli2 version must
    // still reach the user.
    let raw =
      "Analyzing: something new\nfoo.md:1 error MD012/no-multiple-blanks";
    let out = filter_markdownlint_noise(raw);
    assert!(out.contains("Analyzing: something new"));
    assert!(out.contains("MD012/no-multiple-blanks"));
  }

  #[test]
  fn test_lint_diagnostic_shows_rule_id_without_banner_or_file_list() {
    // End-to-end regression for issue #109: a fixture with a known MD-rule
    // violation must produce a diagnostic that carries the rule id and
    // carries neither the `Finding:` echo nor the input file list.
    if !check_binary_exists("markdownlint-cli2")
      && !check_binary_exists("markdownlint")
    {
      return;
    }

    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("bad.md"),
      "#  Bad Heading\n\nsome text\n\n# Another Top Heading\n",
    )
    .unwrap();

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = surface.lint(&ctx, false);
    let SurfaceStatus::ViolationsFound { message, .. } = &res.status else {
      panic!("expected ViolationsFound, got: {:?}", res.status);
    };

    assert!(
      message.contains("MD019"),
      "diagnostic must name the violated rule, got: {message}"
    );
    assert!(
      !message.contains("Finding:"),
      "the `Finding:` path echo must be filtered out, got: {message}"
    );
    assert!(
      !message.contains("Linting:"),
      "the `Linting: N files` line must be filtered out, got: {message}"
    );
    assert!(
      !message.contains("markdownlint-cli2 v"),
      "the version banner must be filtered out, got: {message}"
    );
    assert!(
      !message.contains(&*temp.path().to_string_lossy()),
      "no absolute input path may appear, got: {message}"
    );
  }

  #[test]
  fn test_has_unspaced_hash_line_ignores_headings_and_fenced_code() {
    // MD018's and MD020's own positives (`/^#+[^# \t]/`) must all pass the
    // prefilter, or a candidate never reaches markdownlint.
    for positive in ["#x", "##x", "#x#", "###299 ok.", "#299 for C#"] {
      assert!(
        has_unspaced_hash_line(&format!("text\n{positive}\n")),
        "{positive} must count"
      );
    }
    assert!(!has_unspaced_hash_line("# T\n\n## Sub\n\n#\n\n# Title#\n"));
    assert!(!has_unspaced_hash_line("#\t tab\n##\n"));
    assert!(!has_unspaced_hash_line("```sh\n#!/bin/sh\n```\n"));
    // A shorter or different marker does not close the outer fence.
    assert!(!has_unspaced_hash_line("````\n```\n#x\n~~~\n#y\n````\n"));
    assert!(has_unspaced_hash_line("```\ncode\n```\n#after\n"));
    // Four columns of indent make it no fence marker, so line 3 closes.
    assert!(has_unspaced_hash_line("```\n    ```\n```\n#after\n"));
    // A backtick in a backtick opener's info string makes it a code span.
    assert!(has_unspaced_hash_line("```a`\n#x\n"));
  }

  #[test]
  fn test_hash_rule_lines_reads_md018_and_md020_stdin_findings() {
    // Verbatim shapes from markdownlint-cli2 v0.23.2 and markdownlint-cli
    // v0.49.1 reading stdin; a finding under any other name is not ours.
    let out = "Summary: 4 issues in 1 file\n\
      stdin:4:1 error MD020/no-missing-space-closed-atx No space inside \
      hashes on closed atx style heading [Context: \"#299 for C#\"]\n\
      stdin:6:1 error MD018/no-missing-space-atx No space [Context: \"##x\"]\n\
      stdin:7 error MD025/single-title Multiple top-level headings\n\
      my docs/a.md:9:1 error MD018/no-missing-space-atx No space\n\
      stdin:12 MD018/no-missing-space-atx No space\n";
    assert_eq!(hash_rule_lines(out), vec![4, 6, 12]);
  }

  #[test]
  fn test_escape_line_hashes_skips_real_headings_and_unlisted_lines() {
    // `# Title#` is an MD020 finding but already a heading: escaping it
    // would demote a heading the author wrote.
    let src = "a\n#1 b\n#2 c\n# Title#\nd\n##x\r\n";
    assert_eq!(
      escape_line_hashes(src, &[2, 4, 6]),
      "a\n\\#1 b\n#2 c\n# Title#\nd\n\\##x\r\n"
    );
  }

  #[test]
  fn test_escape_line_hashes_escapes_only_paragraph_continuations() {
    // #413: a `#a` that opens a block of its own is left for markdownlint's
    // fixer to space into `# a`; one that continues a paragraph (lazily,
    // in a list item, blockquote or GFM table row) is escaped.
    let opens_block = [
      "#a\n",
      "p\n\n#a\n",
      "# T\n#a\n",
      "p\n===\n#a\n",
      "- i\n  ===\n#a\n",
      "> q\n\np\n===\n#a\n",
      "p\n---\n#a\n",
      "p\n\n***\n#a\n",
      "- - -\n\n    code\n#a\n",
      "```\nc\n```\n#a\n",
      "```\n    ```\n```\n#a\n",
      "    ```\n#a\n",
      "    ~~~\n#a\n",
      "```a`\n\n#a\n",
      "    code\n#a\n",
      "-     c\n\n      code\n#a\n",
      "- i\n\n      code\n#a\n",
      "- i\n\np\n\n    code\n#a\n",
      "- a\n  - b\n\n  p\n\n      code\n#a\n",
      "<!--\nc\n-->\n#a\n",
      "p\n<!-- c -->\n#a\n",
      "<PRE>\nc\n</pre>\n#a\n",
      "+++\nx = 1\n+++\n#a\n",
      "---\nx: 1\n---\n#a\n",
    ];
    let continues = [
      "p\n#a\n",
      "p\n\n===\n#a\n",
      "p\n\n--\n#a\n",
      "p\n+++\n#a\n",
      "p\n    more\n#a\n",
      "| a |\n| - |\n| 1 |\n#a\n",
      "> q\n#a\n",
      "- i\n#a\n",
      "1. i\n\n    more\n#a\n",
      "- i\n\n    more\n#a\n",
      "-\ti\n\n    more\n#a\n",
      "1.\ti\n\n    more\n#a\n",
      " -\ti\n\n    more\n#a\n",
      "- a\n  - b\n\n      more\n#a\n",
      "> q\n===\n#a\n",
      "- i\n===\n#a\n",
      "- > q\n  ===\n#a\n",
      "<span>x</span>\n#a\n",
    ];
    for src in opens_block.into_iter().chain(continues) {
      let last = src.lines().count();
      let want = if continues.contains(&src) {
        format!("{}\\#a\n", &src[..src.len() - 3])
      } else {
        src.to_string()
      };
      assert_eq!(escape_line_hashes(src, &[last]), want, "for {src:?}");
    }
  }

  /// The first line of a paragraph that prettier's `prose_wrap = "always"`
  /// fills to exactly 80 columns, so the next token starts the next line.
  const WRAP_LEAD: &str = "This line is long enough that the next token \
    wraps onto the next line here, see";

  /// Tokens that land at a line start after [`WRAP_LEAD`], each one a shape
  /// whose markdownlint fixer turns prose into a heading: MD018 (`#x`,
  /// `##x`) and MD020 (`#x#`).
  const WRAPPED_TOKENS: [&str; 3] = ["#299) ok.", "##299 ok.", "#299 for C#"];

  /// Runs the markdown half of `fml fix`: the fixing lint pass, then the
  /// format pass.
  fn fix_once(ctx: &ExecutionContext) {
    let _ = MarkdownSurface.lint(ctx, true);
    let res = MarkdownSurface.format(ctx);
    assert!(res.is_success(), "format failed: {:?}", res.status);
  }

  fn have_markdown_tools() -> bool {
    check_binary_exists("markdownlint-cli2") && check_binary_exists("prettier")
  }

  #[test]
  fn test_format_escapes_hash_that_prettier_wraps_to_line_start() {
    // #314: prettier wrapped `#299)` to a line start, where MD018 (or MD020
    // for a trailing `#`) reported it as unfixable and the next
    // `markdownlint --fix` made it a heading.
    if !have_markdown_tools() {
      return;
    }

    for token in WRAPPED_TOKENS {
      let temp = TempDir::new().unwrap();
      let file = temp.path().join("doc.md");
      std::fs::write(&file, format!("# T\n\n{WRAP_LEAD} {token}\n")).unwrap();
      let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

      fix_once(&ctx);
      let first = std::fs::read_to_string(&file).unwrap();
      assert!(
        first.ends_with(&format!("\n\\{token}\n")),
        "the wrapped `{token}` must be escaped, got: {first}"
      );
      let recheck = MarkdownSurface.lint(&ctx, false);
      assert!(
        recheck.is_success(),
        "the post-format recheck must be clean for `{token}`, got: {:?}",
        recheck.status
      );

      // Convergence: further fix runs change nothing, so no heading appears.
      for _ in 0..2 {
        fix_once(&ctx);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), first);
      }

      let mut check_ctx = ctx;
      check_ctx.check_only = true;
      let check = MarkdownSurface.format(&check_ctx);
      assert!(
        check.is_success(),
        "fmt --check drifted: {:?}",
        check.status
      );
    }
  }

  #[test]
  fn test_format_escapes_under_a_path_with_a_space() {
    // The finding must be attributed whatever the file's path looks like:
    // a `path:line` parser split on whitespace dropped it here.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("my docs");
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, format!("# T\n\n{WRAP_LEAD} #299) ok.\n")).unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = MarkdownSurface.format(&ctx);
    assert!(res.is_success(), "format failed: {:?}", res.status);
    let out = std::fs::read_to_string(&file).unwrap();
    assert!(out.ends_with("\n\\#299) ok.\n"), "got: {out}");
  }

  #[test]
  fn test_format_detection_ignores_a_discovered_fix_true_config() {
    // A `.markdownlint-cli2.jsonc` with `"fix": true` must not turn the
    // read-only MD018/MD020 check into a fixer that writes `# 299) ok.`.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join(".markdownlint-cli2.jsonc"),
      "{\"fix\": true}\n",
    )
    .unwrap();
    let file = temp.path().join("doc.md");
    std::fs::write(&file, format!("# T\n\n{WRAP_LEAD} #299) ok.\n")).unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = MarkdownSurface.format(&ctx);
    assert!(res.is_success(), "format failed: {:?}", res.status);
    let out = std::fs::read_to_string(&file).unwrap();
    assert!(out.ends_with("\n\\#299) ok.\n"), "got: {out}");
  }

  #[test]
  fn test_format_check_reports_the_escape_as_drift() {
    // The `--check` branch must apply the same escape as the write branch,
    // or `fml fmt --check` would report a diff `fml fmt` never writes.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("doc.md"),
      format!("# T\n\n{WRAP_LEAD} #299) ok.\n"),
    )
    .unwrap();
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));
    ctx.check_only = true;

    let res = MarkdownSurface.format(&ctx);
    let SurfaceStatus::ViolationsFound {
      diff: Some(diff), ..
    } = &res.status
    else {
      panic!("expected drift with a diff, got: {:?}", res.status);
    };
    assert!(
      diff.contains("+\\#299) ok."),
      "the drift must be the escape, got: {diff}"
    );
  }

  /// #413: a paragraph the author wrapped by hand right before `#299)`.
  const HAND_WRAPPED: &str =
    "# T\n\nFixed in the parser, see\n#299) for details.\n";

  /// [`HAND_WRAPPED`] after `fml fmt`: prettier joins the escaped line.
  const HAND_WRAPPED_FORMATTED: &str =
    "# T\n\nFixed in the parser, see \\#299) for details.\n";

  #[test]
  fn test_format_escapes_a_hand_wrapped_hash_before_the_fixer() {
    // The first `markdownlint --fix` pass used to space `#299)` into the H1
    // `# 299) for details` before any escape ran.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("doc.md");
    std::fs::write(&file, HAND_WRAPPED).unwrap();
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    ctx.check_only = true;
    let check = MarkdownSurface.format(&ctx);
    let SurfaceStatus::ViolationsFound {
      diff: Some(diff), ..
    } = &check.status
    else {
      panic!("expected drift with a diff, got: {:?}", check.status);
    };
    assert!(
      diff.contains("+Fixed in the parser, see \\#299) for details."),
      "--check must report the escape, got: {diff}"
    );

    ctx.check_only = false;
    for run in 1..=2 {
      let res = MarkdownSurface.format(&ctx);
      assert!(res.is_success(), "format failed: {:?}", res.status);
      assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        HAND_WRAPPED_FORMATTED,
        "run {run}"
      );
    }
  }

  #[test]
  fn test_fix_escapes_a_hand_wrapped_hash_before_the_lint_fixer() {
    // `fml fix` runs `markdownlint --fix` from `lint()` before `format()`.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("doc.md");
    std::fs::write(&file, HAND_WRAPPED).unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    for run in 1..=2 {
      fix_once(&ctx);
      assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        HAND_WRAPPED_FORMATTED,
        "run {run}"
      );
    }
  }

  #[test]
  fn test_format_spaces_an_unspaced_heading_that_opens_a_block() {
    // The other half of #413's decision: `#Title` on its own block is a
    // heading the author forgot to space, not prose to escape.
    if !have_markdown_tools() {
      return;
    }

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("doc.md");
    std::fs::write(&file, "#Title\n\nText.\n").unwrap();
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = MarkdownSurface.format(&ctx);
    assert!(res.is_success(), "format failed: {:?}", res.status);
    assert_eq!(
      std::fs::read_to_string(&file).unwrap(),
      "# Title\n\nText.\n"
    );
  }

  #[test]
  fn test_build_prettier_fmt_args() {
    let files = vec![PathBuf::from("readme.md")];
    let extra = vec!["--loglevel".to_string(), "warn".to_string()];
    let args = build_prettier_fmt_args(&files, &extra);
    assert_eq!(
      args,
      vec![
        "--write".to_string(),
        "readme.md".to_string(),
        "--loglevel".to_string(),
        "warn".to_string(),
      ]
    );
  }

  #[test]
  fn test_markdown_sync_config() {
    let temp = TempDir::new().unwrap();
    let surface = MarkdownSurface;
    let mut lang_cfg = ResolvedLangConfig::new("markdown");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 2;

    let ctx = test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    // Fixes #130: the file this surface writes is named in its result. It
    // used to return only the prettier half, leaving `.markdownlint.json`
    // created on disk and reported nowhere.
    assert_eq!(res.status.created_file_names(), [".markdownlint.json"]);

    let md_path = temp.path().join(".markdownlint.json");
    assert!(md_path.is_file());

    let md_content = std::fs::read_to_string(&md_path).unwrap();
    assert!(md_content.contains("\"line_length\": 100"));

    // `.prettierrc.json` is shared with the json and yaml surfaces and is
    // written once by `sync_shared_prettier_config`, outside the runner's
    // parallel fan-out — never from here (#130).
    assert!(surface.uses_prettier());
    assert!(!temp.path().join(".prettierrc.json").exists());
  }

  #[test]
  fn test_build_prettier_inline_args_shape() {
    let cfg = PrettierConfig {
      comment: "warning".to_string(),
      tab_width: 4,
      print_width: 100,
      use_tabs: true,
      end_of_line: "crlf".to_string(),
      prose_wrap: "preserve".to_string(),
    };
    let args = build_prettier_inline_args(&cfg);
    assert!(args.contains(&"--tab-width=4".to_string()));
    assert!(args.contains(&"--print-width=100".to_string()));
    assert!(args.contains(&"--end-of-line=crlf".to_string()));
    assert!(args.contains(&"--prose-wrap=preserve".to_string()));
    assert!(args.contains(&"--use-tabs".to_string()));
  }

  #[test]
  fn test_markdown_format_does_not_write_prettierrc() {
    // Fixes #151 [pre-recreation]: `fml fmt` must not write `.prettierrc.json` as a side
    // effect; only `fml sync` should materialize the native config file.
    if !check_binary_exists("prettier") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.md"), "# hi\n").unwrap();

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let _ = surface.format(&ctx);

    assert!(!temp.path().join(".prettierrc.json").exists());
  }

  #[test]
  fn test_markdownlint_fix_pass_failed_ok_on_clean_exit_zero() {
    // A clean file: markdownlint-cli2 --fix exits 0, which is not a failed
    // pass — format() proceeds to prettier as before.
    if !check_binary_exists("markdownlint-cli2") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let f = temp.path().join("clean.md");
    std::fs::write(&f, "# Title\n\nA clean paragraph.\n").unwrap();

    let mut cmd = create_tool_command("markdownlint-cli2");
    cmd.arg("--fix").arg(&f);
    assert!(!markdownlint_fix_pass_failed(&cmd.output()));
  }

  #[test]
  fn test_markdownlint_fix_pass_failed_tolerates_unfixable_violations_exit_one()
  {
    // markdownlint-cli2 exits 1 when `--fix` can't resolve every violation
    // (MD001 heading-increment has no autofixer). Per issue #113's acceptance
    // criteria that is NOT a failed pass: format() must still hand off to
    // prettier rather than failing the run.
    if !check_binary_exists("markdownlint-cli2") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let f = temp.path().join("unfixable.md");
    std::fs::write(&f, "# Level one\n\n### Skipped level two\n").unwrap();

    let mut cmd = create_tool_command("markdownlint-cli2");
    cmd.arg("--fix").arg(&f);
    let outcome = cmd.output();
    assert_eq!(
      outcome.as_ref().unwrap().status.code(),
      Some(1),
      "precondition: an unfixable violation makes markdownlint-cli2 exit 1"
    );
    assert!(
      !markdownlint_fix_pass_failed(&outcome),
      "exit 1 (unfixable violations remain) must not count as a failed pass"
    );
  }

  #[test]
  fn test_markdownlint_fix_pass_failed_flags_invalid_config_path() {
    // Acceptance criterion for issue #113: a deliberately invalid `--config`
    // path makes markdownlint-cli2 exit 2 (ENOENT) — a real failure to
    // execute that must be surfaced, not discarded.
    if !check_binary_exists("markdownlint-cli2") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let f = temp.path().join("a.md");
    std::fs::write(&f, "# Title\n\nHi.\n").unwrap();

    let mut cmd = create_tool_command("markdownlint-cli2");
    cmd
      .arg("--fix")
      .arg("--config")
      .arg(temp.path().join("nonexistent-config.json"))
      .arg(&f);
    assert!(markdownlint_fix_pass_failed(&cmd.output()));
  }

  #[test]
  fn test_markdownlint_fix_pass_failed_flags_unresolvable_binary() {
    // Acceptance criterion for issue #113: an unresolvable markdownlint
    // binary (the spawn itself fails) must count as a failed pass.
    let mut cmd =
      create_tool_command("markdownlint-cli2-does-not-exist-fml113");
    cmd.arg("--fix");
    assert!(markdownlint_fix_pass_failed(&cmd.output()));
  }

  #[test]
  fn test_markdown_format_write_pass_reports_execution_error_on_bad_config() {
    // End-to-end on the write branch's exact machinery: the markdownlint-cli2
    // `--fix` pass, when it cannot run (invalid `--config` path -> exit 2),
    // must classify as `ExecutionError` and therefore NOT let the surface
    // report success. Before issue #113 this outcome was `let _ =`-discarded
    // and prettier's later success became the surface's whole result.
    if !check_binary_exists("markdownlint-cli2") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let f = temp.path().join("a.md");
    std::fs::write(&f, "# Title\n\nHi.\n").unwrap();

    let mut md_cmd = create_tool_command("markdownlint-cli2");
    md_cmd
      .arg("--fix")
      .arg("--config")
      .arg(temp.path().join("nonexistent-config.json"))
      .arg(&f);
    let res = run_tool_command_classified(
      "markdown",
      &mut md_cmd,
      classify_exit_one_as_violation,
    );

    assert!(
      matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "a bad --config path must classify as ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn test_markdown_write_reports_execution_error_on_prettier_failure() {
    // Fixes #155: the write path's own prettier pass (the tail end of
    // `format()`, reached once no markdownlint binary is configured or its
    // pass already succeeded) must classify an operational prettier failure
    // as `ExecutionError`, not `ViolationsFound` — `prettier --write` has no
    // "found drift" exit code, same reasoning as the `--check` path.
    if !check_binary_exists("prettier") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.md"), "# hi\n").unwrap();

    let mut lang = ResolvedLangConfig::new("markdown");
    lang.extra_args = vec![
      "--config".to_string(),
      temp
        .path()
        .join("nonexistent-prettier-config-fml155.json")
        .to_string_lossy()
        .into_owned(),
    ];
    let ctx = test_ctx(temp.path(), lang);

    let surface = MarkdownSurface;
    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, SurfaceStatus::ExecutionError { .. }),
      "a prettier failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn test_build_markdownlint_fix_argv_forwards_extra_args() {
    // Fixes #150. Both of `format()`'s markdownlint-cli2 `--fix` passes now
    // build their argv through `build_markdownlint_fix_argv`, so this is the
    // single place the bug can reappear — and asserting on the argv makes the
    // check deterministic and PATH-independent. Dropping the
    // `&ctx.lang_config.extra_args` forwarding inside that builder fails this
    // test; the previous end-to-end `ExecutionError`-variant assertions did
    // not, because prettier receives `extra_args` too and fails on the same
    // bad `--config` (see
    // `test_markdown_write_reports_execution_error_on_prettier_failure`).
    let temp = TempDir::new().unwrap();
    let mut lang = ResolvedLangConfig::new("markdown");
    lang.extra_args = vec![
      "--no-globs".to_string(),
      "--loglevel".to_string(),
      "warn".to_string(),
    ];
    let ctx = test_ctx(temp.path(), lang);

    let files = vec![PathBuf::from("a.md"), PathBuf::from("b.md")];
    let injected = PathBuf::from("/tmp/.markdownlint-abc123.json");
    let argv =
      build_markdownlint_fix_argv(&files, Some(injected.as_path()), &ctx);

    assert_eq!(
      argv,
      vec![
        "--fix".to_string(),
        "--config".to_string(),
        injected.to_string_lossy().to_string(),
        "a.md".to_string(),
        "b.md".to_string(),
        "--no-globs".to_string(),
        "--loglevel".to_string(),
        "warn".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_markdownlint_fix_argv_single_scratch_and_config_ordering() {
    // The `--check` branch hands the builder exactly one path (the temp copy
    // `diff_check_via_tempcopy_classified` made), where the write branch hands
    // it every matched file — that is the only difference between the two, and
    // this pins the single-path shape.
    //
    // It also pins the ordering the documented `--config` behaviour depends
    // on: `extra_args` lands *after* the injected temp config, and
    // markdownlint-cli2 honours the last `--config` it sees, so a
    // user-supplied one silently overrides `fml`'s resolved markdownlint
    // settings on the `fml fmt` path. Verified against markdownlint-cli2
    // v0.23.2; see `build_markdownlint_fix_argv` and
    // `docs/language-surfaces.md`.
    let temp = TempDir::new().unwrap();
    let mut lang = ResolvedLangConfig::new("markdown");
    lang.extra_args = vec!["--config".to_string(), "mine.json".to_string()];
    let ctx = test_ctx(temp.path(), lang);

    let scratch = PathBuf::from("/tmp/scratch/a.md");
    let injected = PathBuf::from("/tmp/.markdownlint-abc123.json");
    let argv = build_markdownlint_fix_argv(
      std::slice::from_ref(&scratch),
      Some(injected.as_path()),
      &ctx,
    );

    assert_eq!(
      argv,
      vec![
        "--fix".to_string(),
        "--config".to_string(),
        injected.to_string_lossy().to_string(),
        scratch.to_string_lossy().to_string(),
        "--config".to_string(),
        "mine.json".to_string(),
      ]
    );
    let last_config_idx = argv.iter().rposition(|a| a == "--config").unwrap();
    assert_eq!(argv[last_config_idx + 1], "mine.json");
  }

  #[test]
  fn test_markdown_write_extra_args_failure_is_attributed_to_markdownlint() {
    // Fixes #150, end to end, and deliberately *not* a bare
    // `matches!(status, ExecutionError { .. })` assertion: prettier already
    // received `extra_args` before this change and exits 2 on the very same
    // nonexistent `--config`, so the variant alone still holds with the
    // markdownlint forwarding removed. The *message* is what separates them —
    // markdownlint-cli2 exits 2 on an unreadable `--config`, which
    // `classify_exit_one_as_violation` maps to `ExecutionError` and the write
    // branch returns early with, before prettier ever runs. Its text is
    // markdownlint's own; prettier reports a JSON parse error instead.
    if !check_binary_exists("markdownlint-cli2") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.md"), "# hi\n").unwrap();

    let mut lang = ResolvedLangConfig::new("markdown");
    lang.extra_args = vec![
      "--config".to_string(),
      temp
        .path()
        .join("nonexistent-markdownlint-config-fml150.json")
        .to_string_lossy()
        .into_owned(),
    ];
    let ctx = test_ctx(temp.path(), lang);

    let res = MarkdownSurface.format(&ctx);
    let SurfaceStatus::ExecutionError { message } = &res.status else {
      panic!(
        "extra_args must reach the markdownlint --fix pass on the write path, got: {:?}",
        res.status
      );
    };
    assert!(
      message.contains("Unable to use configuration file"),
      "the failure must be markdownlint's, not prettier's, got: {message}"
    );
    assert!(!res.is_success());
  }

  // --- Fixes #253: block-level embedded HTML formatting ---

  #[test]
  fn test_extract_html_block_ranges_finds_block_not_inline() {
    let src = "# T\n\n<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n\n\
    Prose with <strong>inline</strong> html.\n";
    let spans = extract_html_block_ranges(src);
    assert_eq!(spans.len(), 1, "only the block-level <p> should be found");
    let (s, e) = spans[0];
    assert!(src[s..e].starts_with("<p align=\"center\">"));
    assert!(src[s..e].contains("</p>"));
    assert!(
      !src[s..e].contains("<strong>"),
      "the block span must not swallow the later inline HTML"
    );
  }

  #[test]
  fn test_extract_html_block_ranges_skips_blocks_inside_containers() {
    // Block HTML in a blockquote carries `> ` prefixes and in a list item
    // carries the item's indent: prettier's html parser would mangle the
    // former and dedent the latter out of the list.
    for src in [
      "> <div align=\"center\">\n> <img src=\"a.png\">\n> </div>\n",
      "- item\n\n  <div align=\"center\">\n  <img src=\"a.png\">\n  </div>\n",
      "Text[^1].\n\n[^1]: <div>\n    <img src=\"a.png\">\n    </div>\n",
    ] {
      assert_eq!(extract_html_block_ranges(src), vec![], "in: {src}");
    }
  }

  #[test]
  fn test_block_structure_flags_list_exit_and_new_code_block() {
    let list = "- item\n\n  <div>\n  <img src=\"a.png\">\n  </div>\n";
    let dedented = "- item\n\n  <div>\n  <img src=\"a.png\" />\n</div>\n";
    assert_ne!(block_structure(list), block_structure(dedented));
    let nested =
      "<div>\n\n<div>\n\n<div>\n\nText.\n\n</div>\n\n</div>\n\n</div>\n";
    let indented = "<div>\n\n  <div>\n\n    <div>\n\nText.\n\n    </div>\n\n  </div>\n\n</div>\n";
    assert_ne!(block_structure(nested), block_structure(indented));
    let badge =
      "<p align=\"center\">\n<img src=\"a.png\"     alt=\"b\">\n</p>\n";
    let tidy =
      "<p align=\"center\">\n  <img src=\"a.png\" alt=\"b\" />\n</p>\n";
    assert_eq!(block_structure(badge), block_structure(tidy));
  }

  #[test]
  fn test_format_block_html_leaves_nested_blank_line_wrappers_untouched() {
    if !check_binary_exists("prettier") {
      return;
    }
    // Prettier indents each nesting level; past three spaces after a blank
    // line the wrapper would turn into an indented code block.
    let src = "<div>\n\n<div>\n\n<div>\n\n<details>\n<summary>X</summary>\n\n\
    Body.\n\n</details>\n\n</div>\n\n</div>\n\n</div>\n";
    assert_eq!(format_block_html(src, &[], &[]), src);
  }

  #[test]
  fn test_extract_html_block_ranges_honours_prettier_ignore() {
    let blocks = |src: &'static str| -> Vec<&'static str> {
      let spans = extract_html_block_ranges(src);
      spans.into_iter().map(|(s, e)| &src[s..e]).collect()
    };
    // The comment shields only the next block, even when that block is a
    // paragraph, and a comment sharing a block with html shields that block.
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n\n<p>\n<b>a</b>\n</p>\n\n<p>c</p>\n"),
      vec!["<p>c</p>\n"]
    );
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n<p>\n<b>a</b>\n</p>\n"),
      [""; 0]
    );
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n\nText.\n\n<p>c</p>\n"),
      vec!["<p>c</p>\n"]
    );
    assert_eq!(
      blocks(
        "<!-- prettier-ignore-start -->\n\n<p>a</p>\n\n<div>b</div>\n\n\
         <!-- prettier-ignore-end -->\n\n<p>c</p>\n"
      ),
      vec!["<p>c</p>\n"]
    );
  }

  #[test]
  fn test_extract_html_block_ranges_skips_comment_only_blocks() {
    // Nothing for prettier to tidy, and each would cost a Node spawn.
    let src = "<!-- markdownlint-disable MD013 -->\n\n<p>c</p>\n\n\
               <!-- a -->\n<!--\nb\n-->\n";
    let spans = extract_html_block_ranges(src);
    let blocks: Vec<&str> = spans.iter().map(|&(s, e)| &src[s..e]).collect();
    assert_eq!(blocks, vec!["<p>c</p>\n"]);
  }

  #[test]
  fn test_scan_html_tags_ignores_void_and_self_closing() {
    let tokens = scan_html_tags("<div><img src=\"a.png\"><br/></div>").unwrap();
    assert_eq!(tokens.len(), 4);
    assert!(!tokens[0].closing && tokens[0].name == "div");
    assert!(
      !tokens[1].closing && tokens[1].name == "img" && !tokens[1].self_closing
    );
    assert!(
      !tokens[2].closing && tokens[2].name == "br" && tokens[2].self_closing
    );
    assert!(tokens[3].closing && tokens[3].name == "div");
  }

  #[test]
  fn test_scan_html_tags_ignores_comments_and_gt_in_attribute_values() {
    // A `>` inside a quoted attribute value must not be mistaken for the
    // tag's own closing `>`, and HTML comments must not be tokenized as tags.
    let tokens =
      scan_html_tags("<!-- <fake> --><div title=\"a > b\"></div>").unwrap();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].name, "div");
    assert!(!tokens[0].closing);
    assert_eq!(tokens[1].name, "div");
    assert!(tokens[1].closing);
  }

  #[test]
  fn test_group_html_blocks_single_self_contained_block() {
    let src = "<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n";
    let spans = extract_html_block_ranges(src);
    assert_eq!(spans.len(), 1, "no blank line inside -> one HtmlBlock span");
    let groups = group_html_blocks(src, &spans).unwrap();
    assert_eq!(groups, vec![vec![0]]);
  }

  #[test]
  fn test_group_html_blocks_regroups_details_split_by_blank_line() {
    // CommonMark's HTML-block rule ends a block at the first blank line, so
    // the `<details>`/`<summary>` opener and the `</details>` closer arrive
    // as two separate spans with the interior markdown outside both.
    let src =
      "<details>\n<summary>More</summary>\n\nExtra text.\n\n</details>\n";
    let spans = extract_html_block_ranges(src);
    assert_eq!(spans.len(), 2, "opener and closer are separate HtmlBlocks");
    let groups = group_html_blocks(src, &spans).unwrap();
    assert_eq!(
      groups,
      vec![vec![0, 1]],
      "the opener and closer must regroup into one balanced unit"
    );
  }

  #[test]
  fn test_group_html_blocks_bails_on_mismatched_closing_tag() {
    // A closing tag with nothing on the stack to match -> None, meaning
    // "leave the whole document's block HTML untouched" rather than guess.
    let src = "</div>\n\n<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n";
    let spans = extract_html_block_ranges(src);
    assert!(group_html_blocks(src, &spans).is_none());
  }

  #[test]
  fn test_format_block_html_normalizes_p_align_center_badge() {
    if !check_binary_exists("prettier") {
      return;
    }
    let src = "# Project\n\n<p align=\"center\">\n  <img src=\"a.png\"     alt=\"badge\">\n</p>\n";
    let out = format_block_html(src, &[], &[]);
    assert!(
      !out.contains("     alt"),
      "the quadruple space in the <img> attributes must be collapsed, got: {out}"
    );
    assert!(out.contains("<p align=\"center\">"));
  }

  #[test]
  fn test_format_block_html_normalizes_details_summary_section() {
    if !check_binary_exists("prettier") {
      return;
    }
    let src = "<details>\n<summary   class=\"foo\"     >More info</summary>\n\n\
    Extra detail text.\n\n</details>\n";
    let out = format_block_html(src, &[], &[]);
    assert!(
      out.contains("<summary class=\"foo\">More info</summary>"),
      "the messy <summary> attribute spacing must be normalized, got: {out}"
    );
    assert!(
      out.contains("Extra detail text."),
      "interior markdown content must survive untouched, got: {out}"
    );
    assert!(out.trim_end().ends_with("</details>"));
  }

  #[test]
  fn test_format_block_html_leaves_inline_html_byte_identical() {
    if !check_binary_exists("prettier") {
      return;
    }
    let src = "<p align=\"center\">\n  <img src=\"a.png\"     alt=\"badge\">\n</p>\n\n\
    Some prose with an <strong>inline</strong>    span here.\n";
    let out = format_block_html(src, &[], &[]);
    assert!(
      out.contains("Some prose with an <strong>inline</strong>    span here."),
      "an inline HTML span mid-paragraph, and its surrounding whitespace, \
       must be byte-identical to the input, got: {out}"
    );
  }

  #[test]
  fn test_format_block_html_is_idempotent() {
    if !check_binary_exists("prettier") {
      return;
    }
    let src = "# Project\n\n<p align=\"center\">\n  <img src=\"a.png\"     alt=\"badge\">\n</p>\n\n\
    Some prose with an <strong>inline</strong>    span here.\n\n\
    <details>\n<summary   class=\"foo\"     >More info</summary>\n\n\
    Extra detail text.\n\n</details>\n";
    let once = format_block_html(src, &[], &[]);
    let twice = format_block_html(&once, &[], &[]);
    assert_eq!(once, twice, "a second pass must be a no-op");
  }

  #[test]
  fn test_format_block_html_leaves_mismatched_document_untouched() {
    // No prettier binary needed: group_html_blocks bails before any
    // subprocess would be spawned.
    let src = "</div>\n\n<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n";
    let out = format_block_html(src, &[], &[]);
    assert_eq!(out, src);
  }

  #[test]
  fn test_markdown_format_normalizes_block_html_end_to_end() {
    // Acceptance criteria for #253: a `<p align="center">` badge block and a
    // `<details>` section are both normalized by a real `fml fmt` write pass,
    // while an inline `<strong>` mid-paragraph stays byte-identical, and a
    // second run is a no-op.
    if !check_binary_exists("prettier") {
      return;
    }
    let temp = TempDir::new().unwrap();
    let readme = temp.path().join("README.md");
    std::fs::write(&readme, README_WITH_INLINE_HTML).unwrap();

    let surface = MarkdownSurface;
    let ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));

    let res = surface.format(&ctx);
    assert!(
      res.is_success(),
      "expected format to pass, got: {:?}",
      res.status
    );

    let formatted = std::fs::read_to_string(&readme).unwrap();
    assert!(
      formatted.contains("<img src=\"badge.png\" alt=\"badge\" />")
        || formatted.contains("<img src=\"badge.png\" alt=\"badge\">"),
      "the badge <img> tag must survive formatting, got: {formatted}"
    );
    assert!(formatted.contains("<details>"));
    assert!(formatted.contains("<summary>More info</summary>"));

    // Second run must be a no-op.
    let res2 = surface.format(&ctx);
    assert!(res2.is_success());
    let formatted_again = std::fs::read_to_string(&readme).unwrap();
    assert_eq!(
      formatted, formatted_again,
      "a second `fml fmt` run must not change the file"
    );
  }

  #[test]
  fn test_markdown_format_check_only_reports_block_html_drift() {
    if !check_binary_exists("prettier") {
      return;
    }
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join("README.md"),
      "<p align=\"center\">\n  <img src=\"a.png\"     alt=\"badge\">\n</p>\n",
    )
    .unwrap();

    let surface = MarkdownSurface;
    let mut ctx = test_ctx(temp.path(), ResolvedLangConfig::new("markdown"));
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, SurfaceStatus::ViolationsFound { .. }),
      "the messy <img> attribute spacing must be reported as drift under \
       --check, got: {:?}",
      res.status
    );
  }
}
