use super::*;

#[test]
fn test_span_width_calculation_unicode_cjk() {
  let ascii_span = Span::plain("hello");
  assert_eq!(ascii_span.display_width(), 5);

  // CJK characters have display width 2 each
  let cjk_span = Span::plain("\u{4f60}\u{597d}\u{4e16}\u{754c}");
  assert_eq!(cjk_span.display_width(), 8);

  // Mixed ASCII, emoji, and CJK
  let mixed_span = Span::plain("Rust \u{1f980} \u{7f16}\u{7a0b}");
  // "Rust " = 5, emoji = 2, " " = 1, Chinese = 4 => 12
  assert_eq!(mixed_span.display_width(), 12);

  let cell = Cell::new(vec![
    Span::styled("Status: ", Style::Strong),
    Span::styled("\u{6210}\u{529f}", Style::Ok),
    Span::plain(" (OK)"),
  ]);
  // "Status: " = 8, Chinese = 4, " (OK)" = 5 => 17
  assert_eq!(cell.display_width(), 17);
}

#[test]
fn test_palette_styling_none_vs_ansi16_vs_truecolor() {
  let none_pal = Palette::none();
  assert_eq!(none_pal.mode(), PaletteMode::None);

  let styles = [
    Style::Plain,
    Style::Dim,
    Style::Strong,
    Style::Path,
    Style::Tool,
    Style::Ok,
    Style::Warn,
    Style::Error,
    Style::Info,
  ];

  for s in styles {
    let applied = none_pal.apply("text", s);
    assert_eq!(
      applied, "text",
      "Style {s:?} should have no SGR in None mode"
    );
  }

  let ansi_pal = Palette::ansi16();
  assert_eq!(ansi_pal.mode(), PaletteMode::Ansi16);
  let ok_ansi = ansi_pal.apply("PASS", Style::Ok);
  assert!(ok_ansi.starts_with("\x1b[1;32m"));
  assert!(ok_ansi.ends_with("\x1b[0m"));

  let err_ansi = ansi_pal.apply("FAIL", Style::Error);
  assert!(err_ansi.starts_with("\x1b[1;31m"));

  let tc_pal = Palette::truecolor();
  assert_eq!(tc_pal.mode(), PaletteMode::Truecolor);
  let ok_tc = tc_pal.apply("PASS", Style::Ok);
  assert!(ok_tc.contains("38;2;80;200;120m"));
  assert!(ok_tc.ends_with("\x1b[0m"));

  let path_tc = tc_pal.apply("src/table.rs", Style::Path);
  assert!(path_tc.contains("38;2;100;180;240m"));
}

#[test]
fn test_wrapping_and_multiline_spans() {
  let mut table = Table::new(vec![
    Column::new("Col 1").width(WidthPolicy::Fixed(12)).overflow(
      Overflow::Truncate {
        suffix: "...".to_string(),
      },
    ),
    Column::new("Col 2")
      .width(WidthPolicy::Fixed(12))
      .overflow(Overflow::Clip),
    Column::new("Col 3")
      .width(WidthPolicy::Fixed(16))
      .overflow(Overflow::Wrap),
  ]);

  table.add_row(Row::new(vec![
    Cell::text("A very long string that needs truncation"),
    Cell::text("A very long string that needs clipping"),
    Cell::text("Word wrap test"),
  ]));

  table.add_row(Row::blank());
  table.add_row(Row::rule());
  table.add_row(Row::group("Summary"));

  let pal = Palette::none();
  let rendered = render(&table, &pal);

  assert!(rendered.contains("Col 1"));
  assert!(rendered.contains("Col 2"));
  assert!(rendered.contains("Col 3"));
  assert!(rendered.contains("..."));
  assert!(rendered.contains("Summary"));
  assert!(rendered.contains('\u{2500}'));
}

#[test]
fn test_table_serialization_deserialization_json() {
  let table = Table::new(vec![
    Column::new("Surface")
      .align(Align::Left)
      .width(WidthPolicy::Fixed(12)),
    Column::new("Status")
      .align(Align::Center)
      .width(WidthPolicy::Fixed(10)),
    Column::new("Count")
      .align(Align::Right)
      .width(WidthPolicy::Fixed(8)),
  ])
  .layout(Layout::compact().indent(2))
  .with_row(Row::new(vec![
    Cell::styled("rust", Style::Tool),
    Cell::styled("PASS", Style::Ok),
    Cell::text("42"),
  ]))
  .with_row(Row::rule())
  .with_row(Row::group("Totals"))
  .with_row(Row::new(vec![
    Cell::text("Total"),
    Cell::text(""),
    Cell::styled("42", Style::Strong),
  ]));

  let json_str = serde_json::to_string_pretty(&table)
    .expect("Failed to serialize table to JSON");
  let deserialized: Table = serde_json::from_str(&json_str)
    .expect("Failed to deserialize table from JSON");

  assert_eq!(table, deserialized);

  let rendered_direct = render(&table, &Palette::none());
  let rendered_json =
    render_json(&json_str).expect("Failed to render json table");
  // With detect or none mode, the layout structure matches
  assert!(rendered_json.contains("Surface"));
  assert!(rendered_json.contains("rust"));
  assert!(rendered_json.contains("42"));
  assert_eq!(
    rendered_direct.lines().count(),
    rendered_json.lines().count()
  );
}

