//! Block-level HTML embedded in markdown: finds it, formats it through
//! prettier's html parser and splices it back (#253).
//!
//! MD033/no-inline-html ships disabled (#120) because README idioms such as a
//! centered badge block (`<p align="center">` + `<img>`) or a `<details>`
//! section have no markdown equivalent. Prettier's markdown parser keeps such
//! a block byte for byte (prettier 3.9.9:
//! `printf '<p align="center">\n<img src="a.png"     alt="b">\n</p>\n' |
//! prettier --parser markdown` echoes the input, quadruple space intact), so
//! this pass formats it instead. Inline HTML inside a paragraph is never
//! touched: whitespace around an inline element renders.
//!
//! The parent `super` surface owns the pipeline and decides when this pass
//! runs; this module owns only the extract, format and splice of one file.

use crate::surfaces::tooling;
use pulldown_cmark;
use std::fmt::Write;
use std::path;
use std::process;

/// HTML void elements (WHATWG), which never take a closing tag.
const VOID_ELEMENTS: &[&str] = &[
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
  "param", "source", "track", "wbr",
];

/// One tag [`scan_tags`] found, reduced to what [`blocks_balanced`] tracks.
struct TagToken {
  /// The lowercased tag name.
  name: String,
  /// The tag is a closing tag (`</p>`).
  closing: bool,
  /// The tag closes itself (`<br/>`).
  self_closing: bool,
}

/// Tokenizes the tags in an HTML fragment, or `None` when a tag or comment
/// is unterminated.
///
/// Not an HTML parser: it honours quoted attribute values (so `alt="a > b"`
/// does not end a tag) and skips comments and `<!...>`/`<?...?>`
/// declarations, but knows no raw-text elements such as `<script>`. Anything
/// it misreads unbalances the stack, and [`blocks_balanced`] then leaves the
/// file as written.
fn scan_tags(html: &str) -> Option<Vec<TagToken>> {
  let mut tokens = Vec::new();
  let mut rest = html;
  while let Some(open) = rest.find('<') {
    rest = &rest[open..];
    if let Some(comment) = rest.strip_prefix("<!--") {
      rest = &comment[comment.find("-->")? + 3..];
      continue;
    }
    let body = &rest[1..];
    if body.starts_with(['!', '?']) {
      rest = &rest[rest.find('>')? + 1..];
      continue;
    }
    let (closing, body) =
      body.strip_prefix('/').map_or((false, body), |b| (true, b));
    if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
      // A bare `<` in text, not a tag.
      rest = &rest[1..];
      continue;
    }
    let name_len = body
      .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
      .unwrap_or(body.len());
    let attrs = &body[name_len..];
    let end = tag_end(attrs)?;
    tokens.push(TagToken {
      name: body[..name_len].to_ascii_lowercase(),
      closing,
      self_closing: attrs[..end].trim_end().ends_with('/'),
    });
    rest = &attrs[end + 1..];
  }
  Some(tokens)
}

/// The byte offset of the `>` that ends a tag whose name precedes `attrs`,
/// skipping quoted attribute values, or `None` when the tag never ends.
fn tag_end(attrs: &str) -> Option<usize> {
  let mut quote = None;
  for (pos, c) in attrs.char_indices() {
    match (quote, c) {
      (Some(q), _) if c == q => quote = None,
      (None, '"' | '\'') => quote = Some(c),
      (None, '>') => return Some(pos),
      _ => {}
    }
  }
  None
}

/// Parses `src` with the GFM extensions and front matter prettier's markdown
/// parser honours, so a table or footnote next to HTML parses as prettier
/// sees it, and HTML inside `---`/`+++` front matter is never a block.
fn parse(src: &str) -> pulldown_cmark::Parser<'_> {
  let opts = pulldown_cmark::Options::ENABLE_TABLES
    | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
    | pulldown_cmark::Options::ENABLE_FOOTNOTES
    | pulldown_cmark::Options::ENABLE_TASKLISTS
    | pulldown_cmark::Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
    | pulldown_cmark::Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
  pulldown_cmark::Parser::new_ext(src, opts)
}

