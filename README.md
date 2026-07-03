# Graphite

Graphite is a compiled knowledge graph CLI for software engineering workflows.

## CLI usage

After installation, use:

```bash
graphite render
graphite validate graph
graphite context compiler --phase implement
```

## Global install with npm

Graphite can be installed globally via npm and exposes a `graphite` command.

### Prerequisites

- Rust toolchain installed (`cargo` available in `PATH`)
- Node.js 18+

### Install globally

```bash
npm i -g @marianditt/graphite
```

The package builds `graphite-cli` during postinstall and wires a global
`graphite` executable.

### Local smoke test for the npm wrapper

From this repo:

```bash
node npm/scripts/postinstall.mjs
node npm/bin/graphite.mjs --help
```

## Publishing to npm

Publishing requires your npm credentials and should be run by you.

1. Log in:

```bash
npm login
```

2. Verify package contents:

```bash
npm pack --dry-run
```

3. Publish:

```bash
npm publish --access public
```

If you publish under a different scope, update `name` in `package.json` and
use that package name for installs.
