#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..', '..');

const cargoVersion = spawnSync('cargo', ['--version'], {
  stdio: 'pipe',
  encoding: 'utf8'
});

if (cargoVersion.status !== 0) {
  console.error('[graphite] Cargo is required to build graphite-cli during npm install.');
  console.error('[graphite] Install Rust from https://rustup.rs and retry.');
  process.exit(1);
}

const build = spawnSync('cargo', ['build', '--release', '-p', 'graphite-cli'], {
  cwd: repoRoot,
  stdio: 'inherit'
});

if (build.status !== 0) {
  console.error('[graphite] Failed to build graphite-cli binary.');
  process.exit(build.status ?? 1);
}

const sourceBin = path.join(
  repoRoot,
  'target',
  'x86_64-unknown-linux-musl',
  'release',
  os.platform() === 'win32' ? 'graphite-cli.exe' : 'graphite-cli'
);

const destDir = path.join(repoRoot, 'npm', 'bin');
const destBin = path.join(
  destDir,
  os.platform() === 'win32' ? 'graphite.exe' : 'graphite'
);

if (!fs.existsSync(sourceBin)) {
  console.error(`[graphite] Built binary not found at ${sourceBin}`);
  process.exit(1);
}

fs.mkdirSync(destDir, { recursive: true });
fs.copyFileSync(sourceBin, destBin);
if (os.platform() !== 'win32') {
  fs.chmodSync(destBin, 0o755);
}

console.log(`[graphite] installed binary: ${destBin}`);
