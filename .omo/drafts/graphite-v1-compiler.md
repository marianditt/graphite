---
slug: graphite-v1-compiler
status: awaiting-approval
intent: clear
pending-action: write .omo/plans/graphite-v1-compiler.md ✅ done
approach: Build the Graphite v1 compiler in Rust from the converged design. Phase 0 seeds the self-describing graph. Phase 1 builds the data model and parser. Phase 2 builds validation. Phase 3 builds the CLI. Phase 4 builds the renderer. Phase 5 wires the self-hosting loop. Phase 6 documents the system in Graphite itself.
---

# Draft: graphite-v1-compiler

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
<!-- id | outcome (one line) | status: active|deferred | evidence path -->
| C1. Graph data model & parser | Parse .node files + graph.schema into typed graph | active | compiler/src/model/, compiler/src/parse/
| C2. Validator | Schema validation, graph validation, body-edge usage, anchor resolution | active | compiler/src/validate/
| C3. Anchor scanner | Scan source files for @graphite:evidence + .graphite sidecars | active | compiler/src/anchors/
| C4. CLI (init, validate, context, diff) | CLI subcommands with JSON + human output | active | compiler/src/cli/
| C5. HTML renderer | Generate navigable docs with heading depth derivation | active | compiler/src/render/
| C6. Seed graph | .node files describing Graphite itself (self-hosting) | active | graph/
| C7. Self-hosting loop | Compiler validates its own graph | active | CI integration

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
| assumption | adopted default | rationale | reversible? |
|---|---|---|---|
| Build tool | Cargo (Rust) + clap + serde + pulldown-cmark + tera/askama | Mature ecosystem, single binary, fast | Yes - can swap renderer lib |
| Anchor scanning | Line-based regex matching (not AST) | Simpler, language-agnostic, sufficient for @graphite:evidence | Yes - can add AST plugins later |
| HTML rendering | Static files, no JS runtime | Correctness over interactivity | Yes - can add later |
| Node body processing | Body is processed by Graphite compiler before Markdown renderer | [edge:<id>] substitution + heading offset | N/A - core design |

## Findings (cited - path:lines)
All design decisions are from the converged discussion with the user (no codebase findings — the repo is empty).

## Decisions (with rationale)
| Decision | Rationale |
|---|---|
| Rust for implementation | Single binary, fast, algebraic types model the graph, serde for YAML/JSON, clap for CLI |
| .node extension | One file = one node, grep-friendly, unambiguous |
| [edge:<id>] in body | Edges are narrative — author places them where they belong, not auto-rendered |
| @graphite:evidence in source | Colocation with evidence, moves with refactoring, compiler verifies existence |
| .graphite sidecar for JSON/txt | Same resolution mechanism, no source pollution for comment-less formats |
| Heading depth from containment tree | Author writes # always, compiler offsets by depth, no manual heading decisions |
| No anchor block | Edges are the anchor — verified_by targets evidence:id directly |
| Sub-indices | Arbitrary nesting within a kind via index nodes containing index nodes |
| Auto-IDs are cosmetic | REQ-1 etc. are display labels assigned in DFS order; canonical URLs use the stable `id:` |
| Edge kind naming | Past-participle adjectives (implemented_by, verified_by) — read naturally after "is" in prose |
| Node kind naming | requirement, adr, service, test — minimal set, each with a key for auto-numbering |
| Schema defines kinds + edges | graph.schema is the type system; contains and evidence are built-in |
| 21-node seed graph | Every essential concept in its own node, edges resolve cleanly, self-hosting |

## Scope IN
- Rust project scaffolding (Cargo workspace)
- Graph data model: Node, Edge, Kind, Schema, Index, Graph
- Diagnostic tutoring type: rule, severity, node_id, file, detail, fix, example, hint
- .node file parser (YAML frontmatter + Markdown body split)
- graph.schema parser
- Validation: reachability, tree constraint on contains, schema edge type conformance, body-edge usage, no cycles
- Tutoring diagnostics for all validation errors (every error teaches: what rule, what's wrong, how to fix, correct example, general principle)
- Anchor scanner: @graphite:evidence in source files, .graphite sidecar files
- CLI: graphite init, graphite validate (with --focus, --first), graphite context (with --phase), graphite plan, graphite diff
- HTML renderer with heading depth derivation
- The seed graph: .node files describing Graphite itself (requirements, architecture, services, tests, decisions)
- graphite.yaml config file

## Scope OUT (Must NOT have)
- No interactive HTML (no JS, no live graph visualization)
- No NLP/AI integration in graphite plan (plan is pure graph traversal)
- No Mermaid diagram generation (v2)
- No file watching / live reload
- No AST-level anchor extraction (line-based regex only)
- No external database / persistence — graph is ephemeral, rebuilt from .node files
- No plugin system (v2)

## Constraint: persistence through the graph
.omo/ is ephemeral. Everything that must survive deletion of .omo/ lives in graph/. This includes:
- Every design decision from this planning session
- All architecture rationale
- The AI protocol (commands, expected JSON shapes)
- Schema definitions for Graphite itself
- Requirements, architecture, service, and test nodes describing Graphite

The seed graph (C6) is the canonical record. The plan file is temporary scaffolding.

## Open questions
None — all design questions settled.

## Approval gate
status: awaiting-approval
<!-- All exploration exhausted, all unknowns answered, plan written. User has seen the brief. -->
