use super::*;
use std::fs;
use std::path::Path;

#[test]
fn test_default_resolution() {
  let cfg = FormalityConfig::with_defaults();
  let global = cfg.resolve_global();
  assert_eq!(global.indent_size, 2);
  assert_eq!(global.line_length, 80);
  assert_eq!(global.end_of_line, "lf");
  assert_eq!(global.layout.indent_size, Some(2));
  assert_eq!(global.layout.line_length, Some(80));
  assert_eq!(global.layout.use_tabs, Some(false));

  let rust = cfg.resolve_for_lang("rust");
  assert_eq!(rust.indent_size, 2);
  assert_eq!(rust.line_length, 80);
  assert_eq!(rust.format_tool.as_deref(), Some("cargo-fmt"));
  assert_eq!(rust.lint_tool.as_deref(), Some("clippy"));
  assert!(rust.enabled);
  assert_eq!(rust.layout.indent_size, Some(2));
  assert_eq!(rust.layout.line_length, Some(80));
  assert_eq!(rust.rust, Some(RustOptions::default()));

  let json = cfg.resolve_for_lang("json");
  assert_eq!(json.format_tool.as_deref(), Some("prettier"));
}

#[test]
fn test_find_project_config_candidates() {
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();

  // No config initially
  assert_eq!(find_project_config(root), None);

  // Test .formality.toml
  let hidden = root.join(".formality.toml");
  fs::write(&hidden, "[global]\nindent_size = 4\n").unwrap();
  assert_eq!(find_project_config(root), Some(hidden.clone()));

  // Test formality.toml (higher precedence than .formality.toml)
  let standard = root.join("formality.toml");
  fs::write(&standard, "[global]\nindent_size = 2\n").unwrap();
  assert_eq!(find_project_config(root), Some(standard));
}

#[test]
fn test_languages_list_parsing() {
  let toml = r#"
      [global]
      languages = ["rust", "toml"]
      ignore_languages = ["cpp"]
      indent_size = 4
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
  let global = parsed.resolve_global();
  assert_eq!(
    global.languages,
    Some(vec!["rust".to_string(), "toml".to_string()])
  );
  assert_eq!(global.ignore_languages, Some(vec!["cpp".to_string()]));
  assert_eq!(global.indent_size, 4);
}

#[test]
fn test_merge_and_override() {
  let mut base = FormalityConfig::with_defaults();

  let override_toml = r#"
            [global]
            indent_size = 4
            line_length = 100

            [lang.markdown]
            indent_size = 2
            prose_wrap = "always"
        "#;

  let parsed =
    FormalityConfig::parse_str(override_toml, Path::new("test.toml")).unwrap();
  base.merge(parsed);

  let global = base.resolve_global();
  assert_eq!(global.indent_size, 4);
  assert_eq!(global.line_length, 100);

  let rust = base.resolve_for_lang("rust");
  assert_eq!(rust.indent_size, 4);
  assert_eq!(rust.line_length, 100);

  let md = base.resolve_for_lang("markdown");
  assert_eq!(md.indent_size, 2);
  assert_eq!(md.line_length, 100);
  assert_eq!(md.prose_wrap.as_deref(), Some("always"));
}

#[test]
fn test_lang_config_extra_args_files_and_exclude() {
  let toml = r#"
      [global]
      indent_size = 2

      [lang.rust]
      extra_args = ["--verbose", "--", "-D", "clippy::all"]
      files = ["src/lib.rs", "src/main.rs"]
      exclude = ["tests/fixtures", "src/generated/**"]
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
  let rust = parsed.resolve_for_lang("rust");
  assert_eq!(
    rust.extra_args,
    vec!["--verbose", "--", "-D", "clippy::all"]
  );
  assert_eq!(
    rust.files,
    vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")]
  );
  assert_eq!(
    rust.exclude,
    vec![
      PathBuf::from("tests/fixtures"),
      PathBuf::from("src/generated/**")
    ]
  );
}

