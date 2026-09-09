//! Schema drift detection test.
//!
//! To regenerate the committed schema when configuration structures change, run:
//! ```bash
//! UPDATE_SCHEMA=1 cargo test -j 2 --test schema_drift
//! ```

use fml::generate_schema;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_schema_drift_check() {
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let schema_path = root.join("schema").join("formality.schema.json");

  let generated_schema = generate_schema();

  if std::env::var("UPDATE_SCHEMA").as_deref() == Ok("1") {
    if let Some(parent) = schema_path.parent() {
      let _ = fs::create_dir_all(parent);
    }
    fs::write(&schema_path, format!("{generated_schema}\n"))
      .expect("Failed to write schema/formality.schema.json");
  }

  assert!(
    schema_path.exists(),
    "schema/formality.schema.json must exist in repository root. Run 'UPDATE_SCHEMA=1 cargo test -j 2 --test schema_drift' to generate it."
  );

  let committed_schema = fs::read_to_string(&schema_path)
    .expect("Failed to read schema/formality.schema.json");

  let committed_val: serde_json::Value =
    serde_json::from_str(&committed_schema)
      .expect("Committed schema should be valid JSON");
  let generated_val: serde_json::Value =
    serde_json::from_str(&generated_schema)
      .expect("Generated schema should be valid JSON");

  assert_eq!(
    committed_val, generated_val,
    "Schema drift detected! The committed schema/formality.schema.json does not match generate_schema(). \
     Please run 'UPDATE_SCHEMA=1 cargo test -j 2 --test schema_drift' to update it."
  );
}
