# Agent Instructions

This repository uses Graphite.

Graphite is the authoritative source of engineering knowledge, including requirements, architecture, decisions, documentation, tests, and implementation context.

Do not discover project structure by searching files first. Use Graphite to obtain relevant context.

## Before starting work

Understand the available commands:

`graphite help`

Get task-specific knowledge:

`graphite context <node-id>`

For implementation or modification tasks, create a work plan:

`graphite plan <node-id>`

Follow the generated plan before editing files.

## During work

Respect all Graphite-provided:

- requirements
- architecture decisions
- technical constraints
- implementation guidance
- validation expectations

Update affected knowledge nodes together with code changes.

## Before finishing work

Validate the knowledge graph:

`graphite validate`

Review knowledge-level changes when applicable:

`graphite diff`

Ensure the graph remains complete, consistent, and traceable.

The repository is correct only when the implementation and the Graphite knowledge graph agree.