/// Byte ranges of the top-level HTML blocks in `src` this pass may format,
/// in document order.
///
/// A block inside a container (blockquote, list item, footnote) is skipped:
/// the html parser would read its `> ` prefixes as text and dedent it out of
/// the container. Also skipped: comment-only blocks, which hold nothing to
/// tidy, the block after `<!-- prettier-ignore -->`, every block between
/// `<!-- prettier-ignore-start -->` and `<!-- prettier-ignore-end -->`, and
/// any block that itself holds such a directive.
fn block_ranges(src: &str) -> Vec<(usize, usize)> {
  let mut spans = Vec::new();
  let mut depth = 0usize;
  let (mut ignore_next, mut ignore_range) = (false, false);
  for (event, range) in parse(src).into_offset_iter() {
    match event {
      pulldown_cmark::Event::Start(tag) => {
        if depth == 0 {
          let is_html = tag == pulldown_cmark::Tag::HtmlBlock;
          let block = &src[range.clone()];
          let mut skip = std::mem::take(&mut ignore_next)
            || ignore_range
            || (is_html && is_comment_only(block));
          // Without blank lines a directive shares its neighbour's block, so
          // every comment in the block counts, not just a leading one.
          let mut rest = if is_html { block } else { "" };
          while let Some(open) = rest.find("<!--")
            && let Some((directive, after)) = leading_comment(&rest[open..])
          {
            match directive {
              "prettier-ignore" => ignore_next = after.trim().is_empty(),
              "prettier-ignore-start" => ignore_range = true,
              "prettier-ignore-end" => ignore_range = false,
              _ => {}
            }
            skip |= directive.starts_with("prettier-ignore");
            rest = after;
          }
          if is_html && !skip {
            spans.push((range.start, range.end));
          }
        }
        depth += 1;
      }
      pulldown_cmark::Event::Rule if depth == 0 => ignore_next = false,
      pulldown_cmark::Event::End(_) => depth -= 1,
      _ => {}
    }
  }
  spans
}

/// Splits `block` into its leading HTML comment's trimmed text and the text
/// after that comment, or `None` when `block` does not open with one.
fn leading_comment(block: &str) -> Option<(&str, &str)> {
  let rest = block.trim_start().strip_prefix("<!--")?;
  let end = rest.find("-->")?;
  Some((rest[..end].trim(), &rest[end + 3..]))
}

/// Whether `block` holds nothing but HTML comments and whitespace.
#[must_use]
fn is_comment_only(mut block: &str) -> bool {
  while let Some((_, rest)) = leading_comment(block) {
    block = rest;
  }
  block.trim().is_empty()
}

/// Whether the tags across `spans`, read in document order as one stack,
/// balance.
///
/// `CommonMark` ends an HTML block at a blank line, so a `<details>` opener
/// and its `</details>` arrive as two spans with markdown between them.
/// Prettier's html parser repairs a lone opener by inserting a closer
/// (prettier 3.9.9: `printf '<details>\n<summary>X</summary>\n' | prettier
/// --parser html` prints a `</details>` line), so the spans may only be
/// formatted together, and only when they balance.
fn blocks_balanced(src: &str, spans: &[(usize, usize)]) -> bool {
  let mut stack = Vec::new();
  for &(start, end) in spans {
    let Some(tokens) = scan_tags(&src[start..end]) else {
      return false;
    };
    for tok in tokens {
      // A void element's stray closer (`</br>`) still pops, and mismatches.
      if tok.self_closing
        || (!tok.closing && VOID_ELEMENTS.contains(&tok.name.as_str()))
      {
        continue;
      }
      if !tok.closing {
        stack.push(tok.name);
      } else if stack.pop() != Some(tok.name) {
        return false;
      }
    }
  }
  stack.is_empty()
}

/// Runs prettier's html parser over `html`, returning `None` when it cannot
/// run or fails.
///
/// `--html-whitespace-sensitivity=css`, prettier's default, is pinned: it
/// keeps whitespace around inline elements significant, where `ignore`
/// splits `<strong>Bold</strong><em>Italic</em>` onto two lines and the page
/// renders "Bold Italic". `args` (inline config, then the user's `prettier`
/// extra args) follow, as in the markdown pass, and it runs from `root` so
/// prettier resolves plugins the same way. `--stdin-filepath` names the
/// markdown file, so a file `.prettierignore` lists comes back unchanged, as
/// the markdown pass leaves it.
fn run_prettier(
  html: &str,
  file: &path::Path,
  root: &path::Path,
  args: &[String],
) -> Option<String> {
  let mut child = tooling::create_tool_command("prettier")
    .args(["--parser", "html", "--html-whitespace-sensitivity=css"])
    .arg("--stdin-filepath")
    .arg(file)
    .args(args)
    .current_dir(root)
    .stdin(process::Stdio::piped())
    .stdout(process::Stdio::piped())
    .stderr(process::Stdio::piped())
    .spawn()
    .ok()?;
  // Prettier reads all of stdin before printing, so writing it in full
  // first cannot deadlock; dropping the handle closes the pipe.
  std::io::Write::write_all(&mut child.stdin.take()?, html.as_bytes()).ok()?;
  let output = child.wait_with_output().ok()?;
  if !output.status.success() {
    return None;
  }
  String::from_utf8(output.stdout).ok()
}

