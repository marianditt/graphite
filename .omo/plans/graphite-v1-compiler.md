# graphite-v1-compiler - Work Plan

## TL;DR (For humans)

**What you'll get:** A Rust CLI tool called `graphite` that reads typed knowledge files (.node), validates them like a compiler, resolves evidence anchors in source code, and generates a static HTML documentation site. Plus a self-describing 21-node knowledge graph that documents Graphite itself — so the tool describes its own design.

**Why this approach:** Graphite is built for AI agents. The compiler does all the reasoning — the AI reads pre-computed JSON context, follows ordered work plans, and fixes one tutored error at a time. The graph is the permanent record; planning files (.omo) are temporary scaffolding.

**What it will NOT do:** No interactive dashboards, no NLP/AI inside the compiler, no file watching, no database — it's a stateless offline CLI.

**Effort:** Large (19 todos, 6 waves)
**Risk:** Medium — novel concept, strong design convergence, straightforward Rust implementation.
**Decisions to sanity-check:** Edge kind names (implemented_by, verified_by, describes, references) — read-in-english test matters more than consistency.

Your next move: approve, then start execution. Full detail below.

---

> TL;DR (machine): Large effort, medium risk. Rust CLI compiler: parse .node files → validate graph with tutoring diagnostics → resolve @graphite:evidence anchors → render static HTML. 5 CLI commands (init, validate --focus/--first, context --phase, plan, diff). 21-node seed graph describing Graphite itself. 19 todos, 6 waves.

## Scope
### Must have
- Graphite v1 compiler in Rust: parse .node files, validate graph, resolve anchors
- Tutoring error format: every Diagnostic has fields rule, severity, node_id, file, detail, fix, example, hint
- CLI: init, validate (with --focus, --first), context (with --phase), plan, diff
- HTML renderer with heading depth derivation
- Seed graph: .node files describing Graphite itself (self-hosting)
- Self-hosting validation: compiler validates its own graph

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No interactive HTML / JS
- No NLP/AI integration in graphite plan (plan is pure graph traversal)
- No Mermaid diagram generation
- No AST-level anchor extraction (line-based regex only)
- No database / persistence
- No plugin system
- No file watching

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after + Rust native testing (cargo test)
- Evidence: .omo/evidence/task-<N>-graphite-v1-compiler.<ext>

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

Wave 1: Project scaffold + data model (todos 1-3)
Wave 2: Parsers + anchor scanner (todos 4-6)
Wave 3: Validation engine (todos 7-9)
Wave 4: CLI commands (todos 10-14)
Wave 5: Renderer + seed graph (todos 15-17)
Wave 6: Self-hosting + final integration (todos 18-19)

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. Rust scaffold | - | 2, 3 | - |
| 2. Graph data model | 1 | 4, 5, 7 | 3 |
| 3. Schema parser | 1 | 7 | 2 |
| 4. .node file parser | 2 | 7 | 5 |
| 5. Anchor scanner | 2 | 9 | 4 |
| 6. Sidecar anchor support | 5 | 9 | - |
| 7. Validation engine | 2, 3, 4 | 10, 11, 12, 13 | 9 |
| 8. Body-edge usage validator | 4 | 7 | - |
| 9. Anchor resolution | 5, 6 | 10, 11, 12, 13 | 7 |
| 10. graphite init | 7, 9 | 15 | 11, 12, 13, 14 |
| 11. graphite validate (+ tutoring errors) | 7, 9 | 15 | 10, 12, 13, 14 |
| 12. graphite context (with --phase) | 7, 9 | 15 | 10, 11, 13, 14 |
| 13. graphite plan (graph traversal) | 7, 9 | 15 | 10, 11, 12, 14 |
| 14. graphite diff | 7, 9 | 15 | 10, 11, 12, 13 |
| 15. HTML renderer | 10, 11, 12, 13, 14 | 16 | - |
| 16. Seed graph (self-describing) | 15 | 17 | - |
| 17. Self-hosting validation | 16 | 18 | - |
| 18. Integration test | 17 | 19 | - |
| 19. Final verification wave | 18 | - | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. Scaffold Rust workspace and core data model
  What to do / Must NOT do: Create Cargo workspace with three crates: graphite-core (data model), graphite-cli (binary), graphite-render (HTML output). Define the core types in graphite-core: Node { id: String, kind: String, body: String, edges: HashMap<String, Vec<String>>, metadata: HashMap }, EdgeKind, Schema, KindDef, Index. Define the Diagnostic struct with tutoring fields: rule (String), severity (enum Error|Warning), node_id (Option<String>), file (Option<String>), detail (String), fix (String), example (Option<String>), hint (String). Define built-in kinds: "index" (with of_kind attribute), "evidence" (anchor target). Must NOT implement any parsing or validation logic yet — types only.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2, 3
  References: Converged design — .node file has YAML frontmatter + Markdown body, edges are HashMap<edge_kind, Vec<target_id>>, index nodes have of_kind + contains only, knowledge nodes forbid contains. Diagnostic must answer 5 questions: what rule, what's wrong, how to fix, correct example, general principle.
  Acceptance criteria (agent-executable): cargo build succeeds. Unit test creates a Node struct and serializes/deserializes it with serde. Unit test creates a Diagnostic with all tutoring fields and serializes to JSON.
  QA scenarios (name the exact tool + invocation): happy: cargo test creates Node and Diagnostic, serializes/deserializes. failure: malformed Node struct rejected at compile time. Evidence .omo/evidence/task-1-graphite-v1-compiler.txt
  Commit: Y | feat(core): scaffold workspace and define graph data model + tutoring Diagnostic type

