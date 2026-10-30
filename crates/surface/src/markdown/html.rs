//! Block-level HTML inside Markdown: finds it, formats it with prettier's
//! html parser and splices it back.
//!
//! Prettier's markdown parser keeps an HTML block byte for byte, so a
//! centered badge block or a `<details>` section stays as written. This pass
//! formats those blocks. Inline HTML inside a paragraph is never touched:
//! whitespace around an inline element renders.

use std::ffi;
use std::path;

use pulldown_cmark;

use crate::prettier;

/// HTML void elements (WHATWG), which never take a closing tag.
const VOID_ELEMENTS: &[&str] = &[
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
  "param", "source", "track", "wbr",
];

/// The start of every gap marker. A file already holding one is left as
/// written, since its own copy could split the batch in the wrong place.
const GAP: &str = "<!--fml-html-gap-";

/// The marker line between the `gap`th block of a batch and the next.
fn marker(gap: usize) -> String {
  format!("{GAP}{gap}-->")
}

/// One tag, reduced to what [`balanced`] tracks.
struct Tag {
  /// The lowercased tag name.
  name: String,
  /// The tag is a closing tag (`</p>`).
  closing: bool,
  /// The tag closes itself (`<br/>`).
  self_closing: bool,
}

/// Formats every top-level HTML block in `src`, the Markdown file `file`,
/// with prettier `args` run from `cwd`, leaving every other byte as written.
///
/// All blocks go to one prettier run, joined by gap marker lines, so an
/// opener and its closer in separate blocks form one balanced document while
/// the Markdown between them is spliced back verbatim. Returns `src` as it
/// is whenever the result is in doubt: unbalanced tags, prettier rejecting
/// the HTML, a reflowed marker, or a result with a different block
/// structure.
///
/// # Errors
///
/// Returns an error when prettier is missing or cannot start.
pub fn format(
  src: &str,
  file: &path::Path,
  cwd: &path::Path,
  args: &[String],
) -> Result<String, crate::Error> {
  let spans = blocks(src);
  if spans.is_empty() || src.contains(GAP) || !balanced(src, &spans) {
    return Ok(src.to_owned());
  }
  let mut batch = String::new();
  for (index, &(start, end)) in spans.iter().enumerate() {
    if index > 0 {
      batch.push_str(&marker(index - 1));
      batch.push('\n');
    }
    batch.push_str(&src[start..end]);
    if !batch.ends_with('\n') {
      batch.push('\n');
    }
  }
  // `css` whitespace sensitivity, prettier's default, is pinned: it keeps
  // the space between inline elements, which renders.
  let fixed = [
    "--parser=html",
    "--html-whitespace-sensitivity=css",
    "--stdin-filepath",
  ];
  let args: Vec<&ffi::OsStr> = fixed
    .iter()
    .map(ffi::OsStr::new)
    .chain([file.as_os_str()])
    .chain(args.iter().map(ffi::OsStr::new))
    .collect();
  let output = prettier::PRETTIER.pipe(&args, cwd, &batch)?;
  if output.code != Some(0) {
    return Ok(src.to_owned());
  }
  let Some(parts) = split(&output.stdout, spans.len() - 1) else {
    return Ok(src.to_owned());
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
  if structure(&out) == structure(src) {
    Ok(out)
  } else {
    Ok(src.to_owned())
  }
}

/// Parses `src` with the extensions prettier's markdown parser honours, so
/// a table or footnote next to HTML parses as prettier sees it, and HTML in
/// front matter is never a block.
pub fn parse(src: &str) -> pulldown_cmark::Parser<'_> {
  let options = pulldown_cmark::Options::ENABLE_TABLES
    | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
    | pulldown_cmark::Options::ENABLE_FOOTNOTES
    | pulldown_cmark::Options::ENABLE_TASKLISTS
    | pulldown_cmark::Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
    | pulldown_cmark::Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
  pulldown_cmark::Parser::new_ext(src, options)
}

