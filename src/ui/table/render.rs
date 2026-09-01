//! Table rendering: the `Table` builder and the comfy-table-backed renderer.

use super::{
  Align, Cell, Column, Layout, Overflow, Palette, Row, RowKind, Span, Style,
  WidthPolicy,
};
use serde::{Deserialize, Serialize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The top-level table specification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Table {
  /// Column definitions.
  pub columns: Vec<Column>,
  /// Row contents.
  pub rows: Vec<Row>,
  /// Layout and width options.
  #[serde(default)]
  pub layout: Layout,
}

impl Table {
  /// Creates a new [`Table`] with specified columns.
  #[must_use]
  pub fn new(columns: Vec<Column>) -> Self {
    Self {
      columns,
      rows: Vec::new(),
      layout: Layout::default(),
    }
  }

  /// Adds a row to the table in place.
  pub fn add_row(&mut self, row: Row) -> &mut Self {
    self.rows.push(row);
    self
  }

  /// Sets the layout policy for the table.
  #[must_use]
  pub fn layout(mut self, layout: Layout) -> Self {
    self.layout = layout;
    self
  }

  /// Renders the table to a terminal string using the given color palette.
  #[must_use]
  pub fn render(&self, palette: &Palette) -> String {
    render(self, palette)
  }
}

fn take_prefix_by_width(text: &str, budget: usize) -> (String, usize) {
  let mut prefix = String::new();
  let mut width = 0;
  for ch in text.chars() {
    let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
    if width + ch_w > budget {
      break;
    }
    prefix.push(ch);
    width += ch_w;
  }
  (prefix, width)
}

pub(super) fn truncate_spans(
  spans: &[Span],
  max_width: usize,
  suffix: &str,
) -> Vec<Span> {
  let suffix_width = suffix.width();
  if suffix_width >= max_width {
    let (truncated_suffix, _) = take_prefix_by_width(suffix, max_width);
    return vec![Span::plain(truncated_suffix)];
  }
  let target_width = max_width - suffix_width;
  let mut current_width = 0;
  let mut result = Vec::new();

  for span in spans {
    let span_width = span.display_width();
    if current_width + span_width <= target_width {
      result.push(span.clone());
      current_width += span_width;
    } else {
      let (partial, _) = take_prefix_by_width(
        &span.text,
        target_width.saturating_sub(current_width),
      );
      if !partial.is_empty() {
        result.push(Span::new(partial, span.style));
      }
      result.push(Span::plain(suffix));
      return result;
    }
  }

  result.push(Span::plain(suffix));
  result
}

fn clip_spans(spans: &[Span], max_width: usize) -> Vec<Span> {
  let mut current_width = 0;
  let mut result = Vec::new();

  for span in spans {
    let span_width = span.display_width();
    if current_width + span_width <= max_width {
      result.push(span.clone());
      current_width += span_width;
    } else {
      let (partial, _) = take_prefix_by_width(
        &span.text,
        max_width.saturating_sub(current_width),
      );
      if !partial.is_empty() {
        result.push(Span::new(partial, span.style));
      }
      return result;
    }
  }
  result
}

fn render_cell_to_string(
  cell: &Cell,
  col_overflow: &Overflow,
  max_width_opt: Option<usize>,
  palette: &Palette,
) -> String {
  let overflow = cell.overflow.as_ref().unwrap_or(col_overflow);
  let mut buf = String::new();
  if let Some(max_w) = max_width_opt
    && cell.display_width() > max_w
  {
    match overflow {
      Overflow::Clip => {
        for span in clip_spans(&cell.spans, max_w) {
          buf.push_str(&palette.apply(&span.text, span.style));
        }
        return buf;
      }
      Overflow::Truncate { suffix } => {
        for span in truncate_spans(&cell.spans, max_w, suffix) {
          buf.push_str(&palette.apply(&span.text, span.style));
        }
        return buf;
      }
      Overflow::Wrap => {}
    }
  }

  for span in &cell.spans {
    buf.push_str(&palette.apply(&span.text, span.style));
  }
  buf
}

/// Characters after which a soft line break is allowed when wrapping a cell:
/// path separators and list punctuation. A break is also always allowed at a
/// space. Deliberately excludes `.`/`-`/`_`/`:` so `rustfmt.exe`,
/// `v1.9.0-stable`, and `C:` stay glued and remain copy/double-click friendly.
const BREAK_AFTER: [char; 4] = ['/', '\\', ',', ';'];

/// The narrowest inner width `solve_column_widths` will shrink a column to as a
/// last resort, once respecting every column's widest-token floor would push
/// the table past its width budget. At this point one token is hard-split.
const LAST_RESORT_MIN: usize = 3;

/// Splits `text` into wrap tokens: maximal runs that must not be broken across
/// lines. A trailing [`BREAK_AFTER`] char stays with its token; runs of spaces
/// each become a single `" "` token so the caller can collapse them at a wrap.
fn break_into_tokens(text: &str) -> Vec<String> {
  let mut toks = Vec::new();
  let mut cur = String::new();
  for ch in text.chars() {
    if ch == ' ' {
      if !cur.is_empty() {
        toks.push(std::mem::take(&mut cur));
      }
      toks.push(" ".to_string());
      continue;
    }
    cur.push(ch);
    if BREAK_AFTER.contains(&ch) {
      toks.push(std::mem::take(&mut cur));
    }
  }
  if !cur.is_empty() {
    toks.push(cur);
  }
  toks
}

/// Display width of the widest single token in `text` — the minimum inner
/// column width at which `text` can be laid out without splitting a token.
fn token_display_width(text: &str) -> usize {
  break_into_tokens(text)
    .iter()
    .filter(|t| t.as_str() != " ")
    .map(|t| t.as_str().width())
    .max()
    .unwrap_or(0)
}

/// Last-resort hard split of a single token genuinely wider than the column.
///
/// Reached from [`wrap_spans`] whenever a column's resolved inner width is
/// below the token's own display width — which happens for a hard-cap policy
/// (`Max` / `Range` upper / `Pct`) tighter than the token, or for any column
/// that `solve_column_widths` had to shrink past its widest-token floor to
/// keep the whole table within its width budget (`LAST_RESORT_MIN`).
fn hard_split(text: &str, width: usize) -> Vec<String> {
  let width = width.max(1);
  let mut out = Vec::new();
  let mut cur = String::new();
  let mut w = 0;
  for ch in text.chars() {
    let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
    if w + cw > width && !cur.is_empty() {
      out.push(std::mem::take(&mut cur));
      w = 0;
    }
    cur.push(ch);
    w += cw;
  }
  if !cur.is_empty() {
    out.push(cur);
  }
  out
}

/// Wrap `spans` onto lines no wider than `width`, breaking only at spaces and
/// after path separators / list punctuation so a token is never split across
/// lines. Span styles are preserved on every fragment.
pub(super) fn wrap_spans(spans: &[Span], width: usize) -> Vec<Vec<Span>> {
  let width = width.max(1);
  let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
  let mut cur_w = 0usize;

  for span in spans {
    for tok in break_into_tokens(&span.text) {
      if tok.as_str() == " " {
        if cur_w == 0 || cur_w + 1 > width {
          continue;
        }
        lines.last_mut().unwrap().push(Span::new(" ", span.style));
        cur_w += 1;
        continue;
      }

      let tw = tok.as_str().width();
      if tw > width {
        for piece in hard_split(&tok, width) {
          if cur_w > 0 {
            lines.push(Vec::new());
          }
          cur_w = piece.as_str().width();
          lines.last_mut().unwrap().push(Span::new(piece, span.style));
        }
        continue;
      }

      if cur_w + tw > width && cur_w > 0 {
        lines.push(Vec::new());
        cur_w = 0;
      }
      lines.last_mut().unwrap().push(Span::new(tok, span.style));
      cur_w += tw;
    }
  }

  // Trailing spaces left dangling by a wrap add nothing visible.
  for line in &mut lines {
    while line
      .last()
      .is_some_and(|s| !s.text.is_empty() && s.text.chars().all(|c| c == ' '))
    {
      line.pop();
    }
  }
  if lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
    lines.pop();
  }
  lines
}