- [ ] 2. Implement graph.schema parser
  What to do / Must NOT do: Parse graph.schema YAML into Schema struct. Define KindDef { key: String }, EdgeDef { from: String, to: String }. Handle built-in edge kind "contains" (for index nodes only). Handle built-in target kind "evidence" (for anchor resolution). Validate at parse time: no duplicate kind names, no duplicate edge names. Must NOT accept edges referencing undefined kinds. The default schema for graphite init must define these exact kinds and edges:
    kinds: requirement (REQ), adr (ADR), service (SVC), test (TST).
    edges: implemented_by (requirement→service), verified_by (requirement→test|evidence), describes (adr→service), references (any→any).
  Must NOT accept edges referencing undefined kinds.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 7
  References: graph.schema defines kinds (with key) and edges (with from/to). "evidence" and "contains" are built-in. The schema is the graph type system. Edge names are past-participle adjectives (implemented_by, verified_by) that read naturally after "is" in prose.
  Acceptance criteria (agent-executable): cargo test parses the default schema correctly. Parsing an invalid schema (undefined kind in edge) returns a parse error with tutoring format.
  QA scenarios (agent-executable): happy: default schema parses correctly. failure: edge referencing undefined kind returns tutored error. Evidence .omo/evidence/task-2-graphite-v1-compiler.txt
  Commit: Y | feat(parse): implement graph.schema parser