/// The start of every [`gap_marker`]; a file already holding it is left as
/// written, since its own copy could split the batch in the wrong place.
const GAP_PREFIX: &str = "<!--fml-html-gap-";

/// The marker line [`format_blocks`] puts between the `index`th and next
/// span of its batch.
fn gap_marker(index: usize) -> String {
  format!("{GAP_PREFIX}{index}-->")
}

/// Splits prettier's output for a batch back into one part per span, or
/// `None` when a marker is missing or no longer alone on its line.
fn split_on_gaps(formatted: &str, gaps: usize) -> Option<Vec<&str>> {
  let mut parts = Vec::with_capacity(gaps + 1);
  let mut rest = formatted;
  for gi in 0..gaps {
    let marker = gap_marker(gi);
    let pos = rest.find(&marker)?;
    let line_start = rest[..pos].rfind('\n').map_or(0, |p| p + 1);
    let after = pos + marker.len();
    let line_end = rest[after..]
      .find('\n')
      .map_or(rest.len(), |p| after + p + 1);
    if !rest[line_start..pos].trim().is_empty()
      || !rest[after..line_end].trim().is_empty()
    {
      return None;
    }
    parts.push(&rest[..line_start]);
    rest = &rest[line_end..];
  }
  parts.push(rest);
  Some(parts)
}

/// The `Start`/`End` sequence `pulldown-cmark` sees in `src`, equal for two
/// documents exactly when they nest the same blocks and spans.
fn block_structure(src: &str) -> Vec<(bool, pulldown_cmark::TagEnd)> {
  parse(src)
    .filter_map(|event| match event {
      pulldown_cmark::Event::Start(tag) => Some((true, tag.to_end())),
      pulldown_cmark::Event::End(tag) => Some((false, tag)),
      _ => None,
    })
    .collect()
}

/// Formats every block-level HTML node in `src`, leaving every other byte
/// as written.
///
/// All spans go to one prettier spawn, joined by [`gap_marker`] lines, so an
/// opener and its closer form one balanced document while the markdown
/// between them (which the html parser would collapse into text) is spliced
/// back verbatim. Returns `src` unchanged whenever anything is inconclusive:
/// unbalanced tags, prettier failing, a reflowed marker, or a result that
/// parses as a different block structure (prettier indenting a nested
/// wrapper four spaces after a blank line makes it indented code). `file`
/// names `src`'s file for prettier's ignore rules.
fn format_blocks(
  src: &str,
  file: &path::Path,
  root: &path::Path,
  args: &[String],
) -> String {
  let spans = block_ranges(src);
  if spans.is_empty()
    || src.contains(GAP_PREFIX)
    || !blocks_balanced(src, &spans)
  {
    return src.to_string();
  }
  let mut batch = String::new();
  for (i, &(start, end)) in spans.iter().enumerate() {
    if i > 0 {
      let _ = writeln!(batch, "{}", gap_marker(i - 1));
    }
    batch.push_str(&src[start..end]);
    if !batch.ends_with('\n') {
      batch.push('\n');
    }
  }
  let Some(formatted) = run_prettier(&batch, file, root, args) else {
    return src.to_string();
  };
  let Some(parts) = split_on_gaps(&formatted, spans.len() - 1) else {
    return src.to_string();
  };
  let mut out = String::with_capacity(src.len());
  let mut cursor = 0;
  for (&(start, end), part) in spans.iter().zip(parts) {
    out.push_str(&src[cursor..start]);
    out.push_str(part.trim_end_matches('\n'));
    out.push('\n');
    cursor = end;
  }
  out.push_str(&src[cursor..]);
  if block_structure(&out) == block_structure(src) {
    out
  } else {
    src.to_string()
  }
}

