// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Collects the licence texts of everything that ends up in the Sorobo programs (Rust crates,
// npm packages of the frontend, bundled fonts) into one file for binary distributions.
//
// usage: node scripts/third-party-licenses.mjs [output file]   (default: dist/THIRD-PARTY-LICENSES.txt)

import { execFileSync, execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(root, process.argv[2] ?? 'dist/THIRD-PARTY-LICENSES.txt');
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$/i;

/** Licence texts found in a package directory (top level only). */
function licenseTexts(dir) {
  let names = [];
  try {
    names = fs.readdirSync(dir).filter((f) => LICENSE_FILE.test(f) && fs.statSync(path.join(dir, f)).isFile());
  } catch {
    return [];
  }
  return names.sort().map((f) => fs.readFileSync(path.join(dir, f), 'utf8').replace(/\r\n/g, '\n').trim());
}

function rustPackages() {
  const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)[1];
  const meta = JSON.parse(
    execFileSync('cargo', ['metadata', '--locked', '--format-version', '1', '--filter-platform', host], {
      cwd: path.join(root, 'backend'),
      encoding: 'utf8',
      maxBuffer: 1 << 28,
    }),
  );
  // Everything reachable from the Sorobo package through normal (non-dev) dependencies.
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const own = meta.packages.find((p) => p.name === 'sorobo');
  const reachable = new Set();
  const visit = (id) => {
    if (reachable.has(id)) return;
    reachable.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((k) => k.kind !== 'dev')) visit(dep.pkg);
    }
  };
  visit(own.id);
  return meta.packages
    .filter((p) => reachable.has(p.id) && p.id !== own.id)
    .map((p) => ({
      name: `${p.name} ${p.version}`,
      license: p.license ?? 'siehe Lizenzdatei',
      url: p.repository ?? '',
      texts: licenseTexts(path.dirname(p.manifest_path)),
    }));
}

function npmPackages() {
  const frontend = path.join(root, 'frontend');
  const dirs = execSync('npm ls --omit=dev --all --parseable', { cwd: frontend, encoding: 'utf8', maxBuffer: 1 << 26 })
    .trim()
    .split(/\r?\n/)
    .slice(1);
  const seen = new Map();
  for (const dir of dirs) {
    const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
    const name = `${pkg.name} ${pkg.version}`;
    if (seen.has(name)) continue;
    const license = typeof pkg.license === 'string' ? pkg.license : (pkg.licenses ?? []).map((l) => l.type ?? l).join(' OR ') || 'siehe Lizenzdatei';
    const repo = typeof pkg.repository === 'string' ? pkg.repository : (pkg.repository?.url ?? '');
    seen.set(name, { name, license, url: repo, texts: licenseTexts(dir) });
  }
  return [...seen.values()];
}

/** Groups packages with identical licence texts, so each text appears once. */
function render(title, packages) {
  const groups = new Map();
  const withoutText = [];
  for (const p of packages.sort((a, b) => a.name.localeCompare(b.name))) {
    if (p.texts.length === 0) {
      withoutText.push(p);
      continue;
    }
    // Each distinct text once (e.g. the Apache licence of hundreds of dual-licensed crates);
    // differences in line breaks/indentation do not count.
    for (const text of p.texts) {
      const key = text.replace(/\s+/g, ' ');
      if (!groups.has(key)) groups.set(key, { text, list: [] });
      groups.get(key).list.push(p);
    }
  }
  const rule = '='.repeat(80);
  let out = `\n${rule}\n${title} (${packages.length})\n${rule}\n`;
  for (const { text, list } of groups.values()) {
    out += `\n${'-'.repeat(80)}\n`;
    out += list.map((p) => `${p.name} – ${p.license}${p.url ? ` – ${p.url}` : ''}`).join('\n');
    out += `\n${'-'.repeat(80)}\n\n${text}\n`;
  }
  if (withoutText.length) {
    out += `\n${'-'.repeat(80)}\nOhne eigene Lizenzdatei im Paket (Lizenz laut Paketangabe; Standardtexte unter https://spdx.org/licenses/):\n${'-'.repeat(80)}\n`;
    out += withoutText.map((p) => `${p.name} – ${p.license}${p.url ? ` – ${p.url}` : ''}`).join('\n') + '\n';
  }
  return out;
}

const rust = rustPackages();
const npm = npmPackages();
const fonts = fs.readFileSync(path.join(root, 'backend/assets/fonts/LICENSE-FONTS.txt'), 'utf8').replace(/\r\n/g, '\n');

let text = `Sorobo – Lizenzen von Drittanbieter-Komponenten
================================================

Sorobo selbst steht unter der GNU Affero General Public License, Version 3 oder später
(siehe LICENSE). Die Programme enthalten die folgenden Komponenten Dritter, die jeweils unter
ihren eigenen Lizenzen stehen.
`;
text += render('Rust-Bibliotheken (Backend, Desktop-Anwendung)', rust);
text += render('JavaScript-Bibliotheken (Benutzeroberfläche)', npm);
text += `\n${'='.repeat(80)}\nSchriften\n${'='.repeat(80)}\n\n${fonts}`;

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, text);
console.log(`${path.relative(root, output)}: ${rust.length} Rust-Pakete, ${npm.length} npm-Pakete, ${(text.length / 1024).toFixed(0)} KB`);