/// Renders a cell to a (possibly multi-line) string already fitted to
/// `inner_width`: token-boundary wrapping for [`Overflow::Wrap`], the existing
/// clip/truncate behavior otherwise.
fn wrap_cell_content(
  cell: &Cell,
  col_overflow: &Overflow,
  inner_width: usize,
  palette: &Palette,
) -> String {
  let overflow = cell.overflow.as_ref().unwrap_or(col_overflow);
  match overflow {
    Overflow::Wrap => wrap_spans(&cell.spans, inner_width)
      .iter()
      .map(|line| {
        line
          .iter()
          .map(|s| palette.apply(&s.text, s.style))
          .collect::<String>()
      })
      .collect::<Vec<_>>()
      .join("\n"),
    Overflow::Clip | Overflow::Truncate { .. } => {
      render_cell_to_string(cell, col_overflow, Some(inner_width), palette)
    }
  }
}

/// Resolves every column to one concrete outer width (content + padding) that
/// respects the column's [`WidthPolicy`], never splits a token, and keeps the
/// total within `table_width` when the content allows it. This is the single
/// place table geometry is decided — comfy-table is then told exact widths and
/// only aligns/pads.
#[allow(clippy::too_many_lines)]
fn solve_column_widths(
  spec: &Table,
  table_width: usize,
  padding_w: usize,
) -> Vec<usize> {
  let n = spec.columns.len();
  if n == 0 {
    return Vec::new();
  }

  let mut natural = vec![0usize; n];
  let mut floor = vec![1usize; n];
  for (i, col) in spec.columns.iter().enumerate() {
    let header_text: String =
      col.header.spans.iter().map(|s| s.text.as_str()).collect();
    natural[i] = natural[i].max(header_text.as_str().width());
    floor[i] = floor[i].max(token_display_width(&header_text).max(1));
    for row in &spec.rows {
      if !matches!(row.kind, RowKind::Data) {
        continue;
      }
      if let Some(cell) = row.cells.get(i) {
        let text: String = cell.spans.iter().map(|s| s.text.as_str()).collect();
        natural[i] = natural[i].max(text.as_str().width());
        floor[i] = floor[i].max(token_display_width(&text).max(1));
      }
    }
  }

  let inner = |w: u16| (w as usize).saturating_sub(padding_w);
  let avail = table_width.saturating_sub(n * padding_w).max(n);

  let mut want = vec![0usize; n];
  let mut can_shrink = vec![false; n];
  let mut can_grow = vec![false; n];
  // The lower bound the *ordinary* shrink respects: never below a column's
  // widest token (so nothing splits) — except where the policy is a hard cap
  // (`Max` / `Range` upper / `Pct`), whose ceiling wins even over that.
  let mut soft_floor = floor.clone();
  for (i, col) in spec.columns.iter().enumerate() {
    match col.width {
      WidthPolicy::Auto => {
        want[i] = natural[i].max(1);
        can_shrink[i] = true;
        can_grow[i] = true;
      }
      WidthPolicy::Fixed(w) => {
        // "At least this wide": honor the request, and widen past it before
        // a token would have to split.
        want[i] = inner(w).max(floor[i]).max(1);
      }
      WidthPolicy::Min(w) => {
        let lo = inner(w).max(floor[i]).max(1);
        want[i] = natural[i].max(lo);
        soft_floor[i] = lo;
        can_grow[i] = true;
        can_shrink[i] = want[i] > lo;
      }
      WidthPolicy::Max(w) => {
        // Hard cap: never wider than `w`, even when a token must be split.
        let cap = inner(w).max(1);
        want[i] = natural[i].min(cap).max(1);
        soft_floor[i] = floor[i].min(cap).max(1);
        can_shrink[i] = want[i] > soft_floor[i];
      }
      WidthPolicy::Range(a, b) => {
        // Clamp content into `[a, b]`; `b` is a hard cap, so this column is
        // never grown to fill spare table width past what its content needs.
        let lo = inner(a).max(1);
        let hi = inner(b).max(lo);
        want[i] = natural[i].clamp(lo, hi);
        soft_floor[i] = floor[i].clamp(lo, hi);
        can_shrink[i] = want[i] > soft_floor[i];
      }
      WidthPolicy::Pct(p) => {
        // Hard cap at the requested fraction of the table.
        let cap = ((table_width * p as usize) / 100)
          .saturating_sub(padding_w)
          .max(1);
        want[i] = cap;
        soft_floor[i] = floor[i].min(cap).max(1);
      }
    }
  }

  let shrink = |want: &mut [usize], bound: &[usize], over: &mut usize| {
    let mut order: Vec<usize> =
      (0..n).filter(|&i| want[i] > bound[i]).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(want[i]));
    let mut progress = true;
    while *over > 0 && progress {
      progress = false;
      for &i in &order {
        if *over == 0 {
          break;
        }
        if want[i] > bound[i] {
          want[i] -= 1;
          *over -= 1;
          progress = true;
        }
      }
    }
  };

  let sum: usize = want.iter().sum();
  if sum > avail {
    let mut over = sum - avail;
    // 1: token-safe shrink of the columns that opted in; 2: token-safe shrink
    // of any column.
    let shrinkable: Vec<usize> = soft_floor
      .iter()
      .enumerate()
      .map(|(i, &f)| if can_shrink[i] { f } else { want[i] })
      .collect();
    shrink(&mut want, &shrinkable, &mut over);
    if over > 0 {
      shrink(&mut want, &soft_floor, &mut over);
    }
    // 3 (last resort): the table still overflows its budget. Let the
    // shrinkable columns go below their widest token — `wrap_spans` then
    // hard-splits it. This trades one chopped token in pathological input
    // (a single unbreakable value wider than the whole table) for staying
    // within the width budget. See #112 for capping diagnostics volume.
    if over > 0 {
      let emergency: Vec<usize> = (0..n)
        .map(|i| {
          if can_shrink[i] {
            LAST_RESORT_MIN
          } else {
            want[i]
          }
        })
        .collect();
      shrink(&mut want, &emergency, &mut over);
    }
  } else if sum < avail {
    let growers: Vec<usize> = (0..n).filter(|&i| can_grow[i]).collect();
    if !growers.is_empty() {
      let slack = avail - sum;
      let each = slack / growers.len();
      let mut rem = slack % growers.len();
      for &i in &growers {
        want[i] += each;
        if rem > 0 {
          want[i] += 1;
          rem -= 1;
        }
      }
    }
  }

  want.iter().map(|w| w + padding_w).collect()
}

