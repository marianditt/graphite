#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..', '..');
const binName = os.platform() === 'win32' ? 'graphite.exe' : 'graphite';
const projectBin = path.join(
  repoRoot,
  'target',
  'x86_64-unknown-linux-musl',
  'release',
  os.platform() === 'win32' ? 'graphite-cli.exe' : 'graphite-cli'
);
const packageBin = path.join(repoRoot, 'npm', 'bin', binName);

const candidates = [projectBin, packageBin];
const graphiteBin = candidates.find((candidate) => fs.existsSync(candidate));

if (!graphiteBin) {
  console.error(
    '[graphite] binary not found. Installing global package should build it via postinstall.'
  );
  console.error('[graphite] try reinstalling: npm i -g @marianditt/graphite');
  process.exit(1);
}

const result = spawnSync(graphiteBin, process.argv.slice(2), {
  stdio: 'inherit'
});

if (result.error) {
  console.error(`[graphite] failed to execute binary: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