#[test]
fn test_doctor_table_rendering_consistency() {
  let mut table = Table::new(vec![
    Column::new("Status").width(WidthPolicy::Fixed(10)),
    Column::new("Tool").width(WidthPolicy::Fixed(14)),
    Column::new("Surface").width(WidthPolicy::Fixed(10)),
    Column::new("Details").width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2));

  table.add_row(Row::new(vec![
    Cell::styled("[READY]", Style::Ok),
    Cell::styled("rustfmt", Style::Tool),
    Cell::styled("rust", Style::Dim),
    Cell::new(vec![
      Span::styled("/usr/bin/rustfmt", Style::Dim),
      Span::styled(" (v1.8.0)", Style::Info),
    ]),
  ]));

  table.add_row(Row::new(vec![
    Cell::styled("[WARN] ", Style::Warn),
    Cell::styled("clippy", Style::Warn),
    Cell::styled("rust", Style::Dim),
    Cell::styled(" (v1.70.0 < MSTV v1.75.0)", Style::Warn),
  ]));

  table.add_row(Row::new(vec![
    Cell::styled("[MISS] ", Style::Warn),
    Cell::styled("ruff", Style::Warn),
    Cell::styled("python", Style::Dim),
    Cell::styled(
      "An extremely fast Python linter and code formatter",
      Style::Dim,
    ),
  ]));

  let pal_none = Palette::none();
  let rendered_none = render(&table, &pal_none);

  assert!(rendered_none.contains("[READY]"));
  assert!(rendered_none.contains("rustfmt"));
  assert!(rendered_none.contains("[WARN]"));
  assert!(rendered_none.contains("clippy"));
  assert!(rendered_none.contains("[MISS]"));
  assert!(rendered_none.contains("ruff"));
  assert!(!rendered_none.contains("\x1b["));

  let pal_tc = Palette::truecolor();
  let rendered_tc = render(&table, &pal_tc);
  assert!(rendered_tc.contains("\x1b["));
  assert!(rendered_tc.contains("38;2;80;200;120m")); // Ok color
}

#[test]
fn test_strip_ansi_escapes_removes_sgr_codes_only() {
  let styled = "\x1b[1;32mPASS\x1b[0m plain \x1b[38;2;80;150;240mtext\x1b[0m";
  assert_eq!(strip_ansi_escapes(styled), "PASS plain text");

  // Text with no escapes at all is returned unchanged.
  assert_eq!(strip_ansi_escapes("no escapes here"), "no escapes here");

  // The stripper is a simple state machine, not a full ANSI parser: once it
  // sees ESC it swallows everything up to and including the *next* literal
  // 'm' character, wherever that occurs — including inside plain text after
  // an unterminated escape (here, the 'm' in "unterminated" itself closes
  // the escape state). It must not panic or loop forever either way.
  assert_eq!(
    strip_ansi_escapes("before\x1b[unterminated"),
    "beforeinated"
  );
}

#[test]
fn test_max_line_display_width_ignores_ansi_and_counts_cjk() {
  let ansi_pal = Palette::ansi16();
  let colored = ansi_pal.apply("PASS", Style::Ok);
  // Display width must be measured on the visible text (4), not the
  // escape-code-inflated byte length.
  assert_eq!(max_line_display_width(&colored), 4);

  let multiline = "short\nlonger line\n\u{4f60}\u{597d}"; // CJK pair = width 4
  assert_eq!(max_line_display_width(multiline), 11); // "longer line"

  assert_eq!(max_line_display_width(""), 0);
}

#[test]
fn test_separator_line_explicit_width_vs_zero_fallback() {
  let explicit = separator_line(10);
  assert_eq!(explicit.chars().count(), 10);
  assert!(explicit.chars().all(|c| c == '\u{2500}'));

  // width == 0 falls back to detect_terminal_width(), which is always
  // clamped into [40, 160] regardless of environment.
  let fallback = separator_line(0);
  assert!(fallback.chars().count() >= 40);
  assert!(fallback.chars().count() <= 160);
}

