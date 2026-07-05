use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use graphite_core::anchor_scanner::AnchorScanner;
use graphite_core::config::Config;
use graphite_core::node_parser::NodeParser;
use graphite_core::schema::{DEFAULT_SCHEMA_YAML, SchemaParser};
use graphite_core::sidecar::SidecarResolver;
use graphite_core::validation::ValidationEngine;
use graphite_core::{Diagnostic, Graph, Severity};
use graphite_render::style;

#[derive(Parser)]
#[command(
    name = "graphite",
    version,
    about = "A compiled knowledge graph for software engineering"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Graphite project scaffold
    Init {
        path: String,
        #[arg(long)]
        force: bool,
    },
    /// Validate a graph directory
    Validate {
        path: String,
        #[arg(long)]
        focus: Option<String>,
        #[arg(long)]
        first: bool,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        compat: Option<String>,
    },
    /// Show context for a node
    Context {
        id: String,
        #[arg(default_value = "graph")]
        graph_dir: String,
    },
    /// Compute a work order for implementing or modifying a node
    Plan {
        id: String,
        #[arg(default_value = "graph")]
        graph_dir: String,
    },
    /// Show knowledge-level changes since a git ref
    Diff {
        #[arg(long, default_value = "HEAD")]
        from: String,
        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(default_value = "graph")]
        graph_dir: String,
        #[arg(long)]
        json: bool,
    },
    /// Render a graph directory to static HTML documentation
    Render {
        #[arg(default_value = "graph")]
        graph_dir: String,
        #[arg(long, short, default_value = "docs")]
        output: String,
        #[arg(long, default_value = "sci-fi")]
        style: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let config = Config::load_or_default(Path::new(".")).unwrap_or_default();

    match cli.command {
        Commands::Init { path, force } => cmd_init(&path, force),
        Commands::Validate {
            path,
            focus,
            first,
            strict,
            json,
            compat,
        } => {
            if !cmd_validate(
                &path,
                focus.as_deref(),
                first,
                strict,
                json,
                compat.as_deref(),
                &config,
            ) {
                std::process::exit(1);
            }
        }
        Commands::Context { id, graph_dir } => {
            cmd_context(&id, &graph_dir, &config.scan);
        }
        Commands::Plan { id, graph_dir } => cmd_plan(&id, &graph_dir),
        Commands::Diff { from, json } => cmd_diff(&from, json),
        Commands::Stats { graph_dir, json } => cmd_stats(&graph_dir, json, &config),
        Commands::Render {
            graph_dir,
            output,
            style,
        } => cmd_render(&graph_dir, &output, &style, &config),
    }
}

// ---------------------------------------------------------------------------
// Shared: load .node files from a directory into a Graph
// ---------------------------------------------------------------------------

fn load_graph(graph_dir: &str) -> Result<Graph, Vec<Diagnostic>> {
    let schema = match load_schema_for_graph_dir(graph_dir) {
        Ok(schema) => schema,
        Err(diag) => return Err(vec![diag]),
    };
    let mut graph = Graph::new(schema);
    let mut errors = Vec::new();

    let dir = Path::new(graph_dir);
    if !dir.is_dir() {
        errors.push(Diagnostic {
            rule: "graph-not-found".into(),
            severity: Severity::Error,
            node_id: None,
            file: Some(graph_dir.to_string()),
            detail: format!("Graph directory '{graph_dir}' not found"),
            fix: "Run `graphite init` or point to a valid graph directory.".into(),
            example: None,
            hint: "The graph directory should contain .node files.".into(),
        });
        return Err(errors);
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(Diagnostic {
                rule: "read-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(graph_dir.to_string()),
                detail: format!("Cannot read directory '{graph_dir}': {e}"),
                fix: "Check directory permissions.".into(),
                example: None,
                hint: "Graph directory must be readable.".into(),
            });
            return Err(errors);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        collect_node_files(&path, &mut graph, &mut errors);
    }

    if errors.is_empty() {
        Ok(graph)
    } else {
        Err(errors)
    }
}