#[test]
fn test_layout_facet_direct_and_inheritance() {
  let toml = r#"
      [global]
      indent_size = 2
      line_length = 80

      [global.layout]
      use_tabs = true
      prose_wrap = "preserve"

      [lang.rust.layout]
      indent_size = 4
      line_length = 100

      [lang.markdown]
      prose_wrap = "always"
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
  let global = parsed.resolve_global();
  assert_eq!(global.indent_size, 2);
  assert_eq!(global.line_length, 80);
  assert!(global.use_tabs);
  assert_eq!(global.layout.prose_wrap.as_deref(), Some("preserve"));

  let rust = parsed.resolve_for_lang("rust");
  assert_eq!(rust.indent_size, 4);
  assert_eq!(rust.line_length, 100);
  assert!(rust.use_tabs);
  assert_eq!(rust.prose_wrap.as_deref(), Some("preserve"));
  assert_eq!(rust.layout.indent_size, Some(4));
  assert_eq!(rust.layout.line_length, Some(100));

  let md = parsed.resolve_for_lang("markdown");
  assert_eq!(md.indent_size, 2);
  assert_eq!(md.line_length, 80);
  assert!(md.use_tabs);
  assert_eq!(md.prose_wrap.as_deref(), Some("always"));
  assert_eq!(
    md.markdown,
    Some(MarkdownOptions {
      prose_wrap: Some("always".to_string())
    })
  );
}

#[test]
fn test_typed_options_deserialization_from_toml() {
  let toml = r#"
      [lang.rust]
      edition = "2021"
      version = "1.75"

      [lang.python]
      quote_style = "single"
      target_version = "py311"

      [lang.cpp]
      standard = "c++20"
      column_limit = 100
      based_on_style = "Google"
      pointer_alignment = "Left"
      break_before_braces = "Attach"
      sort_includes = true

      [lang.markdown]
      prose_wrap = "never"

      [lang.yaml]
      indent_sequence = true

      [lang.json]

      [lang.toml]

      [lang.typst]
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

  let rust = parsed.resolve_for_lang("rust");
  assert_eq!(
    rust.rust,
    Some(RustOptions {
      edition: Some("2021".to_string()),
      version: Some("1.75".to_string()),
    })
  );

  let python = parsed.resolve_for_lang("python");
  assert_eq!(
    python.python,
    Some(PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py311".to_string()),
    })
  );

  let cpp = parsed.resolve_for_lang("cpp");
  assert_eq!(
    cpp.cpp,
    Some(CppOptions {
      standard: Some("c++20".to_string()),
      column_limit: Some(100),
      based_on_style: Some("Google".to_string()),
      pointer_alignment: Some("Left".to_string()),
      break_before_braces: Some("Attach".to_string()),
      sort_includes: Some(true),
    })
  );

  let md = parsed.resolve_for_lang("markdown");
  assert_eq!(
    md.markdown,
    Some(MarkdownOptions {
      prose_wrap: Some("never".to_string()),
    })
  );

  let yaml = parsed.resolve_for_lang("yaml");
  assert_eq!(
    yaml.yaml,
    Some(YamlOptions {
      indent_sequence: Some(true),
      document_start: None,
      truthy: None,
    })
  );

  let json = parsed.resolve_for_lang("json");
  assert_eq!(json.json, Some(JsonOptions {}));

  let toml_lang = parsed.resolve_for_lang("toml");
  assert_eq!(toml_lang.toml, Some(TomlOptions {}));

  let typst = parsed.resolve_for_lang("typst");
  assert_eq!(typst.typst, Some(TypstOptions {}));
}

