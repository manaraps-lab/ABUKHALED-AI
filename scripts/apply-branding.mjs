#!/usr/bin/env node
/**
 * First migration-safe ABUKHALED AI desktop identity layer.
 * Preserves the upstream appId and existing data directories.
 * This script intentionally fails closed if the pinned source structure drifts.
 */
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.argv[2] || 'upstream');
const electronDir = path.join(root, 'electron');
const packagePath = path.join(electronDir, 'package.json');
const mainPath = path.join(electronDir, 'main.js');

function fail(message) {
  console.error(`[ABUKHALED branding] ERROR: ${message}`);
  process.exit(1);
}
function read(file) {
  if (!fs.existsSync(file)) fail(`Required source file not found: ${file}`);
  return fs.readFileSync(file, 'utf8');
}
function replaceExactly(source, before, after, label) {
  const count = source.split(before).length - 1;
  if (count === 0 && source.includes(after)) return source;
  if (count !== 1) fail(`${label}: expected exactly one source match, found ${count}`);
  return source.replace(before, after);
}
function injectFunctionGuard(source, functionName, declaration, guard, label) {
  const marker = `/* ABUKHALED_GUARD:${functionName} */`;
  if (source.includes(marker)) return source;
  const matches = [...source.matchAll(declaration)];
  if (matches.length !== 1) {
    fail(`${label}: expected exactly one ${functionName} function declaration, found ${matches.length}`);
  }
  const match = matches[0];
  const braceOffset = match.index + match[0].lastIndexOf('{') + 1;
  return source.slice(0, braceOffset) +
    `\n  ${marker}\n  ${guard}\n` +
    source.slice(braceOffset);
}

const pkg = JSON.parse(read(packagePath));
if (pkg.version !== '3.8.53') fail(`Unexpected Electron package version: ${pkg.version}`);
if (!pkg.build || pkg.build.appId !== 'online.omniroute.desktop') {
  fail('Electron appId differs from the reviewed upstream baseline; stop for data-path review.');
}
pkg.build.productName = 'ABUKHALED AI';
pkg.build.copyright = 'Copyright © 2026 ABUKHALED AI';
delete pkg.build.publish;

// The reviewed upstream CI log shows electron/scripts build:win invokes
// `electron-builder --win` without a publish policy. Add the explicit policy
// at that source-of-truth script so it cannot be lost through npm argument forwarding.
if (!pkg.scripts || typeof pkg.scripts['build:win'] !== 'string') {
  fail('Electron build:win script is missing; stop for source review.');
}
const expectedBuildWin = 'npm run prepare:bundle && electron-builder --win';
const safeBuildWin = 'npm run prepare:bundle && electron-builder --win --publish never';
if (pkg.scripts['build:win'] === expectedBuildWin) {
  pkg.scripts['build:win'] = safeBuildWin;
} else if (pkg.scripts['build:win'] !== safeBuildWin) {
  fail(`Unexpected Electron build:win script; refusing unsafe build: ${pkg.scripts['build:win']}`);
}
fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

let main = read(mainPath);
const replacements = [
  ['title: "OmniRoute",', 'title: "ABUKHALED AI",', 'window title'],
  ['label: "Open OmniRoute",', 'label: "Open ABUKHALED AI",', 'tray menu label'],
  ['tray.setToolTip("OmniRoute");', 'tray.setToolTip("ABUKHALED AI");', 'tray tooltip'],
  ['title: "OmniRoute Update Ready",', 'title: "ABUKHALED AI Update Ready",', 'update notification title'],
  ['"Name=OmniRoute",', '"Name=ABUKHALED AI",', 'desktop autostart name'],
  ['"Comment=OmniRoute Desktop Client",', '"Comment=ABUKHALED AI Desktop Client",', 'desktop autostart description'],
];
for (const [before, after, label] of replacements) {
  main = replaceExactly(main, before, after, label);
}

/*
 * Guard updater entry points by function declaration rather than matching a
 * fragile multi-line body. The guard is inserted immediately inside each
 * function, so upstream formatting changes do not silently bypass it.
 */
main = injectFunctionGuard(
  main,
  'checkForUpdates',
  /async\s+function\s+checkForUpdates\s*\([^)]*\)\s*\{/g,
  'console.info("[ABUKHALED AI] Updates are disabled until the official release channel is configured.");\n  sendToRenderer("update-status", { status: "not-available", message: "ABUKHALED AI updates are not configured yet." });\n  return;',
  'disable upstream auto-update checks'
);
main = injectFunctionGuard(
  main,
  'downloadUpdate',
  /async\s+function\s+downloadUpdate\s*\([^)]*\)\s*\{/g,
  'throw new Error("ABUKHALED AI update channel is not configured.");',
  'guard upstream update downloads'
);
main = injectFunctionGuard(
  main,
  'installUpdate',
  /function\s+installUpdate\s*\([^)]*\)\s*\{/g,
  'console.warn("[ABUKHALED AI] Update installation is disabled until the official release channel is configured.");\n  return;',
  'guard upstream update installation'
);
fs.writeFileSync(mainPath, main);

const finalPkg = JSON.parse(read(packagePath));
const finalMain = read(mainPath);
if (finalPkg.build.productName !== 'ABUKHALED AI') fail('productName postcondition failed');
if (finalPkg.build.appId !== 'online.omniroute.desktop') fail('appId must remain unchanged');
if (Object.hasOwn(finalPkg.build, 'publish')) fail('upstream publish configuration still exists');
if (finalPkg.scripts?.['build:win'] !== 'npm run prepare:bundle && electron-builder --win --publish never') fail('build:win must explicitly disable publishing');
for (const expected of [
  'title: "ABUKHALED AI"',
  'label: "Open ABUKHALED AI"',
  'tray.setToolTip("ABUKHALED AI")',
  'ABUKHALED_GUARD:checkForUpdates',
  'ABUKHALED_GUARD:downloadUpdate',
  'ABUKHALED_GUARD:installUpdate',
]) {
  if (!finalMain.includes(expected)) fail(`Branding/updater postcondition failed: ${expected}`);
}
console.log('[ABUKHALED branding] Applied and verified.');
console.log(`Product: ${finalPkg.build.productName}`);
console.log(`Version: ${finalPkg.version}`);
console.log(`Preserved appId: ${finalPkg.build.appId}`);
console.log('Upstream publish target: removed');
console.log('Upstream auto-update checks/download/install: guarded');