#[allow(clippy::result_large_err)]
fn load_schema_for_graph_dir(graph_dir: &str) -> Result<graphite_core::Schema, Diagnostic> {
    let graph_path = Path::new(graph_dir);
    let project_root = graph_path.parent().unwrap_or_else(|| Path::new("."));
    let schema_path = project_root.join("graph.schema");

    if !schema_path.exists() {
        return Ok(SchemaParser::default_schema());
    }

    let schema_yaml = fs::read_to_string(&schema_path).map_err(|e| Diagnostic {
        rule: "schema-read-error".into(),
        severity: Severity::Error,
        node_id: None,
        file: Some(schema_path.to_string_lossy().into()),
        detail: format!("Cannot read schema file '{}': {e}", schema_path.display()),
        fix: "Ensure graph.schema exists and is readable UTF-8.".into(),
        example: None,
        hint: "Place graph.schema at the repository root next to graphite.yaml.".into(),
    })?;

    SchemaParser::parse(&schema_yaml).map_err(|mut d| {
        d.file = Some(schema_path.to_string_lossy().into());
        d
    })
}

fn collect_node_files(path: &Path, graph: &mut Graph, errors: &mut Vec<Diagnostic>) {
    // Track file paths of loaded nodes (id → file path) for duplicate detection
    let mut node_files: HashMap<String, String> = HashMap::new();
    collect_node_files_inner(path, graph, errors, &mut node_files);
}

fn collect_node_files_inner(
    path: &Path,
    graph: &mut Graph,
    errors: &mut Vec<Diagnostic>,
    node_files: &mut HashMap<String, String>,
) {
    if path.is_dir() {
        #[allow(clippy::collapsible_if)]
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                return;
            }
        }
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                collect_node_files_inner(&entry.path(), graph, errors, node_files);
            }
        }
        return;
    }

    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("node") {
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            errors.push(Diagnostic {
                rule: "read-error".into(),
                severity: Severity::Error,
                node_id: None,
                file: Some(path.to_string_lossy().into()),
                detail: format!("Cannot read '{}': {e}", path.display()),
                fix: "Ensure the file is readable UTF-8.".into(),
                example: None,
                hint: "Graph files must be valid UTF-8.".into(),
            });
            return;
        }
    };

    match NodeParser::parse(&content) {
        Ok(node) => {
            let file_path = path.to_string_lossy().to_string();
            // Check for duplicate ID
            if let Some(prev_file) = node_files.get(&node.id) {
                errors.push(Diagnostic {
                    rule: "duplicate-node-id".into(),
                    severity: Severity::Error,
                    node_id: Some(node.id.clone()),
                    file: Some(file_path),
                    detail: format!(
                        "Duplicate node ID '{}' — first defined in '{}'",
                        node.id, prev_file
                    ),
                    fix: "Rename one of the nodes to have a unique ID.".into(),
                    example: None,
                    hint: "Every node ID must be globally unique across the graph.".into(),
                });
            } else {
                node_files.insert(node.id.clone(), file_path);
            }
            graph.add_node(node);
        }
        Err(diag) => {
            let mut with_file = diag;
            with_file.file = Some(path.to_string_lossy().into());
            errors.push(with_file);
        }
    }
}

// ---------------------------------------------------------------------------
// Evidence resolution
// ---------------------------------------------------------------------------