#[test]
fn test_typed_options_subtable_deserialization() {
  let toml = r#"
      [lang.rust.rust]
      edition = "2024"
      version = "1.85"

      [lang.python.python]
      quote_style = "double"
      target_version = "py312"

      [lang.cpp.cpp]
      standard = "c++23"
      column_limit = 120
      based_on_style = "Chromium"
      pointer_alignment = "Right"
      break_before_braces = "Allman"
      sort_includes = false

      [lang.yaml.yaml]
      indent_sequence = false
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();

  let rust = parsed.resolve_for_lang("rust");
  assert_eq!(
    rust.rust,
    Some(RustOptions {
      edition: Some("2024".to_string()),
      version: Some("1.85".to_string()),
    })
  );

  let python = parsed.resolve_for_lang("python");
  assert_eq!(
    python.python,
    Some(PythonOptions {
      quote_style: Some("double".to_string()),
      target_version: Some("py312".to_string()),
    })
  );

  let cpp = parsed.resolve_for_lang("cpp");
  assert_eq!(
    cpp.cpp,
    Some(CppOptions {
      standard: Some("c++23".to_string()),
      column_limit: Some(120),
      based_on_style: Some("Chromium".to_string()),
      pointer_alignment: Some("Right".to_string()),
      break_before_braces: Some("Allman".to_string()),
      sort_includes: Some(false),
    })
  );

  let yaml = parsed.resolve_for_lang("yaml");
  assert_eq!(
    yaml.yaml,
    Some(YamlOptions {
      indent_sequence: Some(false),
      document_start: None,
      truthy: None,
    })
  );
}

#[test]
fn test_typed_options_merging_semantics() {
  let mut base = FormalityConfig::empty();
  let base_toml = r#"
      [global]
      indent_size = 2
      line_length = 80

      [lang.rust]
      edition = "2021"
      indent_size = 4

      [lang.python]
      quote_style = "single"
    "#;
  base.merge(
    FormalityConfig::parse_str(base_toml, Path::new("base.toml")).unwrap(),
  );

  let override_toml = r#"
      [lang.rust]
      version = "1.78"
      line_length = 100

      [lang.python]
      target_version = "py312"
    "#;
  base.merge(
    FormalityConfig::parse_str(override_toml, Path::new("override.toml"))
      .unwrap(),
  );

  let rust = base.resolve_for_lang("rust");
  assert_eq!(rust.indent_size, 4);
  assert_eq!(rust.line_length, 100);
  assert_eq!(
    rust.rust,
    Some(RustOptions {
      edition: Some("2021".to_string()),
      version: Some("1.78".to_string()),
    })
  );

  let python = base.resolve_for_lang("python");
  assert_eq!(
    python.python,
    Some(PythonOptions {
      quote_style: Some("single".to_string()),
      target_version: Some("py312".to_string()),
    })
  );
}

#[test]
fn test_serialization_deserialization_roundtrip() {
  let mut config = FormalityConfig::empty();
  config.global = Some(GlobalConfig {
    languages: Some(vec!["rust".to_string(), "python".to_string()]),
    ignore_languages: None,
    indent_size: Some(2),
    line_length: Some(100),
    end_of_line: Some("lf".to_string()),
    charset: Some("utf-8".to_string()),
    insert_final_newline: Some(true),
    trim_trailing_whitespace: Some(true),
    use_tabs: Some(false),
    layout: Some(LayoutFacet {
      indent_size: Some(2),
      line_length: Some(100),
      use_tabs: Some(false),
      prose_wrap: Some("always".to_string()),
    }),
    exclude: Vec::new(),
  });

  let rust_cfg = LangConfig {
    indent_size: Some(4),
    rust: Some(RustOptions {
      edition: Some("2024".to_string()),
      version: Some("1.85".to_string()),
    }),
    ..Default::default()
  };
  config.lang.insert("rust".to_string(), rust_cfg);

  let py_cfg = LangConfig {
    python: Some(PythonOptions {
      quote_style: Some("double".to_string()),
      target_version: Some("py311".to_string()),
    }),
    ..Default::default()
  };
  config.lang.insert("python".to_string(), py_cfg);

  let serialized = toml::to_string(&config).unwrap();
  let deserialized: FormalityConfig =
    FormalityConfig::parse_str(&serialized, Path::new("test.toml")).unwrap();

  assert_eq!(config, deserialized);
}

