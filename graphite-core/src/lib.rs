use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod anchor_scanner;
pub mod config;
pub mod node_parser;
pub mod sidecar;
pub mod validation;

// @graphite:evidence spec-edge
/// A string newtype representing the kind of an edge.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeKind(pub String);

/// Severity level for diagnostics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Severity {

    Error,
    Warning,
}

/// A diagnostic message produced during graph validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub detail: String,
    pub fix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    pub hint: String,
}

// @graphite:evidence spec-schema
// @graphite:evidence spec-node-category
// @graphite:evidence spec-document-format
/// The schema definition for a graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub categories: HashMap<String, CategoryDef>,
    pub edges: Vec<EdgeDef>,
}

// @graphite:evidence spec-node-category
/// The definition of a single category in a schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CategoryDef {
    pub key: String,
}

// @graphite:evidence spec-edge
/// The definition of a directed edge between two kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeDef {
    pub name: String,
    #[serde(rename = "from")]
    pub from: String,
    #[serde(rename = "to")]
    pub to: String,
}

// @graphite:evidence spec-index-node
/// An index indicating a node belongs to a specific category.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Index {
    #[serde(alias = "of_kind")]
    pub of_category: String,
}

// @graphite:evidence spec-node
// @graphite:evidence spec-node-id
// @graphite:evidence spec-node-category
// @graphite:evidence spec-header
// @graphite:evidence spec-body
// @graphite:evidence spec-markdown-extension
/// A node in the knowledge graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(alias = "kind")]
    pub category: String,
    pub body: String,
    pub edges: HashMap<String, Vec<String>>,
    pub metadata: HashMap<String, String>,
    pub index: Option<Index>,
    #[serde(default)]
    pub content_len: usize,
}

// @graphite:evidence spec-graph
/// A typed directed knowledge graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub schema: Schema,
}

impl Graph {
    // @graphite:evidence spec-graph
    pub fn new(schema: Schema) -> Self {
        Graph {
            nodes: HashMap::new(),
            schema,
        }
    }

    // @graphite:evidence spec-graph
    /// Add a node to the graph. Returns the previous node if one with the same
    /// ID already existed (i.e. a duplicate).
    pub fn add_node(&mut self, node: Node) -> Option<Node> {
        self.nodes.insert(node.id.clone(), node)
    }
}

pub mod schema;
