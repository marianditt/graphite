use std::collections::HashMap;
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

// @graphite:evidence cli-simple-req-ev
// @graphite:evidence clap-cli-arc-ev
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
    /// Show context for a node (slice relevant for a specific phase)
    Context {
        id: String,
        #[arg(long)]
        phase: Option<String>,
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
        Commands::Context {
            id,
            phase,
            graph_dir,
        } => {
            cmd_context(&id, phase.as_deref(), &graph_dir);
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
    if path.is_dir() {
        #[allow(clippy::collapsible_if)]
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                return;
            }
        }
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                collect_node_files(&entry.path(), graph, errors);
            }
        }
        return;
    }

    let ext = path.extension().and_then(|e| e.to_str());
    if ext != Some("node") && ext != Some("index") {
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
        Ok(node) => graph.add_node(node),
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

    evidence
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

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

fn check_compatibility(graph: &Graph, compat: Option<&str>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if compat != Some("0.1.0") {
        return diagnostics;
    }

    for node in graph.nodes.values() {
        if node.edges.contains_key("evidence") {
            diagnostics.push(Diagnostic {
                rule: "compat-0.1.0".into(),
                severity: Severity::Warning,
                node_id: Some(node.id.clone()),
                file: None,
                detail: format!(
                    "Node '{}' uses 'evidence' edges, which graphite 0.1.0 does not validate correctly.",
                    node.id
                ),
                fix: "Use the local CLI wrapper (current repo version) for accurate evidence validation, or upgrade the installed binary.".into(),
                example: None,
                hint: "0.1.0 uses a fixed built-in schema path that misses newer edge types.".into(),
            });
        }
    }

    diagnostics
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
        "graph/root",
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

    // root/root.node
    write_node(
        &graph_dir.join("root/root.node"),
        "root",
        "index",
        &[(
            "contains",
            &[
                "requirement",
                "adr",
                "service",
                "test",
                "compliance",
                "runbook",
                "infra",
            ],
        )],
        "# Graphite\n\nA compiled knowledge graph for software engineering.",
        Some("general"),
    );

    // requirement/requirement.index
    write_node(
        &graph_dir.join("requirement/requirement.index"),
        "requirement",
        "index",
        &[("contains", &["compiler-requirement"])],
        "# Requirement Index",
        Some("requirement"),
    );

    write_node(
        &graph_dir.join("requirement/compiler-requirement.node"),
        "compiler-requirement",
        "requirement",
        &[],
        "# Compiler Requirement\n\nThe compiler must parse .node files, validate the graph, resolve evidence anchors, and render HTML documentation.",
        None,
    );

    // adr/adr.index
    write_node(
        &graph_dir.join("adr/adr.index"),
        "adr",
        "index",
        &[("contains", &["compiler-pipeline"])],
        "# Architecture Decision Records",
        Some("adr"),
    );

    write_node(
        &graph_dir.join("adr/compiler-pipeline.node"),
        "compiler-pipeline",
        "adr",
        &[("relates_to", &["compiler-requirement"])],
        "# Compiler Pipeline\n\nThe compiler pipeline has four stages:\n\n1. **Parse**: Read .node files into a Graph\n2. **Validate**: Run structural checks\n3. **Resolve**: Match evidence anchors to source\n4. **Render**: Generate static HTML\n\nRelated: [edge:compiler-requirement]",
        None,
    );

    // service/service.index
    write_node(
        &graph_dir.join("service/service.index"),
        "service",
        "index",
        &[("contains", &["compiler"])],
        "# Service Index",
        Some("service"),
    );

    write_node(
        &graph_dir.join("service/compiler.node"),
        "compiler",
        "service",
        &[],
        "# Compiler\n\nA Rust CLI that implements the graphite compiler pipeline.",
        None,
    );

    // test/test.index
    write_node(
        &graph_dir.join("test/test.index"),
        "test",
        "index",
        &[("contains", &["compiler-tests"])],
        "# Test Index",
        Some("test"),
    );

    write_node(
        &graph_dir.join("test/compiler-tests.node"),
        "compiler-tests",
        "test",
        &[],
        "# Compiler Tests\n\nTests for the graphite compiler pipeline.",
        None,
    );

    write_node(
        &graph_dir.join("compliance/compliance.index"),
        "compliance",
        "index",
        &[("contains", &["traceability-policy"])],
        "# Compliance Index",
        Some("compliance"),
    );

    write_node(
        &graph_dir.join("compliance/traceability-policy.node"),
        "traceability-policy",
        "compliance",
        &[],
        "# Traceability Policy\n\nRequirements should be linked to implementation, tests, and evidence.",
        None,
    );

    write_node(
        &graph_dir.join("runbook/runbook.index"),
        "runbook",
        "index",
        &[("contains", &["validation-runbook"])],
        "# Runbook Index",
        Some("runbook"),
    );

    write_node(
        &graph_dir.join("runbook/validation-runbook.node"),
        "validation-runbook",
        "runbook",
        &[],
        "# Validation Runbook\n\nRun graphite validate before shipping graph changes.",
        None,
    );

    write_node(
        &graph_dir.join("infra/infra.index"),
        "infra",
        "index",
        &[("contains", &["distribution-pipeline"])],
        "# Infra Index",
        Some("infra"),
    );

    write_node(
        &graph_dir.join("infra/distribution-pipeline.node"),
        "distribution-pipeline",
        "infra",
        &[],
        "# Distribution Pipeline\n\nThe package and binary distribution path for graphite.",
        None,
    );

    println!("✓ created Graphite project at '{}'", root.display());
}

fn write_node(
    path: &Path,
    id: &str,
    kind: &str,
    edges: &[(&str, &[&str])],
    body: &str,
    of_kind: Option<&str>,
) {
    let mut frontmatter = format!("---\nid: {id}\nkind: {kind}\n");
    if let Some(ok) = of_kind {
        frontmatter.push_str(&format!("metadata:\n  of_kind: {ok}\n"));
    }
    if !edges.is_empty() {
        frontmatter.push_str("edges:\n");
        for (edge_kind, targets) in edges {
            frontmatter.push_str(&format!("  {edge_kind}:\n"));
            for t in *targets {
                frontmatter.push_str(&format!("    - {t}\n"));
            }
        }
    }
    frontmatter.push_str("---\n");
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
    nodes_by_kind: HashMap<String, usize>,
    edges_total: usize,
    edges_by_kind: HashMap<String, usize>,
    diagnostics_total: usize,
    diagnostics_by_rule: HashMap<String, usize>,
    diagnostics_by_severity: HashMap<String, usize>,
    nodes_with_evidence_pct: f64,
}

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

    let mut nodes_by_kind: HashMap<String, usize> = HashMap::new();
    let mut edges_by_kind: HashMap<String, usize> = HashMap::new();
    let mut edges_total = 0usize;
    let mut nodes_with_evidence = 0usize;

    for node in graph.nodes.values() {
        *nodes_by_kind.entry(node.kind.clone()).or_insert(0) += 1;

        if node
            .edges
            .get("evidence")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        {
            nodes_with_evidence += 1;
        }

        for (edge_kind, targets) in &node.edges {
            *edges_by_kind.entry(edge_kind.clone()).or_insert(0) += targets.len();
            edges_total += targets.len();
        }
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

    let nodes_with_evidence_pct = if graph.nodes.is_empty() {
        100.0
    } else {
        (nodes_with_evidence as f64 / graph.nodes.len() as f64) * 100.0
    };

    let out = StatsOutput {
        nodes_total: graph.nodes.len(),
        nodes_by_kind,
        edges_total,
        edges_by_kind,
        diagnostics_total: diagnostics.len(),
        diagnostics_by_rule,
        diagnostics_by_severity,
        nodes_with_evidence_pct,
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

// ---------------------------------------------------------------------------
// context
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct ContextOutput {
    read: Vec<ContextNode>,
    modify: Vec<ContextNode>,
    validate: Vec<ContextNode>,
}

#[derive(serde::Serialize)]
struct ContextNode {
    id: String,
    kind: String,
    file: String,
    body: String,
}

fn cmd_context(id: &str, phase: Option<&str>, graph_dir: &str) {
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

    // Collect dependency nodes (incoming edges = this node depends on them)
    let mut deps: Vec<&graphite_core::Node> = Vec::new();
    for targets in target.edges.values() {
        for t in targets {
            if let Some(n) = graph.nodes.get(t.as_str())
                && !deps.iter().any(|d| d.id == n.id)
            {
                deps.push(n);
            }
        }
    }

    // Collect dependent nodes (outgoing edges = nodes that depend on this node)
    let mut dependents: Vec<&graphite_core::Node> = Vec::new();
    for node in graph.nodes.values() {
        for targets in node.edges.values() {
            if targets.iter().any(|t| t == id) && !dependents.iter().any(|d| d.id == node.id) {
                dependents.push(node);
            }
        }
    }

    // Phase-specific slicing
    let ctx_node = |n: &graphite_core::Node| -> ContextNode {
        ContextNode {
            id: n.id.clone(),
            kind: n.kind.clone(),
            file: resolve_node_source_file(graph_dir, n),
            body: n.body.clone(),
        }
    };

    match phase {
        Some("understand") => {
            let output: Vec<ContextNode> = std::iter::once(target)
                .chain(deps.iter().copied())
                .map(ctx_node)
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": output })).unwrap()
            );
        }
        Some("plan") => {
            let mut all: Vec<ContextNode> = Vec::new();
            all.push(ctx_node(target));
            all.extend(deps.iter().copied().map(ctx_node));
            all.extend(dependents.iter().copied().map(ctx_node));
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": all })).unwrap()
            );
        }
        Some("implement") => {
            let mut modify: Vec<ContextNode> = Vec::new();
            modify.push(ctx_node(target));
            modify.extend(deps.iter().copied().map(ctx_node));
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "modify": modify })).unwrap()
            );
        }
        Some("validate") => {
            let output: Vec<ContextNode> = dependents
                .iter()
                .copied()
                .chain(dependents.iter().copied())
                .map(ctx_node)
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "nodes": output })).unwrap()
            );
        }
        Some(other) => {
            eprintln!(
                "error: unknown phase '{other}'. Valid phases: understand, plan, implement, validate"
            );
            std::process::exit(1);
        }
        None => {
            let output = ContextOutput {
                read: deps.iter().copied().map(ctx_node).collect(),
                modify: vec![ctx_node(target)],
                validate: dependents
                    .iter()
                    .copied()
                    .chain(dependents.iter().copied())
                    .map(ctx_node)
                    .collect(),
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
    }
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
    kind: String,
}