#[test]
fn test_language_options_merge_units() {
  let mut rust1 = RustOptions {
    edition: Some("2021".to_string()),
    version: None,
  };
  let rust2 = RustOptions {
    edition: None,
    version: Some("1.75".to_string()),
  };
  rust1.merge(rust2);
  assert_eq!(rust1.edition.as_deref(), Some("2021"));
  assert_eq!(rust1.version.as_deref(), Some("1.75"));

  let mut py1 = PythonOptions {
    quote_style: Some("single".to_string()),
    target_version: None,
  };
  let py2 = PythonOptions {
    quote_style: None,
    target_version: Some("py312".to_string()),
  };
  py1.merge(py2);
  assert_eq!(py1.quote_style.as_deref(), Some("single"));
  assert_eq!(py1.target_version.as_deref(), Some("py312"));

  let mut cpp1 = CppOptions {
    standard: Some("c++17".to_string()),
    column_limit: None,
    based_on_style: Some("LLVM".to_string()),
    pointer_alignment: None,
    break_before_braces: None,
    sort_includes: Some(true),
  };
  let cpp2 = CppOptions {
    standard: None,
    column_limit: Some(100),
    based_on_style: None,
    pointer_alignment: Some("Right".to_string()),
    break_before_braces: Some("Allman".to_string()),
    sort_includes: Some(false),
  };
  cpp1.merge(cpp2);
  assert_eq!(cpp1.standard.as_deref(), Some("c++17"));
  assert_eq!(cpp1.column_limit, Some(100));
  assert_eq!(cpp1.based_on_style.as_deref(), Some("LLVM"));
  assert_eq!(cpp1.pointer_alignment.as_deref(), Some("Right"));
  assert_eq!(cpp1.break_before_braces.as_deref(), Some("Allman"));
  assert_eq!(cpp1.sort_includes, Some(false));

  let mut yaml1 = YamlOptions {
    indent_sequence: Some(true),
    document_start: Some(true),
    truthy: None,
  };
  let yaml2 = YamlOptions {
    indent_sequence: Some(false),
    document_start: None,
    truthy: Some(false),
  };
  yaml1.merge(yaml2);
  assert_eq!(yaml1.indent_sequence, Some(false));
  assert_eq!(yaml1.document_start, Some(true));
  assert_eq!(yaml1.truthy, Some(false));

  let mut layout1 = LayoutFacet {
    indent_size: Some(2),
    line_length: None,
    use_tabs: None,
    prose_wrap: None,
  };
  let layout2 = LayoutFacet {
    indent_size: None,
    line_length: Some(100),
    use_tabs: Some(true),
    prose_wrap: Some("preserve".to_string()),
  };
  layout1.merge(layout2);
  assert_eq!(layout1.indent_size, Some(2));
  assert_eq!(layout1.line_length, Some(100));
  assert_eq!(layout1.use_tabs, Some(true));
  assert_eq!(layout1.prose_wrap.as_deref(), Some("preserve"));
}

#[test]
fn test_yaml_options_document_start_and_truthy_rules() {
  let toml = r"
      [lang.yaml]
      indent_sequence = true
      document_start = false
      truthy = true
    ";
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
  let yaml = parsed.resolve_for_lang("yaml");
  assert_eq!(
    yaml.yaml,
    Some(YamlOptions {
      indent_sequence: Some(true),
      document_start: Some(false),
      truthy: Some(true),
    })
  );
}

