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