- [ ] 3. Implement .node file parser
  What to do / Must NOT do: Parse .node files. Split YAML frontmatter (between --- delimiters) from Markdown body (everything after second ---). Parse YAML into Node struct. Validate required fields: id, kind. Validate edges field if present: must be a map of edge_kind to array of target_ids. For index nodes (kind: index), validate of_kind is present and contains is the only allowed edge. For knowledge nodes, validate contains is absent. Must NOT execute any graph-level validation (that's the validation engine).
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 7
  References: Converged design — split YAML frontmatter from Markdown body. Index nodes have of_kind + contains. Knowledge nodes forbid contains.
  Acceptance criteria (agent-executable): cargo test parses a valid .node file into Node struct. Parsing a knowledge node with contains edge returns an error. Parsing a file without frontmatter returns an error.
  QA scenarios (agent-executable): happy: standard .node file parses. failure: knowledge node with contains rejected. Evidence .omo/evidence/task-3-graphite-v1-compiler.txt
  Commit: Y | feat(parse): implement .node file parser

- [ ] 4. Implement anchor scanner for @graphite:evidence in source files
  What to do / Must NOT do: Scan source files recursively from configured paths (src/, tests/, config/). Use line-based regex scanning: `// @graphite:evidence (\S+)` for line-comment languages, `# @graphite:evidence (\S+)` for hash-comment languages, `<!-- @graphite:evidence (\S+) -->` for HTML/XML. Return a map of evidence_id to Vec<(file_path, line_number)>. Handle multiple matches: if an evidence_id appears more than once, collect all locations. Must NOT modify source files. Must NOT require file extension mapping — scan everything and try all comment styles.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 9
  References: Converged design — `// @graphite:evidence <id>` in source, compiler scans and resolves. No line numbers in graph files, only compiler-internal.
  Acceptance criteria (agent-executable): cargo test creates a temp dir with source files containing @graphite:evidence, scanner finds them correctly. Evidence referenced in no file returns empty Vec.
  QA scenarios (agent-executable): happy: scanner finds evidence in .ts, .rs, .py, .c files. failure: file without any evidence returns empty. Evidence .omo/evidence/task-4-graphite-v1-compiler.txt
  Commit: Y | feat(anchors): implement @graphite:evidence scanner for source files

- [ ] 5. Implement .graphite sidecar anchor support
  What to do / Must NOT do: Scan for *.graphite sidecar files alongside source files. Parse JSON sidecar: { "anchors": { "<id>": { "pattern": "<regex_or_jsonpath>" } } }. Support simple regex patterns (match first line). Support jsonpath patterns (parse jsonpath, navigate JSON file). Resolve to file:line. Validate: every anchor must resolve to exactly one location. Zero matches = error. Multiple matches = error. Must NOT use line numbers in sidecar — only patterns and jsonpath.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 9
  References: Converged design — sidecar for JSON, .txt, and other comment-less formats. Same name as source file with .graphite appended. Regex or JSONPath only, no line numbers.
  Acceptance criteria (agent-executable): cargo test creates a .json file with .json.graphite sidecar, resolves the anchor correctly. Sidecar with unresolvable pattern returns an error. Sidecar with ambiguous pattern (multiple matches) returns an error.
  QA scenarios (agent-executable): happy: JSON file anchor resolved via jsonpath. failure: unreachable pattern returns error. Evidence .omo/evidence/task-5-graphite-v1-compiler.txt
  Commit: Y | feat(anchors): implement .graphite sidecar anchor resolution

- [ ] 6. Build the validation engine
  What to do / Must NOT do: Implement Graph struct that holds all parsed nodes and the schema. Implement validation passes: (a) Reachability — every node must be reachable from root via contains edges. (b) Tree constraint — contains edges form a tree (no node has multiple parents). (c) Cycle detection — no cycles in the graph. (d) Edge type conformance — every edge's source kind and target kind must be permitted by the schema. (e) contains edges only on index nodes, and only of_kind matching targets. (f) knowledge nodes cannot have contains. Each validation pass returns Vec<Diagnostic> using the tutoring type (rule, severity, node_id, file, detail, fix, example, hint). Must NOT modify nodes — validation is read-only.
  Parallelization: Wave 3 | Blocked by: 2, 3 | Blocks: 10, 11, 12, 13
  References: Converged design — reachability, tree constraint, no cycles, schema conformance, index vs knowledge node rules. Diagnostic tutoring fields defined in todo 1.
  Acceptance criteria (agent-executable): cargo test with valid graph returns no errors. Test with orphaned node returns Diagnostic with severity Error, rule "unreachable-node", detail explaining which node is orphaned, fix describing how to add a contains edge, example showing correct form, hint explaining reachability principle.
  QA scenarios (agent-executable): happy: valid graph passes all checks. failure: each validation rule violation returns a properly tutored Diagnostic. Evidence .omo/evidence/task-6-graphite-v1-compiler.txt
  Commit: Y | feat(validate): implement graph validation engine with tutoring diagnostics

- [ ] 7. Implement body-edge usage validator
  What to do / Must NOT do: After parsing all nodes, for each node, scan its Markdown body for [edge:<target_id>] patterns. For each edge declared in the node's YAML, verify the target_id appears at least once in the body. Error if an edge target is declared but never referenced (rule: "body-edge-usage"). Error if [edge:<target_id>] references a target_id not declared in edges (rule: "dangling-edge-reference"). Warning if [edge:<target_id>] references a node_id that doesn't exist in the graph (rule: "unknown-edge-target"). All diagnostics use the tutoring format with detail, fix, example, hint. Must NOT auto-render edges as sections — this is the only mechanism for edge usage.
  Parallelization: Wave 3 | Blocked by: 4 | Blocks: 7 | Can parallelize with: 6
  References: Converged design — "if a node has an edge, it must be used in the body at least once." [edge:<id>] syntax. Diagnostic tutoring fields defined in todo 1.
  Acceptance criteria (agent-executable): cargo test with body referencing all declared edges passes. Test with declared edge not used returns Diagnostic with rule "body-edge-usage", fix showing where to add [edge:<id>], example showing correct placement. Test with [edge:unknown] returns warning with did-you-mean suggestions.
  QA scenarios (agent-executable): happy: all edges used in body. failure: unused edge detected with tutoring. Evidence .omo/evidence/task-7-graphite-v1-compiler.txt
  Commit: Y | feat(validate): implement body-edge usage validator with tutoring

- [ ] 8. Implement anchor resolution validation
  What to do / Must NOT do: After anchor scanning (todo 4+5), validate that every edge targeting evidence kind resolves to at least one source location. For each node, for each edge with to: evidence, verify the target_id was found by the anchor scanner. Error if evidence anchor not found in any source file or sidecar (rule: "missing-evidence"). Error if evidence anchor found in multiple files (rule: "ambiguous-evidence"). Use tutoring format: detail stating exactly which evidence_id is missing, fix showing a @graphite:evidence comment example, hint about supported comment syntaxes per language. Collect all resolved anchors alongside their nodes for rendering.
  Parallelization: Wave 3 | Blocked by: 5 | Blocks: 10, 11, 12, 13 | Can parallelize with: 6, 7
  References: Converged design — verified_by targets evidence:id, compiler resolves via @graphite:evidence and .graphite sidecars. Diagnostic tutoring fields defined in todo 1.
  Acceptance criteria (agent-executable): cargo test with evidence present in source passes. Test with evidence missing returns Diagnostic with rule "missing-evidence", fix showing "Add // @graphite:evidence <id> above the relevant function", example showing the syntax.
  QA scenarios (agent-executable): happy: evidence anchor resolves. failure: missing evidence returns tutored error. Evidence .omo/evidence/task-8-graphite-v1-compiler.txt
  Commit: Y | feat(validate): implement anchor resolution validation with tutoring

- [ ] 9. Implement graphite init command
  What to do / Must NOT do: CLI subcommand creating a new Graphite project. Generate: graphite.yaml (default config), graph.schema (with example kinds: requirement, service, test, architecture + example edges: implements, verified_by, references), graph/root/root.node (root index), graph/requirement/requirement.index + example.node, graph/service/service.index + example.node, graph/test/test.index + example.node. Every generated .node file must be valid and compilable. Must NOT overwrite existing files without --force flag. Use clap for CLI argument parsing.
  Parallelization: Wave 4 | Blocked by: 1, 6 | Blocks: 13 | Can parallelize with: 10, 11
  References: Converged design — file layout with kind subdirectories, root.node, kind.index files, example .node files with annotated comments.
  Acceptance criteria (agent-executable): cargo run -- init /tmp/test-project creates the correct file structure. All generated .node files are valid. Running init again without --force returns error.
  QA scenarios (agent-executable): happy: scaffold created with valid files. failure: existing project without --force rejects. Evidence .omo/evidence/task-9-graphite-v1-compiler.txt
  Commit: Y | feat(cli): implement graphite init

- [ ] 10. Implement graphite validate command
  What to do / Must NOT do: CLI subcommand that runs all validation passes (todos 6, 7, 8) on a graph directory. Read graphite.yaml for source paths, anchor scan paths. Run reachability, tree constraint, cycle detection, schema conformance, body-edge usage, anchor resolution. Output diagnostics to stdout using the tutoring format (every Diagnostic includes rule, severity, node_id, file, detail, fix, example, hint). Support flags:
    --json — machine-readable output (Vec<Diagnostic> serialized).
    --focus <id> — only diagnostics for that node and its direct dependencies.
    --first — stop after the first error, show its full tutoring output, exit 1.
    (default) — all errors, each with full tutoring fields.
  Exit code 0 if no errors, 1 if errors found. Must NOT run renderer.
  Parallelization: Wave 4 | Blocked by: 6, 7, 8 | Blocks: 15 | Can parallelize with: 9, 11, 12, 13, 14
  References: Converged design — validate as primary AI feedback loop. Diagnostic tutoring format from todo 1. --focus for targeted fixes, --first for one-error-at-a-time tutoring.
  Acceptance criteria (agent-executable): cargo run -- validate on valid graph returns exit 0. --first flag shows one error with detail+fix+example+hint. --focus <id> returns only diagnostics for that node. --json output is valid JSON array with all tutoring fields.
  QA scenarios (agent-executable): happy: valid graph passes. failure: broken graph with --first returns one tutored error. Evidence .omo/evidence/task-10-graphite-v1-compiler.txt
  Commit: Y | feat(cli): implement graphite validate with tutoring output and --focus/--first

- [ ] 11. Implement graphite context command with --phase flag
  What to do / Must NOT do: CLI subcommand: graphite context <node_id> [--phase <phase>]. Given a node ID, traverse the graph to determine what's relevant. The --phase flag slices the output to exactly what a weak model needs for one step:
    --phase understand — "What is this and why does it exist?" Returns target node body + incoming dependency nodes (what this node depends on).
    --phase plan — "What touches this?" Returns all connected nodes (incoming and outgoing edges, excluding evidence).
    --phase implement — "What do I change and where?" Returns target node body + outgoing edges to non-evidence targets + source file paths with existing evidence anchors.
    --phase validate — "How do I verify it works?" Returns test nodes that verify this node + evidence anchors that must be present.
  Without --phase, returns full context with sections { "read": [Node], "modify": [Node], "validate": [Node] }. Each node output includes id, kind, file path, and body. Every phase output must fit in a weak model's context window alone (no phase outputs more than ~100 lines). Must NOT output the entire graph — only relevant context.
  Parallelization: Wave 4 | Blocked by: 6, 8 | Blocks: 15 | Can parallelize with: 9, 10, 13, 14
  References: Converged design — phase-specific context slices for weak models. understand/plan/implement/validate phases from AI protocol discussion.
  Acceptance criteria (agent-executable): cargo run -- context checkout --phase implement returns JSON with modify section containing the target node's body, source file paths, and existing anchors. Phase output fits under 100 lines.
  QA scenarios (agent-executable): happy: each phase returns correct slice. failure: unknown phase returns error listing valid phases. Evidence .omo/evidence/task-11-graphite-v1-compiler.txt
  Commit: Y | feat(cli): implement graphite context with --phase flag

- [ ] 12. Implement graphite plan command
  What to do / Must NOT do: CLI subcommand: graphite plan <node_id>. Pre-computes a work order for implementing or modifying a node. This is pure graph traversal, NOT NLP/AI. Starting from the given node, traverse the graph to produce an ordered list of work steps. Each step has: order (integer), action ("read"|"modify"|"anchor"|"validate"), node (id), source_file (if applicable), and why (string explanation). The work order follows the two-phase protocol: (1) read context nodes first, (2) modify the target node, (3) add evidence anchors, (4) validate. Output as JSON: { "target": { "id", "kind" }, "work_order": [ { "order": 1, "action": "read", "node": "...", "why": "..." }, ... ] }.
  Must NOT call any external AI service. Must NOT generate code. Must NOT exceed ~50 lines of JSON output.
  Parallelization: Wave 4 | Blocked by: 6, 8 | Blocks: 15 | Can parallelize with: 9, 10, 11, 14
  References: Converged design — plan command as work order pre-computation. Two-phase protocol (graph first, code second).
  Acceptance criteria (agent-executable): cargo run -- plan checkout-service returns JSON work order with ordered steps (read requirement, read architecture, modify service, anchor evidence, validate). Output fits under 50 lines.
  QA scenarios (agent-executable): happy: plan returns ordered work order. failure: plan on non-existent node returns error. Evidence .omo/evidence/task-12-graphite-v1-compiler.txt
  Commit: Y | feat(cli): implement graphite plan (graph traversal work order)

- [ ] 13. Implement graphite diff command
  What to do / Must NOT do: CLI subcommand: graphite diff [--from <ref>]. Compare the current graph state against a git reference (default: HEAD). Show: NEW nodes (added since ref), CHANGED nodes (modified edges or body), REMOVED nodes (deleted since ref). For each change, show: node id, kind, what changed (edges added/removed, body changed). Also show validation status: which nodes have all required edges, which are missing evidence. Output in human-readable format by default, --json for machine-readable. Must NOT show source code diffs — only knowledge-level changes.
  Parallelization: Wave 4 | Blocked by: 6, 8 | Blocks: 15 | Can parallelize with: 9, 10, 11, 12
  References: Converged design — "graphite diff produces a knowledge-aware summary for the human auditor."
  Acceptance criteria (agent-executable): cargo run -- diff on a clean working tree shows no changes. After adding a new .node file, diff shows NEW node. After modifying edges, diff shows CHANGED. --json output is valid.
  QA scenarios (agent-executable): happy: clean tree diff is empty. failure: diff on non-git directory returns error (or gracefully handles). Evidence .omo/evidence/task-13-graphite-v1-compiler.txt
  Commit: Y | feat(cli): implement graphite diff

- [ ] 14. (reserved)

- [ ] 15. Implement HTML renderer with heading depth derivation
  What to do / Must NOT do: Given a validated Graph, generate static HTML documentation. For each node: (a) compute heading depth from containment tree position (root=0, child=parent+1), (b) offset all body headings (1 + depth), clamp at h6, (c) replace [edge:<id>] patterns with <a href="/<kind>/<id>"> links, (d) render body Markdown to HTML, (e) add backlinks section (nodes that reference this node via edges), (f) add evidence section (if node has verified_by edges, show resolved source locations). Generate index.html per kind index. Must NOT execute JavaScript. Must NOT require a web server — output is static files. Use pulldown-cmark for Markdown rendering. Use askama or tera for HTML templates.
  Parallelization: Wave 5 | Blocked by: 10, 11, 12, 13 | Blocks: 16
  References: Converged design — heading depth from containment tree, [edge:<id>] replacement, backlinks, auto-numbered IDs for display.
  Acceptance criteria (agent-executable): cargo test renders a valid .node file to HTML with correct heading levels. [edge:target] replaced with correct link. Index page shows TOC with DFS ordering. Backlinks section appears.
  QA scenarios (agent-executable): happy: rendered page has correct h1-h6 hierarchy, working links. failure: broken [edge:missing] handled gracefully (renders as text, not crash). Evidence .omo/evidence/task-15-graphite-v1-compiler.txt
  Commit: Y | feat(render): implement HTML renderer with heading depth derivation

- [ ] 16. Create the seed graph (self-describing Graphite)
  What to do / Must NOT do: Hand-author the .node files that describe Graphite itself. This is the PERSISTENT record. After .omo/ is deleted, THE GRAPH IS THE DOCUMENTATION. It must capture every concept the project needs to be understood. Do NOT use example/fictional content — every node is about Graphite itself. Uses these edge kinds: contains (built-in for indices), implemented_by (requirement→service), verified_by (requirement→test|evidence), describes (adr→service), references (any→any). Uses these node kinds: index (built-in), requirement, adr, service, test.
  Exact node set (21 nodes, every [edge:<id>] must resolve):
  
  **root:**
  (a) root/root.node — root index. Body: "A compiled knowledge graph for software engineering." Contains all four kind indices.
  
  **requirement:**
  (b) requirement/requirement.index — index. Contains all requirement nodes.
  (c) requirement/compiler-requirement.node — core requirement: compile a typed directed graph from .node files, validate its integrity, resolve evidence anchors, render navigable docs. Edges: implemented_by→compiler, verified_by→compiler-tests.
  (d) requirement/audience-requirement.node — AI-first protocol requirement: CLI output must fit weak model context windows, every command serves a specific work phase, every error teaches the fix. Edges: references→ai-protocol, references→cli, references→tutoring-errors.
  (e) requirement/self-hosting-requirement.node — self-describing constraint: graph/ is the canonical record, compiler validates its own graph in CI, no essential knowledge lives outside the graph. Edges: references→compiler-requirement.
  
  **adr (architecture decisions):**
  (f) adr/adr.index — index. Contains all ADR nodes.
  (g) adr/compiler-pipeline.node — four-stage pipeline: parse → validate → resolve → render. Each stage described with inputs/outputs. Edges: describes→compiler, references→edge-narrative, references→heading-depth, references→anchor-syntax, references→schema-design.
  (h) adr/edge-narrative.node — edges are narrative: every edge must be referenced inline via [edge:<id>], never auto-rendered as sections. Linter enforces usage. Renderer replaces with hyperlinks.
  (i) adr/heading-depth.node — heading derived from containment depth. Table of depth→rendered h-level. Author always writes #. Clamp at h6. Edges: references→rendering.
  (j) adr/anchor-syntax.node — @graphite:evidence for line-comment languages (//, #, <!-- -->), .graphite sidecar for comment-less formats. Regex or JSONPath only, never line numbers. Sidecar naming: source.ext → source.ext.graphite. Edges: describes→compiler.
  (k) adr/index-pattern.node — two mutually exclusive roles: index (contains only, no knowledge edges) and knowledge nodes (knowledge edges only, never contains). Sub-indices enable arbitrary nesting within a kind.
  (l) adr/ai-protocol.node — two-phase protocol detailed: Phase 1 (graph): plan → context --phase understand → edit .node → validate --first → repeat. Phase 2 (code): context --phase implement → edit source with anchors → validate → diff. The AI never searches, never guesses.
  (m) adr/tutoring-errors.node — every Diagnostic has 7 fields: rule, severity, node, file, detail, fix, example, hint. --first emits one error and exits 1. --focus <id> narrows to one node. Edges: describes→compiler.
  (n) adr/schema-design.node — the type system: kinds with keys (REQ, ADR, SVC, TST), edges with from/to. Built-in: contains and evidence. Every edge is validated against schema at compile time. Edges: describes→compiler, references→stable-identity, references→index-pattern.
  (o) adr/rendering.node — static HTML generation: TOC from DFS containment, pages per node, heading offset, [edge:<id>]→link, backlinks, evidence display, cosmetic auto-IDs. No JavaScript, no server. Edges: describes→compiler, references→heading-depth.
  (p) adr/stable-identity.node — every node has stable id: independent of filenames and directories. Cosmetic display IDs (REQ-1) are assigned by renderer in DFS order and shift on insertion. Permanent references use the stable id.
  
  **service:**
  (q) service/service.index — index. Contains compiler and cli.
  (r) service/compiler.node — the Rust CLI. Describes the four-stage pipeline, offline operation, no database/state. Edges: references→compiler-pipeline, references→schema-design, references→anchor-syntax, references→rendering, references→tutoring-errors.
  (s) service/cli.node — five subcommands: init, validate (--focus, --first, --json), context (--phase), plan, diff (--from). Each described with purpose and flags. Edges: references→ai-protocol.
  
  **test:**
  (t) test/test.index — index. Contains compiler-tests.
  (u) test/compiler-tests.node — three levels: unit tests per validation pass, integration tests with synthetic graphs, self-hosting test (compiler validates its own graph/ in CI). Edges: references→compiler-requirement.
  
  Every node must be valid. Every edge must have a corresponding [edge:<id>] in the body. The graph must compile with zero errors. Must NOT create nodes for features that don't exist yet. The graph IS the documentation — a human browsing the generated HTML must be able to understand the full project.
  Parallelization: Wave 5 | Blocked by: 15 | Blocks: 17
  References: Converged design — .node format, index structure, body-edge syntax, the exact kind/edge names from this planning session. This is the self-hosting seed and the permanent record.
  Acceptance criteria (agent-executable): graphite validate on graph/ returns zero errors. graphite context compiler --phase understand returns correct JSON. graphite plan compiler returns ordered work order. HTML renderer produces navigable docs that cover the full project.
  QA scenarios (agent-executable): happy: full validation passes, every [edge:<id>] resolves. failure: remove a reference, re-validate catches it. Evidence .omo/evidence/task-16-graphite-v1-compiler.txt
  Commit: Y | doc(graph): create self-describing 21-node seed graph for Graphite
  Every node must be valid. Every edge must have a corresponding [edge:<id>] in the body. The graph must compile with zero errors. Must NOT create nodes for features that don't exist yet (no v2 scope creep). The graph IS the documentation — it must be readable by a human browsing the generated HTML.
  Parallelization: Wave 5 | Blocked by: 15 | Blocks: 17
  References: Converged design — .node format, index structure, body-edge syntax. This is the self-hosting seed and the permanent record.
  Acceptance criteria (agent-executable): graphite validate on graph/ returns zero errors. graphite context compiler returns correct read/modify/validate sections. HTML renderer produces navigable docs for Graphite itself that a human auditor can read to understand the project.
  QA scenarios (agent-executable): happy: full validation passes. failure: remove an [edge:...] reference, re-validate catches it. Evidence .omo/evidence/task-16-graphite-v1-compiler.txt
  Commit: Y | doc(graph): create self-describing seed graph for Graphite

- [ ] 17. Wire self-hosting validation in CI
  What to do / Must NOT do: Add a CI step (GitHub Actions or similar) that runs graphite validate on the graph/ directory after every build. Add graphite validate to the Rust test suite as an integration test: build the binary, run validate on the project's own graph/, assert exit 0. Must NOT be a separate CI pipeline — it's part of the existing cargo test suite. Must NOT publish artifacts.
  Parallelization: Wave 6 | Blocked by: 16 | Blocks: 18
  References: The Graphite repo is the target audience. It validates itself.
  Acceptance criteria (agent-executable): cargo test includes an integration test that runs ./target/release/graphite validate on the project root and passes. Intentionally breaking a .node file causes the test to fail.
  QA scenarios (agent-executable): happy: CI passes with valid graph. failure: broken graph fails the integration test. Evidence .omo/evidence/task-17-graphite-v1-compiler.txt
  Commit: Y | ci: add self-hosting validation to test suite

- [ ] 18. Integration test: full pipeline from seed graph to rendered HTML
  What to do / Must NOT do: End-to-end test: (a) run graphite init to create a temp project, (b) add a few .node files with edges and body references, (c) add a source file with @graphite:evidence, (d) run graphite validate — assert zero errors, (e) run graphite context --phase implement on a node — assert JSON output matches expected structure, (f) run graphite plan on a node — assert work order has ordered steps, (g) run renderer — assert HTML output exists with correct links and heading levels. Must NOT test against the real Graphite graph (that's todo 17) — use a synthetic mini-graph.
  Parallelization: Wave 6 | Blocked by: 15 | Blocks: 19
  References: Full pipeline from design. This validates every component works together.
  Acceptance criteria (agent-executable): cargo test runs the full pipeline test. Each assertion (validate ok, context returns JSON, plan returns work order, HTML exists with correct content) passes.
  QA scenarios (agent-executable): happy: full pipeline succeeds. failure: each component failure mode tested (broken schema, missing anchor, unused edge). Evidence .omo/evidence/task-18-graphite-v1-compiler.txt
  Commit: Y | test: add end-to-end pipeline integration test

- [ ] 19. Final verification wave
  What to do / Must NOT do: Run all four verification items: F1. Plan compliance audit — verify every item in Scope IN is implemented, nothing in Scope OUT is present. F2. Code quality review — run cargo clippy, cargo fmt --check, no unwrap() in production code (only in tests), no panic paths. F3. Real manual QA — run the full pipeline on the self-describing graph, verify HTML output manually. F4. Scope fidelity — confirm no features beyond v1 scope crept in. Must NOT introduce new features — only verify.
  Parallelization: Final | Blocked by: 18 | Blocks: —
  References: ALL scope boundaries from this plan.
  Acceptance criteria (agent-executable): All four verification items pass. Document results in .omo/evidence/final-verification.txt.
  QA scenarios (agent-executable): F1: checklist against Scope IN/OUT. F2: cargo clippy and fmt. F3: inspect rendered HTML for correctness. F4: grep for any files or patterns not in scope. Evidence .omo/evidence/final-verification.txt
  Commit: N | (verification only, no code changes)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- One commit per todo, prefixed by conventional commit type (feat, doc, ci, test, refactor)
- No squashing — preserve the construction history for audit
- The seed graph commit (todo 16) is the first commit that makes graphite validate pass on its own graph

## Success criteria
- `graphite validate` runs on the Graphite repo's own `graph/` directory and exits 0
- `graphite validate --first` shows exactly one tutored error with detail+fix+example+hint
- `graphite validate --focus <id>` shows only diagnostics for that node
- `graphite context <id> --phase implement` returns JSON with modify section, source files, and existing anchors
- `graphite plan <id>` returns ordered work order following two-phase protocol
- `graphite diff` shows knowledge-level changes between git refs
- `graphite init` creates a valid compilable project
- HTML renderer produces navigable documentation with correct heading hierarchy and resolved [edge:<id>] links
- Graphite describes itself: the seed graph documents the v1 compiler requirements, architecture decisions, design rationale, AI protocol, tutoring error design, services, and tests
- Deleting `.omo/` loses nothing — all essential knowledge survives in `graph/`