#[test]
fn test_generate_sample_omits_languages() {
  let sample = FormalityConfig::generate_sample();
  assert!(sample.contains("# formality configuration file"));
  assert!(sample.contains(
    "#:schema https://github.com/arvinduh/formality/releases/download/s1/formality.schema.json"
  ));
  assert!(sample.contains("[global]"));
  assert!(!sample.contains("languages ="));
  assert!(sample.contains("indent_size = 2"));
  assert!(sample.contains("line_length = 80"));
  assert!(sample.contains("end_of_line = \"lf\""));
  assert!(sample.contains("charset = \"utf-8\""));
  assert!(sample.contains("insert_final_newline = true"));
  assert!(sample.contains("trim_trailing_whitespace = true"));

  let parsed =
    FormalityConfig::parse_str(&sample, Path::new("formality.toml")).unwrap();
  let global = parsed.resolve_global();
  assert_eq!(global.languages, None);
  assert_eq!(global.indent_size, 2);
  assert_eq!(global.line_length, 80);
  assert_eq!(global.end_of_line, "lf");
  assert_eq!(global.charset, "utf-8");
  assert!(global.insert_final_newline);
  assert!(global.trim_trailing_whitespace);
}

#[test]
fn test_generate_init_template_omits_languages() {
  let template =
    FormalityConfig::generate_init_template(&["rust", "python", "toml"]);
  assert!(!template.contains("languages ="));
  assert!(template.contains("[global]"));

  let parsed =
    FormalityConfig::parse_str(&template, Path::new("formality.toml")).unwrap();
  let global = parsed.resolve_global();
  assert_eq!(global.languages, None);
}

#[test]
fn test_generate_init_template_emits_commented_lang_stubs_for_detected() {
  let template =
    FormalityConfig::generate_init_template(&["rust", "python", "toml"]);

  // Detected languages get a commented-out, ready-to-uncomment stub section
  // each, in deterministic sorted order.
  assert!(template.contains("# [lang.python]"));
  assert!(template.contains("# [lang.rust]"));
  assert!(template.contains("# [lang.toml]"));
  let python_pos = template.find("# [lang.python]").unwrap();
  let rust_pos = template.find("# [lang.rust]").unwrap();
  let toml_pos = template.find("# [lang.toml]").unwrap();
  assert!(python_pos < rust_pos);
  assert!(rust_pos < toml_pos);

  // A language that wasn't detected gets no stub.
  assert!(!template.contains("[lang.go]"));

  // Stubs are commented out, so the template still parses to an empty `lang`
  // map — they must not silently activate any override.
  let parsed =
    FormalityConfig::parse_str(&template, Path::new("formality.toml")).unwrap();
  assert!(parsed.lang.is_empty());
}

#[test]
fn test_generate_init_template_dedupes_langs() {
  let template = FormalityConfig::generate_init_template(&["rust", "rust"]);
  assert_eq!(template.matches("[lang.rust]").count(), 1);
}

#[test]
fn test_generate_init_template_no_detected_langs_matches_sample() {
  let template = FormalityConfig::generate_init_template(&[]);
  assert_eq!(template, FormalityConfig::generate_sample());
}

#[test]
fn test_unrecognized_lang_sections_flags_typo_but_not_valid_undetected() {
  let registry = crate::surfaces::SurfaceRegistry::default();

  // A genuine typo: "pythonn" is not a known surface name or alias.
  let toml = r"
    [lang.pythonn]
    indent_size = 4
  ";
  let cfg =
    FormalityConfig::parse_str(toml, Path::new("formality.toml")).unwrap();
  assert_eq!(
    cfg.unrecognized_lang_sections(&registry),
    vec!["pythonn"],
    "a typo'd section name should be flagged"
  );

  // A valid, recognized surface name that simply isn't active/detected in
  // the current workspace (pre-configuring for a language not yet in use)
  // must NOT be flagged — this is a legitimate, intentional override.
  let toml = r"
    [lang.rust]
    indent_size = 4
  ";
  let cfg =
    FormalityConfig::parse_str(toml, Path::new("formality.toml")).unwrap();
  assert!(
    cfg.unrecognized_lang_sections(&registry).is_empty(),
    "a valid but undetected surface name should not be flagged"
  );
}

#[test]
fn test_load_file_missing_path_yields_io_error() {
  let missing = Path::new("this/path/definitely/does/not/exist.toml");
  let err = FormalityConfig::load_file(missing).unwrap_err();
  assert!(matches!(err, ConfigError::Io { .. }));
  let msg = err.to_string();
  assert!(msg.contains("Failed to read config file at"));
  assert!(msg.contains("exist.toml"));
}

