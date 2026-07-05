use graphite_core::schema::{DEFAULT_SCHEMA_YAML, SchemaParser};

#[test]
fn test_default_schema_parses() {
    let schema = SchemaParser::parse(DEFAULT_SCHEMA_YAML).expect("default schema should parse");
    assert_eq!(schema.categories.len(), 7, "default schema should have 7 categories");
    assert_eq!(schema.edges.len(), 5, "default schema should have 5 edges");
}

#[test]
fn test_invalid_schema_undefined_category() {
    let yaml = "\
categories:
  requirement: { key: REQ }
edges:
  bad_edge: { from: requirement, to: nonexistent }
";
    let err = SchemaParser::parse(yaml).expect_err("undefined category should produce error");
    assert_eq!(err.rule, "schema-parse-error");
    assert!(
        err.detail.contains("nonexistent"),
        "detail should mention the missing category: {}",
        err.detail
    );
    assert!(
        err.detail.contains("bad_edge"),
        "detail should mention the edge name: {}",
        err.detail
    );
}

#[test]
fn test_duplicate_category_rejected() {
    let yaml = "\
categories:
  requirement: { key: REQ }
  requirement: { key: REQ2 }
edges:
  test_edge: { from: requirement, to: requirement }
";
    let err = SchemaParser::parse(yaml).expect_err("duplicate category should produce error");
    assert_eq!(err.rule, "schema-parse-error");
    assert!(
        err.detail.contains("Duplicate"),
        "detail should mention duplicate"
    );
    assert!(
        err.detail.contains("requirement"),
        "detail should name the duplicate key"
    );
}

#[test]
fn test_duplicate_edge_rejected() {
    let yaml = "\
categories:
  a: { key: A }
  b: { key: B }
edges:
  same_edge: { from: a, to: b }
  same_edge: { from: b, to: a }
";
    let err = SchemaParser::parse(yaml).expect_err("duplicate edge should produce error");
    assert_eq!(err.rule, "schema-parse-error");
    assert!(
        err.detail.contains("Duplicate"),
        "detail should mention duplicate"
    );
}

#[test]
fn test_default_schema_contains_expected_edges() {
    let schema = SchemaParser::default_schema();
    let names: Vec<&str> = schema.edges.iter().map(|e| e.name.as_str()).collect();

    assert!(
        names.contains(&"implemented_by"),
        "default schema should contain implemented_by edge"
    );
    assert!(
        names.contains(&"verified_by"),
        "default schema should contain verified_by edge"
    );
    assert!(
        names.contains(&"describes"),
        "default schema should contain describes edge"
    );
    assert!(
        names.contains(&"references"),
        "default schema should contain references edge"
    );
}

#[test]
fn test_builtin_categories_allowed() {
    let yaml = "\
categories:
  requirement: { key: REQ }
edges:
  wildcard: { from: any, to: any }
";
    let schema = SchemaParser::parse(yaml).expect("built-in 'any' should be valid");
    assert_eq!(schema.categories.len(), 1, "only requirement should be in categories");
    assert_eq!(schema.edges.len(), 1, "wildcard edge should parse");
}

#[test]
fn test_contains_edge_kind_handled() {
    // "contains" is a valid edge kind name even if not declared in schema YAML
    let yaml = "\
categories:
  index: { key: IDX }
  service: { key: SVC }
edges:
  contains: { from: index, to: service }
";
    let schema =
        SchemaParser::parse(yaml).expect("edge named 'contains' with valid categories should parse");
    assert!(schema.edges.iter().any(|e| e.name == "contains"));
}
