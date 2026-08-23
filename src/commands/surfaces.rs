use colored::Colorize;
use std::path::Path;

use crate::config::FormalityConfig;
use crate::errors::ExitStatus;
use crate::surfaces::{all_surfaces, detect_surfaces_smart};
use crate::ui::table;

/// Runs the `fml surfaces` command: prints every supported language surface
/// with its active/inactive status and any aliases.
pub fn run_surfaces(root: &Path, config: &FormalityConfig) -> ExitStatus {
  let detected = detect_surfaces_smart(root, config);
  let detected_names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

  let mut surfaces_table = table::Table::new(vec![
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(12)),
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(14)),
    table::Column::new(table::Cell::text("")).width(table::WidthPolicy::Auto),
  ])
  .layout(table::Layout::compact().indent(2).padding(0, 1));

  let mut active_count = 0;
  for surface in all_surfaces() {
    let is_detected = detected_names.contains(&surface.name());
    let (status_style, name_style, marker) = if is_detected {
      active_count += 1;
      (table::Style::Ok, table::Style::Strong, "[ACTIVE]  ")
    } else {
      (table::Style::Dim, table::Style::Dim, "[INACTIVE]")
    };

    let aliases_str = if surface.aliases().is_empty() {
      String::new()
    } else {
      format!("aliases: {}", surface.aliases().join(", "))
    };

    surfaces_table.add_row(table::Row::new(vec![
      table::Cell::styled(marker, status_style),
      table::Cell::styled(surface.name(), name_style),
      table::Cell::styled(aliases_str, table::Style::Dim),
    ]));
  }

  let palette = table::Palette::detect();
  let rendered_table = table::render(&surfaces_table, &palette);
  let separator = table::separator_for_content(&rendered_table);

  println!(
    "{} {}",
    "fml surfaces".bold().cyan(),
    format!("({} supported)", all_surfaces().len()).dimmed()
  );
  println!("{}", separator.dimmed());
  if !rendered_table.is_empty() {
    println!("{rendered_table}");
  }
  println!("{}", separator.dimmed());
  println!(
    "  {} active, {} inactive\n",
    active_count.to_string().green().bold(),
    (all_surfaces().len() - active_count).to_string().dimmed()
  );
  ExitStatus::Clean
}