fn resolve_evidence(
    scan_paths: &[String],
    base_dir: &Path,
) -> HashMap<String, Vec<(PathBuf, usize)>> {
    let mut evidence = HashMap::new();

    for scan_path in scan_paths {
        let full_path = {
            let p = Path::new(scan_path);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                base_dir.join(p)
            }
        };

        if let Ok(scanned) = AnchorScanner::scan(&full_path) {
            for (id, locs) in scanned {
                evidence.entry(id).or_insert_with(Vec::new).extend(locs);
            }
        }
        if let Ok(sidecars) = SidecarResolver::resolve(&full_path) {
            for (id, locs) in sidecars {
                evidence.entry(id).or_insert_with(Vec::new).extend(locs);
            }
        }
    }

    // Deduplicate locations within each evidence ID (overlapping scan roots
    // can produce duplicate (path, line) pairs).
    for locations in evidence.values_mut() {
        locations.sort();
        locations.dedup();
    }

    evidence
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

// @graphite:evidence spec-validate
fn cmd_validate(
    graph_dir: &str,
    focus: Option<&str>,
    first: bool,
    strict: bool,
    json: bool,
    compat: Option<&str>,
    config: &Config,
) -> bool {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            output_diagnostics(&errors, focus, first, json);
            return false;
        }
    };

    let engine = ValidationEngine;
    let mut diagnostics = engine.validate(&graph);

    let base_dir = Path::new(graph_dir)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let evidence = resolve_evidence(&config.scan, base_dir);
    diagnostics.extend(engine.check_evidence_anchors(&graph, &evidence));
    diagnostics.extend(engine.check_unused_anchors(&graph, &evidence));
    diagnostics.extend(engine.check_node_file_size(&graph, config.node_max_chars));
    diagnostics.extend(check_compatibility(&graph, compat));

    if strict {
        for d in &mut diagnostics {
            if matches!(d.severity, Severity::Warning) {
                d.severity = Severity::Error;
            }
        }
    }

    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));

    if diagnostics.is_empty() {
        println!("✓ graph is valid");
        return true;
    }

    if !has_errors {
        output_diagnostics(&diagnostics, focus, first, json);
        if !json {
            println!("✓ graph is valid with warnings");
        }
        return true;
    }

    output_diagnostics(&diagnostics, focus, first, json);
    false
}

fn check_compatibility(_graph: &Graph, _compat: Option<&str>) -> Vec<Diagnostic> {
    Vec::new()
}