fn to_comfy_align(align: Align) -> comfy_table::CellAlignment {
  match align {
    Align::Left => comfy_table::CellAlignment::Left,
    Align::Center => comfy_table::CellAlignment::Center,
    Align::Right => comfy_table::CellAlignment::Right,
  }
}

/// Render a semantic Table specification into a formatted string using comfy-table.
// Renders rich formatted tables with palette coloring, column width constraints, row spanning, and terminal clamping.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn render(spec: &Table, palette: &Palette) -> String {
  let mut table = comfy_table::Table::new();
  table.load_style(comfy_table::presets::NOTHING.header_separator(
    comfy_table::LineStyle::new('\u{2500}', '\u{2500}', '\u{2500}', '\u{2500}'),
  ));
  table.style_text_only();
  // Every column is pinned to an exact width computed by `solve_column_widths`
  // below, and cell content is pre-wrapped to that width, so comfy-table only
  // has to align and pad — never re-flow or split a token of its own accord.
  table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

  // Terminal width / max_width handling
  let mut target_width = spec.layout.max_width;
  if spec.layout.clamp_to_terminal {
    let term_width = detect_terminal_width();
    if term_width < target_width {
      target_width = term_width;
    }
  }
  let table_width =
    target_width.saturating_sub(spec.layout.indent).max(1) as usize;

  let num_cols = spec.columns.len();
  let has_headers = spec
    .columns
    .iter()
    .any(|c| c.header.spans.iter().any(|s| !s.text.is_empty()));

  let padding_w = (spec.layout.padding.0 + spec.layout.padding.1) as usize;
  // The `NOTHING` preset still reserves one space for the left border and one
  // between each pair of columns (the right border is reclaimed by
  // `trim_fmt`). Reserve that overhead so `col_widths` + borders lands inside
  // `table_width` rather than spilling `num_cols` columns past it.
  let border_overhead = num_cols;
  let budget = table_width.saturating_sub(border_overhead).max(1);
  let col_widths = solve_column_widths(spec, budget, padding_w);
  let solved_width: usize = col_widths.iter().sum::<usize>() + border_overhead;
  let render_width = solved_width.clamp(1, table_width);
  table.set_width(u16::try_from(render_width).unwrap_or(u16::MAX));

  let inner_width = |i: usize| {
    col_widths
      .get(i)
      .copied()
      .unwrap_or(0)
      .saturating_sub(padding_w)
      .max(1)
  };

  if has_headers {
    let mut header_row = comfy_table::Row::new();
    for (i, col) in spec.columns.iter().enumerate() {
      let content =
        wrap_cell_content(&col.header, &col.overflow, inner_width(i), palette);
      let cell_align = col.header.align.unwrap_or(col.align);
      let cell = comfy_table::Cell::new(content)
        .set_alignment(to_comfy_align(cell_align));
      header_row.add_cell(cell);
    }
    table.set_header(header_row);
  }

  let mut group_titles: Vec<String> = Vec::new();

  // Populate data rows
  let mut row_iter = spec.rows.iter().peekable();
  while let Some(row) = row_iter.next() {
    match &row.kind {
      RowKind::Data => {
        let mut comfy_row = comfy_table::Row::new();
        for i in 0..num_cols {
          let cell = row.cells.get(i);
          let col = spec.columns.get(i);
          let col_overflow = col.map_or(&Overflow::Wrap, |c| &c.overflow);
          let col_align = col.map_or(Align::Left, |c| c.align);

          let (mut content, align) = if let Some(c) = cell {
            (
              wrap_cell_content(c, col_overflow, inner_width(i), palette),
              c.align.unwrap_or(col_align),
            )
          } else {
            (String::new(), col_align)
          };

          if let Some(max_h) = row.max_height {
            content =
              content.lines().take(max_h).collect::<Vec<_>>().join("\n");
          }

          let comfy_cell = comfy_table::Cell::new(content)
            .set_alignment(to_comfy_align(align));
          comfy_row.add_cell(comfy_cell);
        }

        if let Some(max_h) = row.max_height {
          comfy_row.max_height(max_h);
        }

        table.add_row(comfy_row);

        // Comfortable density adds an empty spacer line after data rows
        if spec.layout.density == super::Density::Comfortable
          && row_iter
            .peek()
            .is_some_and(|next| matches!(next.kind, RowKind::Data))
        {
          let mut blank_row = comfy_table::Row::new();
          for _ in 0..num_cols {
            blank_row.add_cell(comfy_table::Cell::new(""));
          }
          table.add_row(blank_row);
        }
      }
      RowKind::Blank => {
        let mut comfy_row = comfy_table::Row::new();
        for _ in 0..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new(""));
        }
        table.add_row(comfy_row);
      }
      RowKind::Rule => {
        let mut comfy_row = comfy_table::Row::new();
        for _ in 0..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new("\u{2500}"));
        }
        table.add_row(comfy_row);
      }
      RowKind::Group(title) => {
        let mut comfy_row = comfy_table::Row::new();
        let idx = group_titles.len();
        group_titles.push(palette.apply(title, Style::Strong));
        comfy_row.add_cell(comfy_table::Cell::new(format!("_G{idx}_")));
        for _ in 1..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new(""));
        }
        table.add_row(comfy_row);
      }
    }
  }

  // Pin every column to the width `solve_column_widths` chose. Content was
  // already wrapped to exactly this width above, so comfy-table only aligns
  // and pads within it.
  for (i, col) in spec.columns.iter().enumerate() {
    if let Some(comfy_col) = table.column_mut(i) {
      comfy_col.set_padding((spec.layout.padding.0, spec.layout.padding.1));
      comfy_col.set_cell_alignment(to_comfy_align(col.align));
      // comfy-table's `Absolute(Fixed(w))` is the *total* column width, padding
      // included — which is exactly what `col_widths[i]` is.
      if let Some(&w) = col_widths.get(i) {
        comfy_col.set_constraint(comfy_table::ColumnConstraint::Absolute(
          comfy_table::Width::Fixed(u16::try_from(w).unwrap_or(u16::MAX)),
        ));
      }
    }
  }

  let formatted = table.trim_fmt();

  // Normalize every horizontal rule (`Row::rule()` rows, and comfy-table's own
  // header separator) to the width of the actual content — never wider — so a
  // rogue over-wide separator line from the renderer can't push the whole
  // block past its budget.
  let content_width = formatted
    .lines()
    .map(strip_ansi_escapes)
    .filter(|s| {
      let t = s.trim();
      !t.is_empty() && !t.chars().all(|c| c == '\u{2500}' || c == ' ')
    })
    .map(|s| unicode_width::UnicodeWidthStr::width(s.as_str()))
    .max()
    .unwrap_or(0);
  let table_width = content_width.min(render_width);
  let processed = if table_width > 0 {
    formatted
      .lines()
      .map(|line| {
        let stripped = strip_ansi_escapes(line);
        if let Some(g_idx) =
          (0..group_titles.len()).find(|&i| line.contains(&format!("_G{i}_")))
        {
          group_titles[g_idx].clone()
        } else if !stripped.is_empty()
          && stripped.chars().all(|c| c == '\u{2500}' || c == ' ')
        {
          "\u{2500}".repeat(table_width)
        } else {
          line.to_string()
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  } else {
    formatted
  };

  if spec.layout.indent > 0 {
    let indent_str = " ".repeat(spec.layout.indent as usize);
    processed
      .lines()
      .map(|line| {
        if line.trim().is_empty() {
          String::new()
        } else {
          format!("{indent_str}{line}")
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  } else {
    processed
  }
}

/// Strips ANSI CSI and OSC escape sequences from a string.
#[must_use]
pub fn strip_ansi_escapes(s: &str) -> String {
  #[derive(Copy, Clone, PartialEq, Eq)]
  enum AnsiState {
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
    EscIntermediate,
  }

  let mut result = String::with_capacity(s.len());
  let mut state = AnsiState::Normal;

  for c in s.chars() {
    match state {
      AnsiState::Normal => {
        if c == '\x1b' {
          state = AnsiState::Esc;
        } else {
          result.push(c);
        }
      }
      AnsiState::Esc => match c {
        '[' => state = AnsiState::Csi,
        ']' => state = AnsiState::Osc,
        '\x1b' => {
          state = AnsiState::Esc;
        }
        '\x20'..='\x2f' => {
          state = AnsiState::EscIntermediate;
        }
        '\x30'..='\x7e' => {
          state = AnsiState::Normal;
        }
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
      AnsiState::Csi => {
        match c {
          '\x1b' => {
            state = AnsiState::Esc;
          }
          '\x20'..='\x3f' => {
            // Parameter or intermediate byte; remain in CSI
          }
          '\x40'..='\x7e' => {
            // Final byte terminating CSI sequence
            state = AnsiState::Normal;
          }
          _ => {
            state = AnsiState::Normal;
            result.push(c);
          }
        }
      }
      AnsiState::Osc => match c {
        '\x07' => {
          state = AnsiState::Normal;
        }
        '\x1b' => {
          state = AnsiState::OscEsc;
        }
        '\n' | '\r' => {
          state = AnsiState::Normal;
          result.push(c);
        }
        _ => {}
      },
      AnsiState::OscEsc => match c {
        '\\' => {
          state = AnsiState::Normal;
        }
        '[' => {
          state = AnsiState::Csi;
        }
        ']' => {
          state = AnsiState::Osc;
        }
        '\x1b' => {}
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
      AnsiState::EscIntermediate => match c {
        '\x1b' => {
          state = AnsiState::Esc;
        }
        '\x20'..='\x2f' => {}
        '\x30'..='\x7e' => {
          state = AnsiState::Normal;
        }
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
    }
  }

  result
}

/// Measures the maximum visual character display width across all lines in a string,
/// ignoring ANSI escape codes and accounting for multi-byte/CJK Unicode width.
#[must_use]
pub fn max_line_display_width(s: &str) -> usize {
  s.lines()
    .map(|line| {
      let stripped = strip_ansi_escapes(line);
      unicode_width::UnicodeWidthStr::width(stripped.as_str())
    })
    .max()
    .unwrap_or(0)
}

/// Detects the active terminal column width (clamped to [40, 160]), defaulting to 80 when stdout is not a TTY.
#[must_use]
pub fn detect_terminal_width() -> u16 {
  use std::io::IsTerminal;
  if std::io::stdout().is_terminal()
    && let Ok((w, _)) = crossterm::terminal::size()
    && w > 0
  {
    return w.clamp(40, 160);
  }
  80
}

/// Returns a horizontal rule of exactly `width` `─` characters.
///
/// This is a dumb primitive: the one place that decides how wide a rule should
/// be is [`crate::ui::table::Frame`] (content-sized, 80-column target). The old
/// `separator_line(0)` "size to the terminal" behavior was removed with #122 so
/// two rules in one command's output can never disagree about width.
#[must_use]
pub fn separator_line(width: usize) -> String {
  "\u{2500}".repeat(width)
}

/// Returns a horizontal rule of `─` matching the widest visible line in
/// `content` (empty for empty content).
///
/// A convenience for external `ui::table` consumers who want a rule under a
/// standalone rendered table. `fml`'s own multi-section output does **not**
/// use this — it goes through [`crate::ui::table::Frame`], which decides one
/// rule width for the whole command (see #122).
#[must_use]
pub fn separator_for_content(content: &str) -> String {
  separator_line(max_line_display_width(content))
}

/// Render a JSON-encoded table specification directly into a formatted string.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if parsing `spec_json` fails.
pub fn render_json(spec_json: &str) -> Result<String, serde_json::Error> {
  let table: Table = serde_json::from_str(spec_json)?;
  let palette = Palette::detect();
  Ok(render(&table, &palette))
}