#[derive(serde::Serialize)]
struct WorkStep {
    order: usize,
    action: String,
    node: String,
    source_file: String,
    why: String,
}

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

    // Phase 1: Read context nodes (dependencies)
    let mut seen = std::collections::HashSet::new();
    for targets in target.edges.values() {
        for t in targets {
            if seen.insert(t.clone()) {
                let source_file = graph
                    .nodes
                    .get(t.as_str())
                    .map(|n| resolve_node_source_file(graph_dir, n))
                    .unwrap_or_else(|| format!("graph/{kind}/{t}.node", kind = target.kind));
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
            kind: target.kind.clone(),
        },
        work_order,
    };

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

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
    let evidence = resolve_evidence(&config.scan, base_dir);
    let output_path = Path::new(output);

    let repo_url = std::env::var("GRAPHITE_REPO_URL").ok();
    let repo_url = repo_url.as_deref();
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

    if let Err(d) = graphite_render::render_to_dir(&graph, &evidence, output_path, repo_url, &css) {
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
    let preferred = [
        root.join(&node.kind).join(format!("{}.node", node.id)),
        root.join(&node.kind).join(format!("{}.index", node.id)),
    ];

    for p in preferred {
        if p.exists() {
            return p.to_string_lossy().into();
        }
    }

    if let Some(found) = find_node_file(root, &node.id) {
        return found.to_string_lossy().into();
    }

    format!("graph/{}/{}.node", node.kind, node.id)
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
        {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("node") || ext == Some("index") {
                return Some(path);
            }
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
            "// @graphite:evidence compiler-impl\nfn compile() {}\n",
        )
        .unwrap();

        let req_path = graph_dir.join("requirement/compiler-requirement.node");
        fs::write(
            &req_path,
            r#"---
id: compiler-requirement
kind: requirement
edges:
  implemented_by:
    - compiler
  verified_by:
    - compiler-tests
---
# Compiler Requirement

The compiler must parse and validate [edge:compiler] and [edge:compiler-tests].
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

        // Call context --phase implement and plan on the requirement node.
        // These print JSON to stdout; on error they call process::exit(1) which
        // would abort the test, so completing means success.
        cmd_context(
            "compiler-requirement",
            Some("implement"),
            graph_dir.to_str().unwrap(),
        );
        cmd_plan("compiler-requirement", graph_dir.to_str().unwrap());

        let graph = load_graph(graph_dir.to_str().unwrap()).expect("integration: load graph");
        let evidence = resolve_evidence(&config.scan, root);
        let output_dir = root.join("html");
        graphite_render::render_to_dir(&graph, &evidence, &output_dir, None, style::DEFAULT_CSS)
            .expect("integration: render should succeed");

        assert!(
            output_dir.join("index.html").exists(),
            "integration: root index.html should exist"
        );
        assert!(
            output_dir.join("requirement/index.html").exists(),
            "integration: requirement kind index should exist"
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
        fs::create_dir_all(graph_dir.join("root")).expect("root dir");
        fs::create_dir_all(graph_dir.join("adr")).expect("adr dir");

        fs::write(
            graph_dir.join("root/root.node"),
            "---\nid: root\nkind: index\nmetadata:\n  of_kind: general\nedges:\n  contains:\n    - svc\n---\n# Root\n",
        )
        .expect("write root");
        fs::write(
            graph_dir.join("adr/svc.node"),
            "---\nid: svc\nkind: adr\nedges:\n  references:\n    - root\n---\n# ADR\n",
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