fn output_diagnostics(diags: &[Diagnostic], focus: Option<&str>, first: bool, json: bool) {
    let mut filtered: Vec<&Diagnostic> = diags.iter().collect();

    if let Some(focus_id) = focus {
        filtered.retain(|d| d.node_id.as_deref() == Some(focus_id));
    }

    if first {
        filtered.truncate(1);
    }

    if json {
        let json_str = serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| "[]".into());
        println!("{json_str}");
        return;
    }

    for d in &filtered {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        println!("[{sev}] {}: {}", d.rule, d.detail);
        println!("  fix: {}", d.fix);
        if let Some(example) = &d.example {
            println!("  example:\n{}", example);
        }
        println!("  hint: {}", d.hint);
        if first {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

// @graphite:evidence spec-init
fn cmd_init(path: &str, force: bool) {
    let root = Path::new(path);
    let graph_dir = root.join("graph");

    if graph_dir.exists() && !force {
        eprintln!(
            "error: '{}/graph' already exists (use --force to overwrite)",
            root.display()
        );
        std::process::exit(1);
    }

    let dirs = [
        "graph/spec",
        "graph/requirement",
        "graph/adr",
        "graph/service",
        "graph/test",
        "graph/compliance",
        "graph/runbook",
        "graph/infra",
    ];
    for d in &dirs {
        let _ = fs::create_dir_all(root.join(d));
    }

    let schema_path = root.join("graph.schema");
    let _ = fs::write(&schema_path, DEFAULT_SCHEMA_YAML);

    write_node(
        &graph_dir.join("spec/root.node"),
        "root",
        "spec",
        "# Graphite\n\nA compiled knowledge graph for software engineering.\n\n[edge:compiler-requirement] [edge:compiler]",
    );

    write_node(
        &graph_dir.join("requirement/compiler-requirement.node"),
        "compiler-requirement",
        "requirement",
        "# Compiler Requirement\n\nThe compiler must parse .node files, validate the graph, resolve evidence anchors, and render HTML documentation.\n\n[edge:compiler-pipeline] [edge:compiler]",
    );

    write_node(
        &graph_dir.join("adr/compiler-pipeline.node"),
        "compiler-pipeline",
        "adr",
        "# Compiler Pipeline\n\nThe compiler pipeline has four stages:\n\n1. **Parse**: Read .node files into a Graph\n2. **Validate**: Run structural checks\n3. **Resolve**: Match evidence anchors to source\n4. **Render**: Generate static HTML\n\nRelated: [edge:compiler-requirement]",
    );

    write_node(
        &graph_dir.join("service/compiler.node"),
        "compiler",
        "service",
        "# Compiler\n\nA Rust CLI that implements the graphite compiler pipeline.",
    );

    write_node(
        &graph_dir.join("test/compiler-tests.node"),
        "compiler-tests",
        "test",
        "# Compiler Tests\n\nTests for the graphite compiler pipeline.",
    );

    write_node(
        &graph_dir.join("compliance/traceability-policy.node"),
        "traceability-policy",
        "compliance",
        "# Traceability Policy\n\nRequirements should be linked to implementation, tests, and evidence.",
    );

    write_node(
        &graph_dir.join("runbook/validation-runbook.node"),
        "validation-runbook",
        "runbook",
        "# Validation Runbook\n\nRun graphite validate before shipping graph changes.",
    );

    write_node(
        &graph_dir.join("infra/distribution-pipeline.node"),
        "distribution-pipeline",
        "infra",
        "# Distribution Pipeline\n\nThe package and binary distribution path for graphite.",
    );

    println!("✓ created Graphite project at '{}'", root.display());
}

fn write_node(path: &Path, id: &str, category: &str, body: &str) {
    let frontmatter = format!("---\nid: {id}\ncategory: {category}\n---\n");
    let content = format!("{frontmatter}{body}\n");
    let _ = fs::write(path, content);
}

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct DiffOutput {
    new: Vec<String>,
    changed: Vec<String>,
    removed: Vec<String>,
}

// @graphite:evidence spec-diff
fn cmd_diff(from: &str, json: bool) {
    let repo_root = match git_root() {
        Some(r) => r,
        None => {
            eprintln!("error: not in a git repository");
            std::process::exit(1);
        }
    };

    let output = std::process::Command::new("git")
        .args(["diff", "--name-status", from, "--", "*.node"])
        .current_dir(&repo_root)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            eprintln!("error: git diff failed (is '{from}' a valid ref?)");
            std::process::exit(1);
        }
    };

    let mut new_nodes = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let status = parts[0];
        let file = parts[1];
        let file = file.trim_start_matches("graph/");
        match status.chars().next() {
            Some('A') => new_nodes.push(file.to_string()),
            Some('M' | 'R') => changed.push(file.to_string()),
            Some('D') => removed.push(file.to_string()),
            _ => {}
        }
    }

    let diff = DiffOutput {
        new: new_nodes,
        changed,
        removed,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&diff).unwrap());
    } else {
        for f in &diff.new {
            println!("  NEW    {f}");
        }
        for f in &diff.changed {
            println!("  CHANGED {f}");
        }
        for f in &diff.removed {
            println!("  REMOVED {f}");
        }
        if diff.new.is_empty() && diff.changed.is_empty() && diff.removed.is_empty() {
            println!("  (no changes)");
        }
    }
}

#[derive(serde::Serialize)]
struct StatsOutput {
    nodes_total: usize,
    nodes_by_category: HashMap<String, usize>,
    edges_total: usize,
    edges_by_kind: HashMap<String, usize>,
    diagnostics_total: usize,
    diagnostics_by_rule: HashMap<String, usize>,
    diagnostics_by_severity: HashMap<String, usize>,
    nodes_with_evidence_pct: f64,
}

