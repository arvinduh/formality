use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct TaxonomyData {
  non_code_extensions: Vec<String>,
  non_code_filenames: Vec<String>,
}

struct ParsedTaxonomy {
  non_code_extensions: HashSet<String>,
  non_code_filenames: HashSet<String>,
}

static TAXONOMY: LazyLock<ParsedTaxonomy> = LazyLock::new(|| {
  let raw: TaxonomyData = serde_json::from_str(include_str!("taxonomy.json"))
    .expect("Invalid embedded taxonomy.json");
  ParsedTaxonomy {
    non_code_extensions: raw
      .non_code_extensions
      .into_iter()
      .map(|s| s.to_lowercase())
      .collect(),
    non_code_filenames: raw
      .non_code_filenames
      .into_iter()
      .map(|s| s.to_lowercase())
      .collect(),
  }
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCategory {
  /// Bucket B: Supported language surface (e.g. "rust", "python", "cpp", "markdown", etc.)
  Supported(&'static str),
  /// Bucket A: Non-code file, asset, lockfile, binary, or extensionless metadata file.
  NonCode,
  /// Bucket C: Any remaining extension representing an unsupported language (e.g. "go", "ts", "java").
  UnsupportedLanguage(String),
}

/// Returns the surface name if the extension belongs to a built-in supported surface.
pub fn get_supported_surface_for_ext(ext: &str) -> Option<&'static str> {
  let lower = ext.to_lowercase();
  crate::surfaces::all_surfaces().into_iter().find_map(|s| {
    if s.extensions().iter().any(|&e| e.eq_ignore_ascii_case(&lower)) {
      Some(s.name())
    } else {
      None
    }
  })
}

/// Returns true if the extension is in the known non-code / asset / lockfile list.
pub fn is_non_code_extension(ext: &str) -> bool {
  let lower = ext.to_lowercase();
  TAXONOMY.non_code_extensions.contains(&lower)
}

/// Classifies a file path into Bucket A (NonCode), Bucket B (Supported), or Bucket C (UnsupportedLanguage).
pub fn classify_file(path: &Path) -> FileCategory {
  let file_name = path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or_default();

  // Check if entire filename matches known extensionless or metadata file
  let file_name_lower = file_name.to_lowercase();
  if TAXONOMY.non_code_filenames.contains(&file_name_lower) {
    return FileCategory::NonCode;
  }

  // Check if file starts with a dot and has no further extension (e.g. .gitignore, .dockerignore)
  if file_name.starts_with('.') && !file_name[1..].contains('.') {
    return FileCategory::NonCode;
  }

  let ext = match path.extension().and_then(|e| e.to_str()) {
    Some(e) if !e.is_empty() => e,
    _ => return FileCategory::NonCode, // Extensionless -> Bucket A
  };

  if let Some(surface) = get_supported_surface_for_ext(ext) {
    FileCategory::Supported(surface)
  } else if is_non_code_extension(ext) {
    FileCategory::NonCode
  } else {
    FileCategory::UnsupportedLanguage(ext.to_lowercase())
  }
}

/// Scans the workspace directory for unsupported language files (Bucket C),
/// respecting .gitignore and ignoring target, node_modules, .git, .venv, vendor, fixtures.
/// Returns a map of extension -> file count.
pub fn scan_unsupported_workspace_extensions(
  root: &Path,
) -> BTreeMap<String, usize> {
  let mut counts = BTreeMap::new();
  let walker = ignore::WalkBuilder::new(root)
    .hidden(false)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .filter_entry(|entry| {
      let name = entry.file_name().to_string_lossy();
      name != "target"
        && name != "node_modules"
        && name != ".git"
        && name != ".venv"
        && name != "vendor"
        && name != "fixtures"
    })
    .build();

  for entry in walker.filter_map(Result::ok) {
    let path = entry.path();
    if path.is_file() {
      if let FileCategory::UnsupportedLanguage(ext) = classify_file(path) {
        *counts.entry(ext).or_insert(0) += 1;
      }
    }
  }

  counts
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_classify_supported_files() {
    assert_eq!(
      classify_file(Path::new("src/main.rs")),
      FileCategory::Supported("rust")
    );
    assert_eq!(
      classify_file(Path::new("script.py")),
      FileCategory::Supported("python")
    );
    assert_eq!(
      classify_file(Path::new("doc.md")),
      FileCategory::Supported("markdown")
    );
    assert_eq!(
      classify_file(Path::new("config.toml")),
      FileCategory::Supported("toml")
    );
    assert_eq!(
      classify_file(Path::new("data.json")),
      FileCategory::Supported("json")
    );
    assert_eq!(
      classify_file(Path::new("paper.typ")),
      FileCategory::Supported("typst")
    );
  }

  #[test]
  fn test_classify_non_code_files() {
    assert_eq!(
      classify_file(Path::new("LICENSE")),
      FileCategory::NonCode
    );
    assert_eq!(
      classify_file(Path::new("Makefile")),
      FileCategory::NonCode
    );
    assert_eq!(
      classify_file(Path::new(".gitignore")),
      FileCategory::NonCode
    );
    assert_eq!(
      classify_file(Path::new("notes.txt")),
      FileCategory::NonCode
    );
    assert_eq!(
      classify_file(Path::new("Cargo.lock")),
      FileCategory::NonCode
    );
    assert_eq!(
      classify_file(Path::new("image.png")),
      FileCategory::NonCode
    );
  }

  #[test]
  fn test_no_overlap_between_supported_and_non_code() {
    for surface in crate::surfaces::all_surfaces() {
      for &ext in surface.extensions() {
        assert!(
          !is_non_code_extension(ext),
          "Supported extension '{}' for surface '{}' should not be in non_code_extensions",
          ext,
          surface.name()
        );
        assert_eq!(
          get_supported_surface_for_ext(ext),
          Some(surface.name()),
          "Supported extension '{}' must dynamically map to surface '{}'",
          ext,
          surface.name()
        );
      }
    }
  }

  #[test]
  fn test_planned_unsupported_languages_are_classified_as_unsupported() {
    let planned_exts = [
      "go", "ts", "tsx", "js", "jsx", "java", "kt", "kts", "swift", "rb",
      "cs", "zig", "lua", "php", "dart", "scala", "ex", "exs", "html", "css",
      "scss", "sql", "sh", "bash", "zsh", "hs",
    ];

    for ext in planned_exts {
      assert!(
        get_supported_surface_for_ext(ext).is_none(),
        "Planned extension '{}' should not yet be a supported surface",
        ext
      );
      assert!(
        !is_non_code_extension(ext),
        "Planned language extension '{}' should not be classified as non-code",
        ext
      );
      let filename = format!("test.{}", ext);
      assert_eq!(
        classify_file(Path::new(&filename)),
        FileCategory::UnsupportedLanguage(ext.to_string()),
        "Planned extension '{}' must classify as UnsupportedLanguage",
        ext
      );
    }
  }
}