/// The byte ranges of the top-level HTML blocks in `src` this pass may
/// format, in order.
///
/// Skipped: a block inside a container (the html parser would read its `> `
/// prefixes as text), a block of only comments, and every block prettier is
/// told to ignore by a `prettier-ignore` comment or range.
fn blocks(src: &str) -> Vec<(usize, usize)> {
  let mut spans = Vec::new();
  let mut depth = 0usize;
  let (mut ignore_next, mut ignore_range) = (false, false);
  for (event, range) in parse(src).into_offset_iter() {
    match event {
      pulldown_cmark::Event::Start(tag) => {
        if depth == 0 {
          let html = tag == pulldown_cmark::Tag::HtmlBlock;
          let block = &src[range.clone()];
          let mut skip = std::mem::take(&mut ignore_next)
            || ignore_range
            || (html && comments_only(block));
          // Without blank lines a directive shares its neighbour's block, so
          // every comment in the block counts.
          let mut rest = if html { block } else { "" };
          while let Some(open) = rest.find("<!--")
            && let Some((directive, after)) = comment(&rest[open..])
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
          if html && !skip {
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

/// Splits `block` into its leading comment's trimmed text and the text after
/// it, or `None` when `block` does not open with a comment.
fn comment(block: &str) -> Option<(&str, &str)> {
  let rest = block.trim_start().strip_prefix("<!--")?;
  let end = rest.find("-->")?;
  Some((rest[..end].trim(), &rest[end + 3..]))
}

/// Whether `block` holds nothing but comments and whitespace.
fn comments_only(mut block: &str) -> bool {
  while let Some((_, rest)) = comment(block) {
    block = rest;
  }
  block.trim().is_empty()
}

/// Whether the tags across `spans`, read in order as one stack, balance.
///
/// A blank line ends an HTML block, so a `<details>` and its `</details>`
/// can arrive as two spans with Markdown between them. Prettier repairs a
/// lone opener by inserting a closer, so the spans are formatted together,
/// and only when they balance.
fn balanced(src: &str, spans: &[(usize, usize)]) -> bool {
  let mut stack = Vec::new();
  for &(start, end) in spans {
    let Some(tags) = tags(&src[start..end]) else {
      return false;
    };
    for tag in tags {
      if tag.self_closing
        || (!tag.closing && VOID_ELEMENTS.contains(&tag.name.as_str()))
      {
        continue;
      }
      if !tag.closing {
        stack.push(tag.name);
      } else if stack.pop() != Some(tag.name) {
        return false;
      }
    }
  }
  stack.is_empty()
}

/// The tags in an HTML fragment, or `None` when a tag or comment never ends.
///
/// Not an HTML parser: it honours quoted attribute values and skips comments
/// and declarations, but knows no raw-text elements such as `<script>`.
/// Anything it misreads unbalances the stack, which leaves the file as
/// written.
fn tags(html: &str) -> Option<Vec<Tag>> {
  let mut tags = Vec::new();
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
    let (closing, body) = body
      .strip_prefix('/')
      .map_or((false, body), |body| (true, body));
    if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
      // A bare `<` in text, not a tag.
      rest = &rest[1..];
      continue;
    }
    let name = body
      .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
      .unwrap_or(body.len());
    let attributes = &body[name..];
    let end = tag_end(attributes)?;
    tags.push(Tag {
      name: body[..name].to_ascii_lowercase(),
      closing,
      self_closing: attributes[..end].trim_end().ends_with('/'),
    });
    rest = &attributes[end + 1..];
  }
  Some(tags)
}

/// The offset of the `>` that ends a tag whose attributes start
/// `attributes`, skipping quoted values, or `None` when the tag never ends.
fn tag_end(attributes: &str) -> Option<usize> {
  let mut quote = None;
  for (at, c) in attributes.char_indices() {
    match (quote, c) {
      (Some(open), _) if c == open => quote = None,
      (None, '"' | '\'') => quote = Some(c),
      (None, '>') => return Some(at),
      _ => {}
    }
  }
  None
}

/// Splits prettier's output for a batch back into one part per block, or
/// `None` when a marker is missing or no longer alone on its line.
fn split(formatted: &str, gaps: usize) -> Option<Vec<&str>> {
  let mut parts = Vec::with_capacity(gaps + 1);
  let mut rest = formatted;
  for gap in 0..gaps {
    let marker = marker(gap);
    let at = rest.find(&marker)?;
    let line_start = rest[..at].rfind('\n').map_or(0, |at| at + 1);
    let after = at + marker.len();
    let line_end = rest[after..]
      .find('\n')
      .map_or(rest.len(), |end| after + end + 1);
    if !rest[line_start..at].trim().is_empty()
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

/// The block and span nesting of `src`, equal for two documents exactly
/// when they nest the same blocks and spans.
fn structure(src: &str) -> Vec<(bool, pulldown_cmark::TagEnd)> {
  parse(src)
    .filter_map(|event| match event {
      pulldown_cmark::Event::Start(tag) => Some((true, tag.to_end())),
      pulldown_cmark::Event::End(tag) => Some((false, tag)),
      _ => None,
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn blocks_split_by_markdown_balance_together() {
    let src = "<details>\n<summary>X</summary>\n\n*text*\n\n</details>\n";
    let spans = blocks(src);
    assert_eq!(spans.len(), 2, "a blank line ends each block");
    assert!(balanced(src, &spans));
    assert!(!balanced(src, &spans[..1]), "a lone opener does not");
  }

  #[test]
  fn ignored_and_nested_blocks_are_skipped() {
    let src = "<!-- prettier-ignore -->\n<p>a</p>\n\n> <p>b</p>\n\n<p>c</p>\n";
    let spans = blocks(src);
    assert_eq!(spans.len(), 1);
    assert_eq!(&src[spans[0].0..spans[0].1], "<p>c</p>\n");
  }
}