// @graphite:evidence spec-stats
fn cmd_stats(graph_dir: &str, json: bool, config: &Config) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            output_diagnostics(&errors, None, false, json);
            std::process::exit(1);
        }
    };

    let engine = ValidationEngine;
    let mut diagnostics = engine.validate(&graph);
    let base_dir = Path::new(graph_dir)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let evidence = resolve_evidence(&config.scan, base_dir);
    diagnostics.extend(engine.check_evidence_anchors(&graph, &evidence));

    let mut nodes_by_category: HashMap<String, usize> = HashMap::new();

    for node in graph.nodes.values() {
        *nodes_by_category.entry(node.category.clone()).or_insert(0) += 1;
    }

    let mut diagnostics_by_rule: HashMap<String, usize> = HashMap::new();
    let mut diagnostics_by_severity: HashMap<String, usize> = HashMap::new();
    for d in &diagnostics {
        *diagnostics_by_rule.entry(d.rule.clone()).or_insert(0) += 1;
        let sev = match d.severity {
            Severity::Error => "error".to_string(),
            Severity::Warning => "warning".to_string(),
        };
        *diagnostics_by_severity.entry(sev).or_insert(0) += 1;
    }

    let out = StatsOutput {
        nodes_total: graph.nodes.len(),
        nodes_by_category,
        edges_total: 0,
        edges_by_kind: HashMap::new(),
        diagnostics_total: diagnostics.len(),
        diagnostics_by_rule,
        diagnostics_by_severity,
        nodes_with_evidence_pct: 0.0,
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&out).expect("serialize stats")
        );
    } else {
        println!("nodes: {}", out.nodes_total);
        println!("edges: {}", out.edges_total);
        println!("diagnostics: {}", out.diagnostics_total);
        println!("nodes with evidence: {:.1}%", out.nodes_with_evidence_pct);
    }
}

fn git_root() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(PathBuf::from(path))
    } else {
        None
    }
}

fn extract_edge_refs_from_body(body: &str) -> Vec<String> {
    extract_marker_refs(body, "[edge:")
}

fn extract_evidence_refs_from_body(body: &str) -> Vec<String> {
    extract_marker_refs(body, "[evidence:")
}

