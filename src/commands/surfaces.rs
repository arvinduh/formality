//! `fml list-surfaces` command: prints every supported language surface's
//! detection status and aliases.

use colored::Colorize;
use std::path::Path;

use crate::config::FormalityConfig;
use crate::errors::ExitStatus;
use crate::surfaces::default_registry;
use crate::ui::table;

/// Runs the `fml surfaces` command: prints every supported language surface
/// with its active/inactive status and any aliases.
pub fn run_surfaces(root: &Path, config: &FormalityConfig) -> ExitStatus {
  let registry = default_registry();
  let detected = registry.detect_surfaces_smart(root, config);
  let detected_names: Vec<&str> = detected.iter().map(|s| s.name()).collect();
  let total_count = registry.len();

  let mut surfaces_table = table::Table::new(vec![
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(12)),
    table::Column::new(table::Cell::text(""))
      .width(table::WidthPolicy::Fixed(14)),
    table::Column::new(table::Cell::text("")).width(table::WidthPolicy::Auto),
  ])
  .layout(
    table::Layout::compact()
      .indent(2)
      .padding(0, 1)
      .max_width(80),
  );

  let mut active_count = 0;
  for surface in registry.surfaces() {
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
  let frame = table::Frame::for_body(&rendered_table);

  let title = format!(
    "{} {}",
    "fml surfaces".bold().cyan(),
    format!("({total_count} supported)").dimmed()
  );
  println!("{}", frame.section(&title, &rendered_table, &palette));
  println!(
    "  {} active, {} inactive\n",
    active_count.to_string().green().bold(),
    (total_count - active_count).to_string().dimmed()
  );
  ExitStatus::Clean
}
