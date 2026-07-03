use graphite_core::schema::{DEFAULT_SCHEMA_YAML, SchemaParser};

#[test]
fn test_default_schema_parses() {
    let schema = SchemaParser::parse(DEFAULT_SCHEMA_YAML).expect("default schema should parse");
    assert_eq!(schema.kinds.len(), 4, "default schema should have 4 kinds");
    assert_eq!(schema.edges.len(), 5, "default schema should have 5 edges");
}

#[test]
fn test_invalid_schema_undefined_kind() {
    let yaml = "\
kinds:
  requirement: { key: REQ }
edges:
  bad_edge: { from: requirement, to: nonexistent }
";
    let err = SchemaParser::parse(yaml).expect_err("undefined kind should produce error");
    assert_eq!(err.rule, "schema-parse-error");
    assert!(
        err.detail.contains("nonexistent"),
        "detail should mention the missing kind: {}",
        err.detail
    );
    assert!(
        err.detail.contains("bad_edge"),
        "detail should mention the edge name: {}",
        err.detail
    );
}

#[test]
fn test_duplicate_kind_rejected() {
    let yaml = "\
kinds:
  requirement: { key: REQ }
  requirement: { key: REQ2 }
edges:
  test_edge: { from: requirement, to: requirement }
";
    let err = SchemaParser::parse(yaml).expect_err("duplicate kind should produce error");
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
kinds:
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
fn test_builtin_kinds_allowed() {
    // "evidence", "index", and "any" are always valid even without declaration
    let yaml = "\
kinds:
  requirement: { key: REQ }
edges:
  to_evidence: { from: requirement, to: evidence }
  from_index: { from: index, to: requirement }
  wildcard: { from: any, to: any }
";
    let schema = SchemaParser::parse(yaml).expect("built-ins evidence, index, any should be valid");
    assert_eq!(schema.kinds.len(), 1, "only requirement should be in kinds");
    assert_eq!(schema.edges.len(), 3, "all 3 edges should parse");
}

#[test]
fn test_contains_edge_kind_handled() {
    // "contains" is a valid edge kind name even if not declared in schema YAML
    let yaml = "\
kinds:
  index: { key: IDX }
  service: { key: SVC }
edges:
  contains: { from: index, to: service }
";
    let schema =
        SchemaParser::parse(yaml).expect("edge named 'contains' with valid kinds should parse");
    assert!(schema.edges.iter().any(|e| e.name == "contains"));
}