/// Formats the block-level HTML in the markdown file at `path`, run from
/// `root` with prettier `args`.
///
/// A failure to format leaves the file as written; only I/O fails the call.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written.
///
/// # Side Effects
///
/// Rewrites `path` when its HTML changed; a file without block HTML costs a
/// read and no process.
pub fn format_file(
  path: &path::Path,
  root: &path::Path,
  args: &[String],
) -> std::io::Result<()> {
  let content = std::fs::read_to_string(path)?;
  let updated = format_blocks(&content, path, root, args);
  if updated != content {
    std::fs::write(path, updated)?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn have_prettier() -> bool {
    tooling::check_binary_exists("prettier")
  }

  fn fmt(src: &str) -> String {
    format_blocks(src, path::Path::new("a.md"), path::Path::new("."), &[])
  }

  /// The text of each span [`block_ranges`] returns for `src`.
  fn blocks(src: &str) -> Vec<&str> {
    block_ranges(src)
      .into_iter()
      .map(|(s, e)| &src[s..e])
      .collect()
  }

  #[test]
  fn block_ranges_finds_block_not_inline_html() {
    let src = "# T\n\n<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n\n\
      Prose with <strong>inline</strong> html.\n";
    assert_eq!(
      blocks(src),
      vec!["<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n"]
    );
  }

  #[test]
  fn block_ranges_skips_blocks_inside_containers() {
    for src in [
      "> <div align=\"center\">\n> <img src=\"a.png\">\n> </div>\n",
      "- item\n\n  <div align=\"center\">\n  <img src=\"a.png\">\n  </div>\n",
      "Text[^1].\n\n[^1]: <div>\n    <img src=\"a.png\">\n    </div>\n",
      // One-line blocks carry no prefix inside their span, so only the
      // depth rule keeps prettier from growing them out of the container.
      "> <p align=\"center\"><img src=\"a.png\"></p>\n",
      "- item\n\n  <img src=\"a.png\"     alt=\"b\">\n",
      // Front matter is not markdown; its YAML would lose an indent.
      "---\nd: |\n  <div align=\"center\">\n  <img src=\"a.png\">\n  </div>\n\
       ---\n\n# T\n",
      "+++\nd = '''\n  <div>\n  <img src=\"a.png\">\n  </div>\n'''\n+++\n",
    ] {
      assert_eq!(block_ranges(src), vec![], "in: {src}");
    }
  }

  #[test]
  fn block_ranges_honours_prettier_ignore() {
    // The comment shields only the next block, even a paragraph, and a
    // comment sharing a block with html shields that block.
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n\n<p>\n<b>a</b>\n</p>\n\n<p>c</p>\n"),
      vec!["<p>c</p>\n"]
    );
    assert!(
      blocks("<!-- prettier-ignore -->\n<p>\n<b>a</b>\n</p>\n").is_empty()
    );
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n\nText.\n\n<p>c</p>\n"),
      vec!["<p>c</p>\n"]
    );
    assert_eq!(
      blocks("<!-- prettier-ignore -->\n\n---\n\n<p>c</p>\n"),
      vec!["<p>c</p>\n"]
    );
    assert_eq!(
      blocks(
        "<!-- prettier-ignore-start -->\n\n<p>a</p>\n\n<div>b</div>\n\n\
         <!-- prettier-ignore-end -->\n\n<p>c</p>\n"
      ),
      vec!["<p>c</p>\n"]
    );
    // With no blank lines the directives share the table's block; the end
    // comment on its last line must still close the range.
    assert_eq!(
      blocks(
        "<!-- prettier-ignore-start -->\n<table><td>a</td></table>\n\
         <!-- prettier-ignore-end -->\n\n<p>c</p>\n"
      ),
      vec!["<p>c</p>\n"]
    );
    assert!(
      blocks("<p>a</p>\n<!-- prettier-ignore-start -->\n\n<p>b</p>\n")
        .is_empty()
    );
  }

  #[test]
  fn block_ranges_skips_comment_only_blocks() {
    let src = "<!-- markdownlint-disable MD013 -->\n\n<p>c</p>\n\n\
      <!-- a -->\n<!--\nb\n-->\n";
    assert_eq!(blocks(src), vec!["<p>c</p>\n"]);
  }

  #[test]
  fn scan_tags_reads_void_self_closing_and_quoted_gt() {
    let tokens =
      scan_tags("<!-- <fake> --><div title=\"a > b\"><img><br/></div>")
        .unwrap();
    let got: Vec<_> = tokens
      .iter()
      .map(|t| (t.name.as_str(), t.closing, t.self_closing))
      .collect();
    assert_eq!(
      got,
      vec![
        ("div", false, false),
        ("img", false, false),
        ("br", false, true),
        ("div", true, false),
      ]
    );
    assert!(scan_tags("<div title=\"a").is_none());
  }

  #[test]
  fn blocks_balanced_pairs_details_split_by_blank_line() {
    let src = "<details>\n<summary>More</summary>\n\nText.\n\n</details>\n";
    let spans = block_ranges(src);
    assert_eq!(spans.len(), 2, "opener and closer are separate blocks");
    assert!(blocks_balanced(src, &spans));
    assert!(!blocks_balanced(src, &spans[..1]), "opener alone");
    let svg = "<svg><path d=\"x\"/></svg>\n";
    assert!(
      blocks_balanced(svg, &block_ranges(svg)),
      "self-closing path"
    );
  }

  #[test]
  fn blocks_balanced_rejects_stray_closing_tag() {
    let src = "</div>\n\n<p align=\"center\">\n  <img src=\"a.png\">\n</p>\n";
    assert!(!blocks_balanced(src, &block_ranges(src)));
    // Rejected before any prettier spawn.
    assert_eq!(fmt(src), src);
  }

  #[test]
  fn split_on_gaps_requires_marker_on_its_own_line() {
    let ok = "<p>a</p>\n  <!--fml-html-gap-0-->\n<p>b</p>\n";
    assert_eq!(split_on_gaps(ok, 1), Some(vec!["<p>a</p>\n", "<p>b</p>\n"]));
    let reflowed = "<b>a</b> <!--fml-html-gap-0-->\n<b>b</b>\n";
    assert_eq!(split_on_gaps(reflowed, 1), None);
  }

  #[test]
  fn block_structure_flags_list_exit_and_new_code_block() {
    let list = "- item\n\n  <div>\n  <img src=\"a.png\">\n  </div>\n";
    let dedented = "- item\n\n  <div>\n  <img src=\"a.png\" />\n</div>\n";
    assert_ne!(block_structure(list), block_structure(dedented));
    let nested =
      "<div>\n\n<div>\n\n<div>\n\nText.\n\n</div>\n\n</div>\n\n</div>\n";
    let indented = "<div>\n\n  <div>\n\n    <div>\n\nText.\n\n    </div>\n\n  \
      </div>\n\n</div>\n";
    assert_ne!(block_structure(nested), block_structure(indented));
    let badge =
      "<p align=\"center\">\n<img src=\"a.png\"     alt=\"b\">\n</p>\n";
    let tidy =
      "<p align=\"center\">\n  <img src=\"a.png\" alt=\"b\" />\n</p>\n";
    assert_eq!(block_structure(badge), block_structure(tidy));
  }

  #[test]
  fn format_blocks_tidies_badge_and_details_and_is_idempotent() {
    if !have_prettier() {
      return;
    }
    let src = "# Project\n\n<p align=\"center\">\n  \
      <img src=\"a.png\"     alt=\"badge\">\n</p>\n\n\
      Some prose with an <strong>inline</strong>    span here.\n\n\
      <details>\n<summary   class=\"foo\"     >More info</summary>\n\n\
      Extra detail text.\n\n</details>\n";
    // Formatted alone, the opener would gain a spurious `</details>`; the
    // balanced batch yields exactly one closer. The inline span keeps its
    // spacing.
    let want = "# Project\n\n<p align=\"center\">\n  \
      <img src=\"a.png\" alt=\"badge\" />\n</p>\n\n\
      Some prose with an <strong>inline</strong>    span here.\n\n\
      <details>\n  <summary class=\"foo\">More info</summary>\n\n\
      Extra detail text.\n\n</details>\n";
    let once = fmt(src);
    assert_eq!(once, want);
    assert_eq!(fmt(&once), once, "a second pass must be a no-op");
  }

  #[test]
  fn format_blocks_leaves_unbalanced_opener_untouched() {
    if !have_prettier() {
      return;
    }
    // Alone, prettier would close the `<details>` before the body.
    let src = "<details>\n<summary>X</summary>\n\nBody.\n";
    assert_eq!(fmt(src), src);
  }

  #[test]
  fn format_blocks_keeps_crlf_line_endings() {
    if !have_prettier() {
      return;
    }
    let src = "<p align=\"center\">\r\n<img src=\"a.png\"     alt=\"b\">\r\n\
      </p>\r\n";
    let crlf = ["--end-of-line=crlf".to_string()];
    assert_eq!(
      format_blocks(src, path::Path::new("a.md"), path::Path::new("."), &crlf),
      "<p align=\"center\">\r\n  <img src=\"a.png\" alt=\"b\" />\r\n</p>\r\n"
    );
  }

  #[test]
  fn format_blocks_leaves_nested_blank_line_wrappers_untouched() {
    if !have_prettier() {
      return;
    }
    // Prettier indents each nesting level; past three spaces after a blank
    // line the wrapper would turn into an indented code block.
    let src = "<div>\n\n<div>\n\n<div>\n\n<details>\n<summary>X</summary>\n\n\
      Body.\n\n</details>\n\n</div>\n\n</div>\n\n</div>\n";
    assert_eq!(fmt(src), src);
  }
}