fn extract_marker_refs(body: &str, marker: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut pos = 0;
    while let Some(start) = body[pos..].find(marker) {
        let content_start = pos + start + marker.len();
        if let Some(rel_end) = body[content_start..].find(']') {
            let id = body[content_start..content_start + rel_end].trim();
            if !id.is_empty() {
                result.push(id.to_string());
            }
            pos = content_start + rel_end + 1;
        } else {
            break;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// context
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct EvidenceItem {
    id: String,
    file: String,
    line: usize,
}

#[derive(serde::Serialize)]
struct ContextOutput {
    nodes: Vec<ContextNode>,
    evidence: Vec<EvidenceItem>,
}

#[derive(serde::Serialize)]
struct ContextNode {
    id: String,
    category: String,
    file: String,
    body: String,
    relations: Vec<String>,
}

// @graphite:evidence spec-context
fn cmd_context(id: &str, graph_dir: &str, scan: &[String]) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let target = match graph.nodes.get(id) {
        Some(n) => n,
        None => {
            eprintln!("error: node '{id}' not found in graph");
            std::process::exit(1);
        }
    };

    let mut node_map: HashMap<String, Vec<String>> = HashMap::new();

    for target_ref in extract_edge_refs_from_body(&target.body) {
        if graph.nodes.contains_key(target_ref.as_str()) {
            node_map.entry(target_ref).or_default().push("forward".to_string());
        }
    }

    for node in graph.nodes.values() {
        if node.id == id {
            continue;
        }
        for target_ref in extract_edge_refs_from_body(&node.body) {
            if target_ref == id {
                node_map.entry(node.id.clone())
                    .or_default()
                    .push("reverse".to_string());
            }
        }
    }

    node_map.entry(id.to_string()).or_default().push("self".to_string());

    let base_dir = Path::new(graph_dir)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let evidence_map = resolve_evidence(scan, base_dir);

    let mut evidence_items: Vec<EvidenceItem> = Vec::new();
    let mut seen_evidence: HashSet<String> = HashSet::new();
    for eid in extract_evidence_refs_from_body(&target.body) {
        if !seen_evidence.insert(eid.clone()) {
            continue;
        }
        if let Some(locations) = evidence_map.get(eid.as_str()) {
            for (file_path, line) in locations {
                evidence_items.push(EvidenceItem {
                    id: eid.clone(),
                    file: file_path.to_string_lossy().to_string(),
                    line: *line,
                });
            }
        }
    }

    let mut sorted_ids: Vec<String> = node_map.keys().cloned().collect();
    sorted_ids.sort();
    let mut nodes: Vec<ContextNode> = Vec::with_capacity(sorted_ids.len());
    for nid in &sorted_ids {
        let relations = node_map.remove(nid.as_str()).unwrap();
        if let Some(n) = graph.nodes.get(nid.as_str()) {
            nodes.push(ContextNode {
                id: n.id.clone(),
                category: n.category.clone(),
                file: resolve_node_source_file(graph_dir, n),
                body: n.body.clone(),
                relations,
            });
        }
    }

    let output = ContextOutput {
        nodes,
        evidence: evidence_items,
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

// ---------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct PlanOutput {
    target: PlanTarget,
    work_order: Vec<WorkStep>,
}

#[derive(serde::Serialize)]
struct PlanTarget {
    id: String,
    category: String,
}

#[derive(serde::Serialize)]
struct WorkStep {
    order: usize,
    action: String,
    node: String,
    source_file: String,
    why: String,
}

// @graphite:evidence spec-plan
fn cmd_plan(id: &str, graph_dir: &str) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let target = match graph.nodes.get(id) {
        Some(n) => n,
        None => {
            eprintln!("error: node '{id}' not found in graph");
            std::process::exit(1);
        }
    };

    let mut work_order: Vec<WorkStep> = Vec::new();
    let mut order = 1;

    let mut seen = std::collections::HashSet::new();
    for t in extract_edge_refs_from_body(&target.body) {
        if seen.insert(t.clone()) {
            let source_file = graph
                .nodes
                .get(t.as_str())
                .map(|n| resolve_node_source_file(graph_dir, n))
                .unwrap_or_else(|| format!("graph/{category}/{t}.node", category = target.category));
            work_order.push(WorkStep {
                order,
                action: "read".into(),
                node: t.clone(),
                source_file,
                why: format!("Understand context for '{}'", t),
            });
            order += 1;
        }
    }

    // Phase 2: Modify the target node
    let target_file = resolve_node_source_file(graph_dir, target);
    work_order.push(WorkStep {
        order,
        action: "modify".into(),
        node: id.to_string(),
        source_file: target_file,
        why: format!("Implement or modify '{}'", id),
    });
    order += 1;

    // Phase 3: Evidence anchors
    work_order.push(WorkStep {
        order,
        action: "anchor".into(),
        node: id.to_string(),
        source_file: String::new(),
        why: format!(
            "Add @graphite:evidence anchors for '{}' in source files",
            id
        ),
    });
    order += 1;

    // Phase 4: Validate
    work_order.push(WorkStep {
        order,
        action: "validate".into(),
        node: id.to_string(),
        source_file: String::new(),
        why: "Run `graphite validate --first` and fix errors iteratively".into(),
    });

    let output = PlanOutput {
        target: PlanTarget {
            id: id.to_string(),
            category: target.category.clone(),
        },
        work_order,
    };

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

// @graphite:evidence spec-render
fn cmd_render(graph_dir: &str, output: &str, style_arg: &str, config: &Config) {
    let graph = match load_graph(graph_dir) {
        Ok(g) => g,
        Err(errors) => {
            for d in &errors {
                let sev = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                eprintln!("[{sev}] {}: {}", d.rule, d.detail);
            }
            std::process::exit(1);
        }
    };

    let base_dir = Path::new(graph_dir)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let _evidence = resolve_evidence(&config.scan, base_dir);
    let output_path = Path::new(output);

    let repo_url = config.repo_url.as_deref();
    let css = match style_arg {
        "sci-fi" => style::SCI_FI_CSS.to_string(),
        "default" => style::DEFAULT_CSS.to_string(),
        path => match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("[error] style-read-error: Cannot read CSS file '{path}': {e}");
                eprintln!(
                    "  fix: pass --style default, --style sci-fi, or a readable CSS file path"
                );
                std::process::exit(1);
            }
        },
    };

    if let Err(d) = graphite_render::render_to_dir(&graph, output_path, repo_url, &css, &config.base_url) {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        eprintln!("[{sev}] {}: {}", d.rule, d.detail);
        eprintln!("  fix: {}", d.fix);
        eprintln!("  hint: {}", d.hint);
        std::process::exit(1);
    }

    println!("✓ rendered {} nodes to '{}'", graph.nodes.len(), output);
}

fn resolve_node_source_file(graph_dir: &str, node: &graphite_core::Node) -> String {
    let root = Path::new(graph_dir);
    let preferred = root.join(&node.category).join(format!("{}.node", node.id));

    if preferred.exists() {
        return preferred.to_string_lossy().into();
    }

    if let Some(found) = find_node_file(root, &node.id) {
        return found.to_string_lossy().into();
    }

    format!("graph/{}/{}.node", node.category, node.id)
}

fn find_node_file(root: &Path, node_id: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str())
                && (name.starts_with('.') || name == "target" || name == "node_modules")
            {
                continue;
            }
            if let Some(found) = find_node_file(&path, node_id) {
                return Some(found);
            }
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem == node_id
            && path.extension().and_then(|s| s.to_str()) == Some("node")
        {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn self_hosting_validation() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let graph_dir = workspace_root.join("graph");
        let config = Config::load_or_default(workspace_root).expect("load config");
        let ok = cmd_validate(
            graph_dir.to_str().unwrap(),
            None,
            false,
            false,
            false,
            None,
            &config,
        );
        assert!(ok, "self-hosting validation failed on graph/");
    }

    #[test]
    fn full_pipeline_integration() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let graph_dir = root.join("graph");

        cmd_init(root.to_str().unwrap(), false);
        assert!(graph_dir.exists(), "init should create graph/ directory");

        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("compiler.rs"),
            "// @graphite:evidence compiler-impl\n// @graphite:evidence compiler-tests-impl\n// @graphite:evidence pipeline-impl\n// @graphite:evidence policy-impl\n// @graphite:evidence runbook-impl\n// @graphite:evidence distro-impl\nfn compile() {}\n",
        )
        .unwrap();

        // Update all nodes created by init to have evidence edges
        let req_path = graph_dir.join("requirement/compiler-requirement.node");
        fs::write(
            &req_path,
            r#"---
id: compiler-requirement
category: requirement
---
# Compiler Requirement

The compiler must parse and validate [edge:compiler] and [edge:compiler-tests].

[evidence:compiler-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("adr/compiler-pipeline.node"),
            r#"---
id: compiler-pipeline
category: adr
---
# Compiler Pipeline

The compiler pipeline has four stages:

1. **Parse**: Read .node files into a Graph
2. **Validate**: Run structural checks
3. **Resolve**: Match evidence anchors to source
4. **Render**: Generate static HTML

Related: [edge:compiler-requirement]

[evidence:pipeline-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("service/compiler.node"),
            r#"---
id: compiler
category: service
---
# Compiler

A Rust CLI that implements the graphite compiler pipeline.

[evidence:compiler-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("test/compiler-tests.node"),
            r#"---
id: compiler-tests
category: test
---
# Compiler Tests

Tests for the graphite compiler pipeline.

[evidence:compiler-tests-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("compliance/traceability-policy.node"),
            r#"---
id: traceability-policy
category: compliance
---
# Traceability Policy

Requirements should be linked to implementation, tests, and evidence.

[evidence:policy-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("runbook/validation-runbook.node"),
            r#"---
id: validation-runbook
category: runbook
---
# Validation Runbook

Run graphite validate before shipping graph changes.

[evidence:runbook-impl]
"#,
        )
        .unwrap();

        fs::write(
            graph_dir.join("infra/distribution-pipeline.node"),
            r#"---
id: distribution-pipeline
category: infra
---
# Distribution Pipeline

The package and binary distribution path for graphite.

[evidence:distro-impl]
"#,
        )
        .unwrap();

        let config = Config::default();
        let valid = cmd_validate(
            graph_dir.to_str().unwrap(),
            None,
            false,
            false,
            false,
            None,
            &config,
        );
        assert!(valid, "integration: validate should pass with valid graph");

        // Call context and plan on the requirement node.
        // These print JSON to stdout; on error they call process::exit(1) which
        // would abort the test, so completing means success.
        cmd_context(
            "compiler-requirement",
            graph_dir.to_str().unwrap(),
            &config.scan,
        );
        cmd_plan("compiler-requirement", graph_dir.to_str().unwrap());

        let graph = load_graph(graph_dir.to_str().unwrap()).expect("integration: load graph");
        let evidence = resolve_evidence(&config.scan, root);
        let output_dir = root.join("html");
        graphite_render::render_to_dir(&graph, &output_dir, None, style::DEFAULT_CSS, "")
            .expect("integration: render should succeed");

        assert!(
            output_dir.join("index.html").exists(),
            "integration: root index.html should exist"
        );
        let compiler_page = output_dir.join("service/compiler.html");
        assert!(
            compiler_page.exists(),
            "integration: service/compiler.html should exist"
        );
        let html = fs::read_to_string(&compiler_page).unwrap();
        assert!(
            html.contains("href="),
            "integration: rendered page should have hyperlinks: {}",
            html
        );
    }

    #[test]
    fn load_graph_reports_schema_parse_error() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let graph_dir = root.join("graph");
        fs::create_dir_all(&graph_dir).expect("create graph dir");

        fs::write(root.join("graph.schema"), "kinds:\n  bad: [\n").expect("write invalid schema");

        let result = load_graph(graph_dir.to_str().expect("graph path"));
        let errors = result.expect_err("invalid graph.schema should fail load_graph");
        assert!(
            errors.iter().any(|d| d.rule == "schema-parse-error"),
            "expected schema-parse-error, got: {:?}",
            errors
        );
    }

    #[test]
    fn strict_mode_turns_warnings_into_failure() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let graph_dir = root.join("graph");
        let dirs_test = ["graph/spec", "graph/adr"];
        for d in &dirs_test {
            fs::create_dir_all(root.join(d)).unwrap();
        }

        fs::write(
            &graph_dir.join("spec/root.node"),
            r#"---
id: root
category: spec
---
"#,
        )
        .expect("write root");
        fs::write(
            graph_dir.join("adr/svc.node"),
            "---\nid: svc\ncategory: adr\n---\n",
        )
        .expect("write service");

        let cfg = Config::default();
        let non_strict = cmd_validate(
            graph_dir.to_str().expect("graph path"),
            None,
            false,
            false,
            false,
            None,
            &cfg,
        );
        assert!(non_strict, "warnings should not fail non-strict validation");

        let strict = cmd_validate(
            graph_dir.to_str().expect("graph path"),
            None,
            false,
            true,
            false,
            None,
            &cfg,
        );
        assert!(!strict, "strict validation should fail on warnings");
    }
}