#[test]
fn test_render_json_invalid_json_returns_error() {
  let result = render_json("{ not valid json");
  assert!(result.is_err());

  let result_wrong_shape = render_json(r#"{"columns": "should be an array"}"#);
  assert!(result_wrong_shape.is_err());
}

#[test]
fn test_width_policy_min_max_range_pct_all_render() {
  // Only WidthPolicy::Fixed had rendering coverage; Min/Max/Range/Pct map
  // to distinct comfy_table::ColumnConstraint arms that were previously
  // untested and could silently regress (e.g. swapped Min/Max) without any
  // test failing.
  let mut table = Table::new(vec![
    Column::new("Min").width(WidthPolicy::Min(6)),
    Column::new("Max").width(WidthPolicy::Max(20)),
    Column::new("Range").width(WidthPolicy::Range(4, 30)),
    Column::new("Pct").width(WidthPolicy::Pct(25)),
  ]);
  table.add_row(Row::new(vec![
    Cell::text("a"),
    Cell::text("bb"),
    Cell::text("ccc"),
    Cell::text("dddd"),
  ]));

  let rendered = render(&table, &Palette::none());
  assert!(rendered.contains("Min"));
  assert!(rendered.contains("Max"));
  assert!(rendered.contains("Range"));
  assert!(rendered.contains("Pct"));
  assert!(!rendered.is_empty());
}

#[test]
fn test_align_center_applies_to_header_and_cell() {
  let mut table = Table::new(vec![
    Column::new("Centered")
      .align(Align::Center)
      .width(WidthPolicy::Fixed(20)),
  ]);
  table.add_row(Row::new(vec![Cell::text("mid")]));

  let rendered = render(&table, &Palette::none());
  // comfy-table pads centered content with leading spaces on both sides;
  // assert the value is present and the line is not left-flush (i.e. some
  // leading whitespace precedes it), distinguishing it from Align::Left.
  let content_line = rendered
    .lines()
    .find(|l| l.contains("mid"))
    .expect("row must render");
  assert!(content_line.starts_with(' '));
}

#[test]
fn test_cell_level_overflow_overrides_column_overflow() {
  // A Cell's own `overflow` must take precedence over its Column's
  // overflow policy — this per-cell override path had no coverage.
  let mut table = Table::new(vec![
    Column::new("Col").width(WidthPolicy::Fixed(10)).overflow(
      Overflow::Truncate {
        suffix: "...".to_string(),
      },
    ),
  ]);
  table.add_row(Row::new(vec![
    Cell::text("this text is definitely too long")
      .overflow(Overflow::Clip),
  ]));

  let rendered = render(&table, &Palette::none());
  // Clip must not introduce the column's "..." suffix.
  assert!(!rendered.contains("..."));
}

#[test]
fn test_truncate_suffix_wider_than_column_width_does_not_panic() {
  // Edge case: the truncation suffix itself is wider than the available
  // column width (after padding is subtracted). The internal
  // `truncate_spans` helper must degrade gracefully (truncate the suffix
  // itself) instead of underflowing `max_width - suffix_width` and
  // panicking. Exercised through the public render() API since
  // truncate_spans is a private implementation detail of this module.
  let mut table = Table::new(vec![
    Column::new("Col").width(WidthPolicy::Fixed(3)).overflow(
      Overflow::Truncate {
        suffix: "...".to_string(),
      },
    ),
  ]);
  table.add_row(Row::new(vec![Cell::text("hello world")]));

  // Must not panic; a bounded, well-formed string comes out.
  let rendered = render(&table, &Palette::none());
  assert!(!rendered.is_empty());
}

#[test]
fn test_layout_comfortable_and_density_variant() {
  let layout = Layout::comfortable();
  assert_eq!(layout.density, Density::Comfortable);

  let mut table =
    Table::new(vec![Column::new("A"), Column::new("B")]).layout(layout);
  table.add_row(Row::new(vec![Cell::text("x"), Cell::text("y")]));

  let rendered = render(&table, &Palette::none());
  assert!(rendered.contains('x'));
  assert!(rendered.contains('y'));
}

#[test]
fn test_row_with_fewer_cells_than_columns_renders_blank_gaps() {
  // A Row shorter than the table's column count must render an empty cell
  // for the missing columns rather than panicking on an out-of-bounds
  // index (row.cells.get(i) returning None).
  let table = Table::new(vec![
    Column::new("A"),
    Column::new("B"),
    Column::new("C"),
  ])
  .with_row(Row::new(vec![Cell::text("only-a")]));

  let rendered = render(&table, &Palette::none());
  assert!(rendered.contains("only-a"));
}

#[test]
fn test_dynamic_separator_and_width_alignment() {
  let mut table = Table::new(vec![
    Column::new("Status").width(WidthPolicy::Fixed(8)),
    Column::new("Surface").width(WidthPolicy::Fixed(14)),
    Column::new("Details").width(WidthPolicy::Fixed(24)),
  ])
  .layout(Layout::compact());

  table.add_row(Row::new(vec![
    Cell::styled("[PASS]", Style::Ok),
    Cell::styled("rust", Style::Tool),
    Cell::styled("Clean / Formatted", Style::Dim),
  ]));
  table.add_row(Row::rule());
  table.add_row(Row::new(vec![
    Cell::styled("[FAIL]", Style::Error),
    Cell::styled("python", Style::Tool),
    Cell::styled("Violations found", Style::Error),
  ]));

  let rendered = render(&table, &Palette::none());
  let max_w = max_line_display_width(&rendered);
  let sep = separator_for_content(&rendered);

  assert_eq!(sep.chars().count(), max_w);
  assert!(sep.chars().all(|c| c == '\u{2500}'));

  // Verify the rule line inside the table also spans the table width
  let rule_line = rendered
    .lines()
    .find(|l| l.chars().all(|c| c == '\u{2500}'))
    .expect("Must have a rule line");
  assert_eq!(rule_line.chars().count(), max_w);
}