#[test]
fn test_parse_str_malformed_toml_yields_parse_error() {
  // Missing closing bracket / invalid TOML syntax.
  let bad_toml = "[global\nindent_size = 2";
  let err = FormalityConfig::parse_str(bad_toml, Path::new("formality.toml"))
    .unwrap_err();
  assert!(matches!(err, ConfigError::Parse { .. }));
  let msg = err.to_string();
  assert!(msg.contains("Failed to parse config file at"));
  assert!(msg.contains("formality.toml"));
}

#[test]
fn test_config_error_invalid_display() {
  let err = ConfigError::Invalid("something is wrong".to_string());
  assert_eq!(err.to_string(), "Invalid config: something is wrong");
}

#[test]
fn test_java_aosp_style_defaults_indent_width_to_four() {
  // Java's indent_width is conditionally Fixed: google-java-format's
  // --aosp flag pins it to 4 spaces (vs. 2 for the default Google style),
  // resolved via `[lang.java] style` rather than a plain constant (see
  // docs/facet-rosetta.md, JavaSurface::facet_support). Neither the AOSP
  // branch nor its interaction with an explicit indent_size override had
  // any test coverage.
  let toml = r#"
      [lang.java]
      style = "aosp"
    "#;
  let parsed =
    FormalityConfig::parse_str(toml, Path::new("test.toml")).unwrap();
  let java = parsed.resolve_for_lang("java");
  assert_eq!(java.indent_size, 4);

  // Default (Google) style keeps the ordinary global-inherited indent_size.
  let toml_google = r#"
      [lang.java]
      style = "google"
    "#;
  let parsed_google =
    FormalityConfig::parse_str(toml_google, Path::new("test.toml")).unwrap();
  let java_google = parsed_google.resolve_for_lang("java");
  assert_eq!(java_google.indent_size, 2);

  // No [lang.java] section at all: not AOSP, so global default applies.
  let default_java = FormalityConfig::with_defaults().resolve_for_lang("java");
  assert_eq!(default_java.indent_size, 2);

  // An explicit indent_size override always wins over the AOSP inference.
  let toml_explicit = r#"
      [lang.java]
      style = "aosp"
      indent_size = 8
    "#;
  let parsed_explicit =
    FormalityConfig::parse_str(toml_explicit, Path::new("test.toml")).unwrap();
  let java_explicit = parsed_explicit.resolve_for_lang("java");
  assert_eq!(java_explicit.indent_size, 8);
}

#[test]
fn test_unrecognized_lang_sections_handles_case_and_aliases() {
  let registry = crate::surfaces::SurfaceRegistry::default();

  // Canonical names are matched case-insensitively.
  let toml = r"
    [lang.RUST]
    indent_size = 4
  ";
  let cfg =
    FormalityConfig::parse_str(toml, Path::new("formality.toml")).unwrap();
  assert!(
    cfg.unrecognized_lang_sections(&registry).is_empty(),
    "canonical names should resolve case-insensitively"
  );

  // Aliases (e.g. "py" for "python", "js" for "javascript") also resolve
  // and must not be flagged.
  let toml = r"
    [lang.py]
    indent_size = 4
  ";
  let cfg =
    FormalityConfig::parse_str(toml, Path::new("formality.toml")).unwrap();
  assert!(
    cfg.unrecognized_lang_sections(&registry).is_empty(),
    "known aliases should resolve to their canonical surface"
  );

  // Multiple unrecognized sections are all reported.
  let toml = r"
    [lang.pythonn]
    indent_size = 4

    [lang.jaav]
    indent_size = 4
  ";
  let cfg =
    FormalityConfig::parse_str(toml, Path::new("formality.toml")).unwrap();
  let mut unrecognized = cfg.unrecognized_lang_sections(&registry);
  unrecognized.sort_unstable();
  assert_eq!(unrecognized, vec!["jaav", "pythonn"]);
}
